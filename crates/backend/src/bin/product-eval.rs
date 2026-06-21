#[path = "../canonical.rs"]
mod canonical;
#[path = "../delta_policy.rs"]
mod delta_policy;
#[allow(dead_code)]
#[path = "../models.rs"]
mod models;

use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context};
use delta_policy::{
    evaluate_delta_eligibility, BaselineEvidence, DeltaEligibilityDecision,
    DeltaEligibilityDiagnostics,
};
use models::{CandidateKind, CanonicalReadingUnit, Priority, ReaderDirective};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct ProductBaseline {
    reader_profile: String,
    #[serde(default)]
    known_concepts: Vec<String>,
    #[serde(default)]
    familiar_claims: Vec<String>,
    #[serde(default)]
    interests: Vec<String>,
    #[serde(default)]
    explicit_not_interested_topics: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct FixtureMetadata {
    fixture_type: String,
    baseline_name: String,
    #[serde(default)]
    thresholds: FixtureThresholds,
}

#[derive(Debug, Default, Deserialize)]
struct FixtureThresholds {
    delta_precision_min: Option<f64>,
    delta_recall_min: Option<f64>,
    familiar_suppression_rate_min: Option<f64>,
    known_as_delta_rate_max: Option<f64>,
    visible_guidance_per_page_max: Option<f64>,
    false_negative_delta_count_max: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct GoldLabels {
    labels: Vec<GoldLabel>,
}

#[derive(Debug, Deserialize)]
struct GoldLabel {
    sentence_id: String,
    expected_priority: Priority,
    expected_directive: ReaderDirective,
    reason: String,
}

#[derive(Clone, Debug)]
struct FixturePrediction {
    priority: Priority,
    directive: ReaderDirective,
    confidence: Option<f64>,
    signals: BaselineOverlapSignals,
    gate_decision: DeltaEligibilityDecision,
}

#[derive(Clone, Debug, Default, Serialize)]
struct BaselineOverlapSignals {
    matched_known_concepts: Vec<String>,
    matched_familiar_claims: Vec<String>,
    matched_interests: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
struct ErrorDetail {
    sentence_id: String,
    page: usize,
    candidate_kind: String,
    sentence_text: String,
    predicted_priority: String,
    predicted_directive: String,
    expected_priority: String,
    expected_directive: String,
    confidence: Option<f64>,
    gold_reason: String,
    likely_cause: String,
    baseline_overlap: BaselineOverlapSignals,
    gate_decision: DeltaEligibilityDecision,
}

#[derive(Debug, Serialize)]
struct ProductErrorReport {
    fixture: String,
    fixture_type: String,
    baseline_name: String,
    metrics: EvaluationMetrics,
    false_positive_deltas: Vec<ErrorDetail>,
    false_negative_deltas: Vec<ErrorDetail>,
    familiar_highlight_errors: Vec<ErrorDetail>,
    bridge_confusions: Vec<ErrorDetail>,
    over_annotation_errors: Vec<ErrorDetail>,
    delta_gate_affected: Vec<ErrorDetail>,
}

#[derive(Clone, Debug, Serialize)]
struct EvaluationMetrics {
    delta_precision: RatioMetric,
    delta_recall: RatioMetric,
    familiar_suppression_rate: f64,
    known_as_delta_rate: f64,
    bridge_precision: RatioMetric,
    visible_guidance_per_page: f64,
    highlight_per_page: f64,
    callout_per_page: f64,
    soft_fade_per_page: f64,
    body_sentence_count: usize,
    gold_coverage_ratio: f64,
    false_positive_delta_count: usize,
    false_negative_delta_count: usize,
    familiar_highlight_count: usize,
    over_annotation_score: f64,
}

#[derive(Clone, Debug, Serialize)]
struct RatioMetric {
    value: Option<f64>,
    numerator: usize,
    denominator: usize,
}

fn main() -> anyhow::Result<()> {
    let fixture_dir = parse_fixture_dir()?;
    let pdf_path = fixture_dir.join("document.pdf");
    let baseline_path = fixture_dir.join("baseline.json");
    let gold_path = fixture_dir.join("gold_labels.json");
    let metadata_path = fixture_dir.join("fixture.json");

    let pdf_bytes =
        fs::read(&pdf_path).with_context(|| format!("failed to read {}", pdf_path.display()))?;
    let baseline: ProductBaseline = read_json(&baseline_path)?;
    let gold: GoldLabels = read_json(&gold_path)?;
    let metadata: FixtureMetadata = read_json(&metadata_path)?;
    if !matches!(
        metadata.fixture_type.as_str(),
        "smoke" | "realistic" | "adversarial"
    ) {
        bail!("fixture_type must be smoke, realistic, or adversarial");
    }
    let model = canonical::extract_canonical_text_model(&pdf_bytes, Uuid::nil())?;
    let candidates = canonical::generate_canonical_reading_units(&model)?;
    let predictions = analyze_fixture(&candidates, &baseline);

    validate_gold(&gold.labels, &candidates)?;
    let metrics = print_report(
        &fixture_dir,
        &metadata,
        &baseline,
        &model,
        &candidates,
        &predictions,
        &gold.labels,
    );
    let error_report = build_error_report(
        &fixture_dir,
        &metadata,
        &candidates,
        &predictions,
        &gold.labels,
        &metrics,
    );
    print_error_analysis(&error_report);
    write_error_reports(&error_report)?;
    enforce_thresholds(&metadata.thresholds, &metrics)?;
    Ok(())
}

fn parse_fixture_dir() -> anyhow::Result<PathBuf> {
    let mut args = env::args().skip(1);
    let Some(path) = args.next() else {
        bail!("usage: product-eval <resource/product-fixtures/FIXTURE>");
    };
    if let Some(extra) = args.next() {
        bail!("unexpected argument {extra}");
    }
    let path = PathBuf::from(path);
    for required in [
        "document.pdf",
        "baseline.json",
        "gold_labels.json",
        "fixture.json",
        "README.md",
    ] {
        if !path.join(required).is_file() {
            bail!("fixture is missing {required}: {}", path.display());
        }
    }
    Ok(path)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> anyhow::Result<T> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("invalid JSON in {}", path.display()))
}

fn analyze_fixture(
    candidates: &[CanonicalReadingUnit],
    baseline: &ProductBaseline,
) -> HashMap<String, FixturePrediction> {
    candidates
        .iter()
        .filter(|candidate| matches!(candidate.candidate_kind, CandidateKind::BodySentence))
        .map(|candidate| {
            (
                candidate.sentence_id.clone(),
                predict_candidate(&candidate.normalized_text, baseline),
            )
        })
        .collect()
}

fn predict_candidate(text: &str, baseline: &ProductBaseline) -> FixturePrediction {
    let excluded = maximum_phrase_score(text, &baseline.explicit_not_interested_topics);
    let familiar_claim = maximum_phrase_score(text, &baseline.familiar_claims);
    let known = maximum_phrase_score(text, &baseline.known_concepts);
    let interest = maximum_phrase_score(text, &baseline.interests);
    let signals = BaselineOverlapSignals {
        matched_known_concepts: matched_phrases(text, &baseline.known_concepts),
        matched_familiar_claims: matched_phrases(text, &baseline.familiar_claims),
        matched_interests: matched_phrases(text, &baseline.interests),
    };

    let (priority, directive, confidence) = if excluded >= 0.6 {
        (Priority::Familiar, ReaderDirective::LeaveNormal, excluded)
    } else if interest >= 0.55 && known >= 0.55 {
        (
            Priority::Bridge,
            ReaderDirective::Callout,
            interest.min(known),
        )
    } else if interest >= 0.55 {
        (Priority::Delta, ReaderDirective::Highlight, interest)
    } else if familiar_claim >= 0.6 {
        (
            Priority::Familiar,
            ReaderDirective::SoftFade,
            familiar_claim,
        )
    } else if known >= 0.6 {
        (Priority::Familiar, ReaderDirective::LeaveNormal, known)
    } else if baseline.reader_profile.to_lowercase().contains("beginner") {
        (Priority::Bridge, ReaderDirective::Callout, 0.5)
    } else {
        (Priority::Bridge, ReaderDirective::LeaveNormal, 0.5)
    };
    let mut prediction = FixturePrediction {
        priority,
        directive,
        confidence: Some(confidence.clamp(0.0, 1.0)),
        signals,
        gate_decision: DeltaEligibilityDecision::default(),
    };
    prediction.gate_decision = evaluate_delta_eligibility(
        text,
        matches!(prediction.priority, Priority::Delta)
            && matches!(prediction.directive, ReaderDirective::Highlight),
        BaselineEvidence {
            known_concepts: &baseline.known_concepts,
            familiar_claims: &baseline.familiar_claims,
            interests: &baseline.interests,
        },
    );
    if prediction.gate_decision.demoted_by_familiar_claim {
        prediction.priority = Priority::Familiar;
        prediction.directive = ReaderDirective::LeaveNormal;
    }
    prediction
}

fn matched_phrases(text: &str, phrases: &[String]) -> Vec<String> {
    phrases
        .iter()
        .filter(|phrase| phrase_score(text, phrase) >= 0.55)
        .cloned()
        .collect()
}

fn maximum_phrase_score(text: &str, phrases: &[String]) -> f64 {
    phrases
        .iter()
        .map(|phrase| phrase_score(text, phrase))
        .fold(0.0, f64::max)
}

fn phrase_score(text: &str, phrase: &str) -> f64 {
    let normalized_text = normalize(text);
    let normalized_phrase = normalize(phrase);
    if normalized_phrase.is_empty() {
        return 0.0;
    }
    if normalized_text.contains(&normalized_phrase) {
        return 1.0;
    }
    let text_tokens = tokens(&normalized_text);
    let phrase_tokens = tokens(&normalized_phrase);
    if phrase_tokens.is_empty() {
        0.0
    } else {
        phrase_tokens.intersection(&text_tokens).count() as f64 / phrase_tokens.len() as f64
    }
}

fn normalize(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn tokens(value: &str) -> HashSet<&str> {
    const STOP: &[&str] = &[
        "a", "an", "and", "as", "at", "by", "for", "from", "in", "of", "on", "the", "to", "with",
    ];
    value
        .split_whitespace()
        .filter(|token| token.len() > 1 && !STOP.contains(token))
        .collect()
}

fn validate_gold(gold: &[GoldLabel], candidates: &[CanonicalReadingUnit]) -> anyhow::Result<()> {
    let candidates_by_id = candidates
        .iter()
        .map(|candidate| (candidate.sentence_id.as_str(), candidate))
        .collect::<HashMap<_, _>>();
    let mut seen = HashSet::new();
    for label in gold {
        let Some(candidate) = candidates_by_id.get(label.sentence_id.as_str()) else {
            bail!(
                "gold label references unknown sentence_id {}",
                label.sentence_id
            );
        };
        if !matches!(candidate.candidate_kind, CandidateKind::BodySentence) {
            bail!(
                "gold label {} targets {:?}, not body_sentence",
                label.sentence_id,
                candidate.candidate_kind
            );
        }
        if !seen.insert(label.sentence_id.as_str()) {
            bail!("gold label repeats sentence_id {}", label.sentence_id);
        }
    }
    for candidate in candidates
        .iter()
        .filter(|candidate| matches!(candidate.candidate_kind, CandidateKind::BodySentence))
    {
        if !seen.contains(candidate.sentence_id.as_str()) {
            bail!("gold labels omit body sentence {}", candidate.sentence_id);
        }
    }
    Ok(())
}

fn print_report(
    fixture_dir: &Path,
    metadata: &FixtureMetadata,
    baseline: &ProductBaseline,
    model: &models::CanonicalTextModel,
    candidates: &[CanonicalReadingUnit],
    predictions: &HashMap<String, FixturePrediction>,
    gold: &[GoldLabel],
) -> EvaluationMetrics {
    let labeled = gold
        .iter()
        .filter_map(|label| {
            predictions
                .get(&label.sentence_id)
                .map(|prediction| (label, prediction))
        })
        .collect::<Vec<_>>();
    let expected_delta = labeled
        .iter()
        .filter(|(label, _)| matches!(label.expected_priority, Priority::Delta))
        .count();
    let predicted_delta = labeled
        .iter()
        .filter(|(_, prediction)| matches!(prediction.priority, Priority::Delta))
        .count();
    let true_delta = labeled
        .iter()
        .filter(|(label, prediction)| {
            matches!(label.expected_priority, Priority::Delta)
                && matches!(prediction.priority, Priority::Delta)
        })
        .count();
    let false_positive_delta_count = labeled
        .iter()
        .filter(|(label, prediction)| {
            !matches!(label.expected_priority, Priority::Delta)
                && matches!(prediction.priority, Priority::Delta)
        })
        .count();
    let false_negative_delta_count = labeled
        .iter()
        .filter(|(label, prediction)| {
            matches!(label.expected_priority, Priority::Delta)
                && !matches!(prediction.priority, Priority::Delta)
        })
        .count();
    let expected_familiar = labeled
        .iter()
        .filter(|(label, _)| matches!(label.expected_priority, Priority::Familiar))
        .count();
    let suppressed_familiar = labeled
        .iter()
        .filter(|(label, prediction)| {
            matches!(label.expected_priority, Priority::Familiar)
                && matches!(
                    prediction.directive,
                    ReaderDirective::SoftFade | ReaderDirective::LeaveNormal
                )
        })
        .count();
    let known_as_delta = labeled
        .iter()
        .filter(|(label, prediction)| {
            matches!(label.expected_priority, Priority::Familiar)
                && matches!(prediction.priority, Priority::Delta)
        })
        .count();
    let familiar_highlight_count = labeled
        .iter()
        .filter(|(label, prediction)| {
            matches!(label.expected_priority, Priority::Familiar)
                && matches!(prediction.directive, ReaderDirective::Highlight)
        })
        .count();
    let predicted_bridge = labeled
        .iter()
        .filter(|(_, prediction)| matches!(prediction.priority, Priority::Bridge))
        .count();
    let true_bridge = labeled
        .iter()
        .filter(|(label, prediction)| {
            matches!(label.expected_priority, Priority::Bridge)
                && matches!(prediction.priority, Priority::Bridge)
        })
        .count();
    let page_count = model.pages.len().max(1) as f64;
    let visible = predictions
        .values()
        .filter(|prediction| !matches!(prediction.directive, ReaderDirective::LeaveNormal))
        .count();
    let directive_count = |directive| {
        predictions
            .values()
            .filter(|prediction| prediction.directive == directive)
            .count()
    };
    let over_annotated = labeled
        .iter()
        .filter(|(label, prediction)| {
            matches!(label.expected_directive, ReaderDirective::LeaveNormal)
                && !matches!(prediction.directive, ReaderDirective::LeaveNormal)
        })
        .count();
    let body_sentence_count = candidates
        .iter()
        .filter(|candidate| matches!(candidate.candidate_kind, CandidateKind::BodySentence))
        .count();
    let metrics = EvaluationMetrics {
        delta_precision: RatioMetric::new(true_delta, predicted_delta),
        delta_recall: RatioMetric::new(true_delta, expected_delta),
        familiar_suppression_rate: ratio_or_zero(suppressed_familiar, expected_familiar),
        known_as_delta_rate: ratio_or_zero(known_as_delta, expected_familiar),
        bridge_precision: RatioMetric::new(true_bridge, predicted_bridge),
        visible_guidance_per_page: visible as f64 / page_count,
        highlight_per_page: directive_count(ReaderDirective::Highlight) as f64 / page_count,
        callout_per_page: directive_count(ReaderDirective::Callout) as f64 / page_count,
        soft_fade_per_page: directive_count(ReaderDirective::SoftFade) as f64 / page_count,
        body_sentence_count,
        gold_coverage_ratio: ratio_or_zero(gold.len(), body_sentence_count),
        false_positive_delta_count,
        false_negative_delta_count,
        familiar_highlight_count,
        over_annotation_score: ratio_or_zero(over_annotated, gold.len()),
    };

    println!("Product-fit evaluation: {}", fixture_dir.display());
    println!("fixture_type: {}", metadata.fixture_type);
    println!("baseline_name: {}", metadata.baseline_name);
    println!("Reader profile: {}", baseline.reader_profile);
    println!("Pages: {}", model.pages.len());
    println!("Canonical sentence candidates: {}", candidates.len());
    println!("Gold labels: {}", gold.len());
    println!("Scored body sentences: {}", labeled.len());
    println!("body_sentence_count: {}", metrics.body_sentence_count);
    println!("gold_coverage_ratio: {:.3}", metrics.gold_coverage_ratio);
    println!("delta_precision: {}", metrics.delta_precision.display());
    println!("delta_recall: {}", metrics.delta_recall.display());
    println!(
        "familiar_suppression_rate: {:.3}",
        metrics.familiar_suppression_rate
    );
    println!("known_as_delta_rate: {:.3}", metrics.known_as_delta_rate);
    println!("bridge_precision: {}", metrics.bridge_precision.display());
    println!(
        "visible_guidance_per_page: {:.3}",
        metrics.visible_guidance_per_page
    );
    println!("highlight_per_page: {:.3}", metrics.highlight_per_page);
    println!("callout_per_page: {:.3}", metrics.callout_per_page);
    println!("soft_fade_per_page: {:.3}", metrics.soft_fade_per_page);
    println!(
        "false_positive_delta_count: {}",
        metrics.false_positive_delta_count
    );
    println!(
        "false_negative_delta_count: {}",
        metrics.false_negative_delta_count
    );
    println!(
        "familiar_highlight_count: {}",
        metrics.familiar_highlight_count
    );
    println!(
        "over_annotation_score: {:.3}",
        metrics.over_annotation_score
    );
    let mut gate_diagnostics = DeltaEligibilityDiagnostics::default();
    for prediction in predictions.values() {
        gate_diagnostics.record(&prediction.gate_decision);
    }
    println!(
        "delta_eligibility_checked: {}",
        gate_diagnostics.delta_eligibility_checked
    );
    println!(
        "delta_demoted_by_familiar_claim: {}",
        gate_diagnostics.delta_demoted_by_familiar_claim
    );
    println!(
        "delta_kept_due_to_novelty_cue: {}",
        gate_diagnostics.delta_kept_due_to_novelty_cue
    );
    println!(
        "interest_overlap_without_novelty: {}",
        gate_diagnostics.interest_overlap_without_novelty
    );
    println!(
        "familiar_claim_overlap_count: {}",
        gate_diagnostics.familiar_claim_overlap_count
    );

    let mismatches = labeled
        .iter()
        .filter(|(label, prediction)| {
            label.expected_priority != prediction.priority
                || label.expected_directive != prediction.directive
        })
        .collect::<Vec<_>>();
    println!("Mismatches: {}", mismatches.len());
    for (label, prediction) in mismatches {
        println!(
            "- {} expected={}/{} actual={}/{} reason={}",
            label.sentence_id,
            priority_name(label.expected_priority),
            directive_name(label.expected_directive),
            priority_name(prediction.priority),
            directive_name(prediction.directive),
            label.reason
        );
    }
    metrics
}

fn build_error_report(
    fixture_dir: &Path,
    metadata: &FixtureMetadata,
    candidates: &[CanonicalReadingUnit],
    predictions: &HashMap<String, FixturePrediction>,
    gold: &[GoldLabel],
    metrics: &EvaluationMetrics,
) -> ProductErrorReport {
    let candidates_by_id = candidates
        .iter()
        .map(|candidate| (candidate.sentence_id.as_str(), candidate))
        .collect::<HashMap<_, _>>();
    let details = gold
        .iter()
        .filter_map(|label| {
            Some(make_error_detail(
                candidates_by_id.get(label.sentence_id.as_str())?,
                predictions.get(&label.sentence_id)?,
                label,
            ))
        })
        .collect::<Vec<_>>();
    let select = |predicate: fn(&ErrorDetail) -> bool| {
        details
            .iter()
            .filter(|detail| predicate(detail))
            .cloned()
            .collect::<Vec<_>>()
    };
    ProductErrorReport {
        fixture: fixture_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown")
            .to_owned(),
        fixture_type: metadata.fixture_type.clone(),
        baseline_name: metadata.baseline_name.clone(),
        metrics: metrics.clone(),
        false_positive_deltas: select(|detail| {
            detail.predicted_priority == "delta" && detail.expected_priority != "delta"
        }),
        false_negative_deltas: select(|detail| {
            detail.expected_priority == "delta" && detail.predicted_priority != "delta"
        }),
        familiar_highlight_errors: select(|detail| {
            detail.expected_priority == "familiar" && detail.predicted_directive == "highlight"
        }),
        bridge_confusions: select(|detail| {
            (detail.expected_priority == "bridge" || detail.predicted_priority == "bridge")
                && detail.expected_priority != detail.predicted_priority
        }),
        over_annotation_errors: select(|detail| {
            detail.expected_directive == "leave_normal"
                && detail.predicted_directive != "leave_normal"
        }),
        delta_gate_affected: select(|detail| {
            detail.gate_decision.demoted_by_familiar_claim
                || detail.gate_decision.kept_due_to_novelty_cue
        }),
    }
}

fn make_error_detail(
    candidate: &CanonicalReadingUnit,
    prediction: &FixturePrediction,
    label: &GoldLabel,
) -> ErrorDetail {
    let likely_cause = likely_error_cause(label, prediction);
    ErrorDetail {
        sentence_id: candidate.sentence_id.clone(),
        page: candidate.page,
        candidate_kind: candidate_kind_name(candidate.candidate_kind).to_owned(),
        sentence_text: candidate.normalized_text.clone(),
        predicted_priority: priority_name(prediction.priority).to_owned(),
        predicted_directive: directive_name(prediction.directive).to_owned(),
        expected_priority: priority_name(label.expected_priority).to_owned(),
        expected_directive: directive_name(label.expected_directive).to_owned(),
        confidence: prediction.confidence,
        gold_reason: label.reason.clone(),
        likely_cause: likely_cause.to_owned(),
        baseline_overlap: prediction.signals.clone(),
        gate_decision: prediction.gate_decision.clone(),
    }
}

fn likely_error_cause(label: &GoldLabel, prediction: &FixturePrediction) -> &'static str {
    if label.reason.to_lowercase().contains("ambiguous") {
        "ambiguous_gold_label"
    } else if !prediction.signals.matched_familiar_claims.is_empty() {
        "familiar_claim_overlap"
    } else if !prediction.signals.matched_known_concepts.is_empty() {
        "known_concept_overlap"
    } else if matches!(label.expected_priority, Priority::Bridge)
        && matches!(prediction.priority, Priority::Delta)
    {
        "bridge_misclassified_as_delta"
    } else if matches!(label.expected_directive, ReaderDirective::LeaveNormal) {
        "low_value_technical_sentence"
    } else {
        "unknown"
    }
}

fn print_error_analysis(report: &ProductErrorReport) {
    println!("\nDetailed error analysis:");
    print_error_section("false_positive_deltas", &report.false_positive_deltas);
    print_error_section("false_negative_deltas", &report.false_negative_deltas);
    print_error_section(
        "familiar_highlight_errors",
        &report.familiar_highlight_errors,
    );
    print_error_section("bridge_confusions", &report.bridge_confusions);
    print_error_section("over_annotation_errors", &report.over_annotation_errors);
    print_error_section("delta_gate_affected", &report.delta_gate_affected);
}

fn print_error_section(name: &str, errors: &[ErrorDetail]) {
    println!("{name} ({}):", errors.len());
    for error in errors {
        println!(
            "- sentence_id={} page={} candidate_kind={} confidence={} cause={}",
            error.sentence_id,
            error.page,
            error.candidate_kind,
            error
                .confidence
                .map(|value| format!("{value:.3}"))
                .unwrap_or_else(|| "n/a".to_owned()),
            error.likely_cause
        );
        println!("  text: {}", error.sentence_text);
        println!(
            "  predicted: {}/{}; expected: {}/{}",
            error.predicted_priority,
            error.predicted_directive,
            error.expected_priority,
            error.expected_directive
        );
        println!("  gold_reason: {}", error.gold_reason);
        println!(
            "  overlaps: known={:?} familiar={:?} interests={:?}",
            error.baseline_overlap.matched_known_concepts,
            error.baseline_overlap.matched_familiar_claims,
            error.baseline_overlap.matched_interests
        );
        println!(
            "  gate: eligible={} demoted_by_familiar_claim={} kept_due_to_novelty_cue={} interest_overlap_without_novelty={} novelty_cues={:?} familiar_cues={:?}",
            error.gate_decision.eligible,
            error.gate_decision.demoted_by_familiar_claim,
            error.gate_decision.kept_due_to_novelty_cue,
            error.gate_decision.interest_overlap_without_novelty,
            error.gate_decision.novelty_cues,
            error.gate_decision.familiar_cues,
        );
    }
}

fn write_error_reports(report: &ProductErrorReport) -> anyhow::Result<()> {
    let output_dir = Path::new("artifacts/product-eval").join(&report.fixture);
    fs::create_dir_all(&output_dir)
        .with_context(|| format!("failed to create {}", output_dir.display()))?;
    let json_path = output_dir.join("error_report.json");
    let markdown_path = output_dir.join("error_report.md");
    fs::write(&json_path, serde_json::to_vec_pretty(report)?)
        .with_context(|| format!("failed to write {}", json_path.display()))?;
    fs::write(&markdown_path, markdown_report(report))
        .with_context(|| format!("failed to write {}", markdown_path.display()))?;
    println!("error_report_json: {}", json_path.display());
    println!("error_report_markdown: {}", markdown_path.display());
    Ok(())
}

fn markdown_report(report: &ProductErrorReport) -> String {
    let mut output = format!(
        "# Product-fit error report: {}\n\n- Fixture type: `{}`\n- Baseline: `{}`\n\n",
        report.fixture, report.fixture_type, report.baseline_name
    );
    output.push_str("## False positive deltas by likely cause\n\n");
    for cause in [
        "known_concept_overlap",
        "familiar_claim_overlap",
        "bridge_misclassified_as_delta",
        "low_value_technical_sentence",
        "ambiguous_gold_label",
        "unknown",
    ] {
        let errors = report
            .false_positive_deltas
            .iter()
            .filter(|error| error.likely_cause == cause)
            .cloned()
            .collect::<Vec<_>>();
        markdown_error_section(&mut output, &format!("### {cause}"), &errors);
    }
    markdown_error_section(
        &mut output,
        "## False negative deltas",
        &report.false_negative_deltas,
    );
    markdown_error_section(
        &mut output,
        "## Familiar highlight errors",
        &report.familiar_highlight_errors,
    );
    markdown_error_section(
        &mut output,
        "## Bridge confusions",
        &report.bridge_confusions,
    );
    markdown_error_section(
        &mut output,
        "## Over-annotation errors",
        &report.over_annotation_errors,
    );
    markdown_error_section(
        &mut output,
        "## Delta gate affected sentences",
        &report.delta_gate_affected,
    );
    output
}

fn markdown_error_section(output: &mut String, heading: &str, errors: &[ErrorDetail]) {
    output.push_str(heading);
    output.push_str("\n\n");
    if errors.is_empty() {
        output.push_str("None.\n\n");
        return;
    }
    for error in errors {
        output.push_str(&format!(
            "- **{}** (page {}, `{}`; confidence {})\n  - Text: {}\n  - Predicted: `{}/{}`; expected: `{}/{}`\n  - Gold reason: {}\n  - Baseline overlap: known={:?}; familiar={:?}; interests={:?}\n  - Gate: eligible={}; demoted_by_familiar_claim={}; kept_due_to_novelty_cue={}; interest_overlap_without_novelty={}; novelty_cues={:?}; familiar_cues={:?}\n\n",
            error.sentence_id,
            error.page,
            error.candidate_kind,
            error
                .confidence
                .map(|value| format!("{value:.3}"))
                .unwrap_or_else(|| "n/a".to_owned()),
            error.sentence_text,
            error.predicted_priority,
            error.predicted_directive,
            error.expected_priority,
            error.expected_directive,
            error.gold_reason,
            error.baseline_overlap.matched_known_concepts,
            error.baseline_overlap.matched_familiar_claims,
            error.baseline_overlap.matched_interests,
            error.gate_decision.eligible,
            error.gate_decision.demoted_by_familiar_claim,
            error.gate_decision.kept_due_to_novelty_cue,
            error.gate_decision.interest_overlap_without_novelty,
            error.gate_decision.novelty_cues,
            error.gate_decision.familiar_cues,
        ));
    }
}

fn enforce_thresholds(
    thresholds: &FixtureThresholds,
    metrics: &EvaluationMetrics,
) -> anyhow::Result<()> {
    let mut failures = Vec::new();
    check_min_ratio(
        &mut failures,
        "delta_precision",
        &metrics.delta_precision,
        thresholds.delta_precision_min,
    );
    check_min_ratio(
        &mut failures,
        "delta_recall",
        &metrics.delta_recall,
        thresholds.delta_recall_min,
    );
    check_min(
        &mut failures,
        "familiar_suppression_rate",
        metrics.familiar_suppression_rate,
        thresholds.familiar_suppression_rate_min,
    );
    check_max(
        &mut failures,
        "known_as_delta_rate",
        metrics.known_as_delta_rate,
        thresholds.known_as_delta_rate_max,
    );
    check_max(
        &mut failures,
        "visible_guidance_per_page",
        metrics.visible_guidance_per_page,
        thresholds.visible_guidance_per_page_max,
    );
    if let Some(maximum) = thresholds.false_negative_delta_count_max {
        if metrics.false_negative_delta_count > maximum {
            failures.push(format!(
                "false_negative_delta_count={} exceeds maximum {}",
                metrics.false_negative_delta_count, maximum
            ));
        }
    }
    if failures.is_empty() {
        println!("Thresholds: PASS");
        Ok(())
    } else {
        bail!("threshold failures: {}", failures.join(", "))
    }
}

fn check_min_ratio(
    failures: &mut Vec<String>,
    name: &str,
    metric: &RatioMetric,
    threshold: Option<f64>,
) {
    let Some(minimum) = threshold else {
        return;
    };
    match metric.value {
        Some(actual) if actual < minimum => {
            failures.push(format!("{name}={actual:.3} below {minimum:.3}"));
        }
        None => failures.push(format!(
            "{name}=N/A because denominator is zero, but minimum {minimum:.3} is required"
        )),
        Some(_) => {}
    }
}

fn check_min(failures: &mut Vec<String>, name: &str, actual: f64, threshold: Option<f64>) {
    if let Some(minimum) = threshold {
        if actual < minimum {
            failures.push(format!("{name}={actual:.3} below {minimum:.3}"));
        }
    }
}

fn check_max(failures: &mut Vec<String>, name: &str, actual: f64, threshold: Option<f64>) {
    if let Some(maximum) = threshold {
        if actual > maximum {
            failures.push(format!("{name}={actual:.3} above {maximum:.3}"));
        }
    }
}

impl RatioMetric {
    fn new(numerator: usize, denominator: usize) -> Self {
        Self {
            value: (denominator != 0).then(|| numerator as f64 / denominator as f64),
            numerator,
            denominator,
        }
    }

    fn display(&self) -> String {
        self.value
            .map(|value| format!("{value:.3}"))
            .unwrap_or_else(|| "N/A".to_owned())
    }
}

fn ratio_or_zero(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn priority_name(priority: Priority) -> &'static str {
    match priority {
        Priority::Delta => "delta",
        Priority::Bridge => "bridge",
        Priority::Familiar => "familiar",
    }
}

fn directive_name(directive: ReaderDirective) -> &'static str {
    match directive {
        ReaderDirective::Highlight => "highlight",
        ReaderDirective::SoftFade => "soft_fade",
        ReaderDirective::Callout => "callout",
        ReaderDirective::LeaveNormal => "leave_normal",
    }
}

fn candidate_kind_name(kind: CandidateKind) -> &'static str {
    match kind {
        CandidateKind::BodySentence => "body_sentence",
        CandidateKind::Heading => "heading",
        CandidateKind::Metadata => "metadata",
        CandidateKind::Reference => "reference",
        CandidateKind::FormulaOrTable => "formula_or_table",
        CandidateKind::ShortFragment => "short_fragment",
        CandidateKind::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn baseline() -> ProductBaseline {
        ProductBaseline {
            reader_profile: "Experienced retrieval engineer".to_owned(),
            known_concepts: vec!["indexing workflows".to_owned()],
            familiar_claims: vec!["vector databases store embeddings".to_owned()],
            interests: vec!["adaptive freshness budgets".to_owned()],
            explicit_not_interested_topics: vec!["vendor pricing".to_owned()],
        }
    }

    #[test]
    fn deterministic_fixture_analysis_separates_product_roles() {
        let baseline = baseline();
        assert_eq!(
            predict_candidate(
                "Vector databases store embeddings for retrieval.",
                &baseline
            )
            .priority,
            Priority::Familiar
        );
        assert_eq!(
            predict_candidate(
                "Indexing workflows now use adaptive freshness budgets for updates.",
                &baseline
            )
            .priority,
            Priority::Bridge
        );
        assert_eq!(
            predict_candidate(
                "Adaptive freshness budgets reduce stale answers by 31 percent.",
                &baseline
            )
            .priority,
            Priority::Delta
        );
        assert_eq!(
            predict_candidate("Vendor pricing lists subscription tiers.", &baseline).directive,
            ReaderDirective::LeaveNormal
        );
    }

    #[test]
    fn ratio_metrics_are_na_for_empty_denominators() {
        let unavailable = RatioMetric::new(0, 0);
        assert_eq!(unavailable.value, None);
        assert_eq!(unavailable.display(), "N/A");
        assert_eq!(
            serde_json::to_value(&unavailable).unwrap()["value"],
            serde_json::Value::Null
        );
        assert_eq!(unavailable.denominator, 0);

        let available = RatioMetric::new(1, 2);
        assert_eq!(available.value, Some(0.5));
        assert_eq!(available.display(), "0.500");
    }

    #[test]
    fn false_negative_count_threshold_is_enforced() {
        let metrics = EvaluationMetrics {
            delta_precision: RatioMetric::new(1, 1),
            delta_recall: RatioMetric::new(1, 1),
            familiar_suppression_rate: 1.0,
            known_as_delta_rate: 0.0,
            bridge_precision: RatioMetric::new(1, 1),
            visible_guidance_per_page: 0.0,
            highlight_per_page: 0.0,
            callout_per_page: 0.0,
            soft_fade_per_page: 0.0,
            body_sentence_count: 1,
            gold_coverage_ratio: 1.0,
            false_positive_delta_count: 0,
            false_negative_delta_count: 1,
            familiar_highlight_count: 0,
            over_annotation_score: 0.0,
        };
        let thresholds = FixtureThresholds {
            false_negative_delta_count_max: Some(0),
            ..FixtureThresholds::default()
        };
        assert!(enforce_thresholds(&thresholds, &metrics).is_err());
    }

    #[test]
    fn na_metric_is_ignored_without_threshold_and_rejected_when_required() {
        let metric = RatioMetric::new(0, 0);
        let mut failures = Vec::new();
        check_min_ratio(&mut failures, "bridge_precision", &metric, None);
        assert!(failures.is_empty());
        check_min_ratio(&mut failures, "bridge_precision", &metric, Some(0.5));
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("denominator is zero"));
    }

    #[test]
    fn familiarity_gate_demotes_adversarial_false_delta() {
        let baseline = ProductBaseline {
            reader_profile: "Experienced retrieval lead".to_owned(),
            known_concepts: Vec::new(),
            familiar_claims: vec!["previously validated configuration".to_owned()],
            interests: vec!["adaptive freshness budget".to_owned()],
            explicit_not_interested_topics: Vec::new(),
        };
        let prediction = predict_candidate(
            "An adaptive freshness budget was in the previously validated configuration.",
            &baseline,
        );
        let gold = GoldLabel {
            sentence_id: "cp1s4".to_owned(),
            expected_priority: Priority::Familiar,
            expected_directive: ReaderDirective::LeaveNormal,
            reason: "Already known.".to_owned(),
        };

        assert_eq!(prediction.priority, Priority::Familiar);
        assert_eq!(prediction.directive, ReaderDirective::LeaveNormal);
        assert!(prediction.gate_decision.demoted_by_familiar_claim);
        assert!(prediction.gate_decision.interest_overlap_without_novelty);
        assert_eq!(
            prediction.signals.matched_familiar_claims,
            vec!["previously validated configuration"]
        );
        assert_eq!(
            prediction.signals.matched_interests,
            vec!["adaptive freshness budget"]
        );
        assert_eq!(
            likely_error_cause(&gold, &prediction),
            "familiar_claim_overlap"
        );
    }

    #[test]
    fn markdown_report_contains_all_error_groups() {
        let report = ProductErrorReport {
            fixture: "test".to_owned(),
            fixture_type: "adversarial".to_owned(),
            baseline_name: "test_baseline".to_owned(),
            metrics: EvaluationMetrics {
                delta_precision: RatioMetric::new(1, 1),
                delta_recall: RatioMetric::new(1, 1),
                familiar_suppression_rate: 1.0,
                known_as_delta_rate: 0.0,
                bridge_precision: RatioMetric::new(0, 0),
                visible_guidance_per_page: 0.0,
                highlight_per_page: 0.0,
                callout_per_page: 0.0,
                soft_fade_per_page: 0.0,
                body_sentence_count: 1,
                gold_coverage_ratio: 1.0,
                false_positive_delta_count: 0,
                false_negative_delta_count: 0,
                familiar_highlight_count: 0,
                over_annotation_score: 0.0,
            },
            false_positive_deltas: Vec::new(),
            false_negative_deltas: Vec::new(),
            familiar_highlight_errors: Vec::new(),
            bridge_confusions: Vec::new(),
            over_annotation_errors: Vec::new(),
            delta_gate_affected: Vec::new(),
        };
        let markdown = markdown_report(&report);
        assert!(markdown.contains("### familiar_claim_overlap"));
        assert!(markdown.contains("## False negative deltas"));
        assert!(markdown.contains("## Familiar highlight errors"));
        assert!(markdown.contains("## Bridge confusions"));
        assert!(markdown.contains("## Over-annotation errors"));
        assert!(markdown.contains("## Delta gate affected sentences"));
    }
}
