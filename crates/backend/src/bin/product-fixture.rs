#[path = "../canonical.rs"]
mod canonical;
#[allow(dead_code)]
#[path = "../delta_policy.rs"]
mod delta_policy;
#[allow(dead_code)]
#[path = "../models.rs"]
mod models;

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context};
use models::{CandidateKind, CanonicalReadingUnit};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

const PRODUCT_FIXTURE_ROOT: &str = "resource/product-fixtures";

#[derive(Debug, Deserialize)]
struct Baseline {
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

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CandidateExport {
    sentence_id: String,
    page: usize,
    candidate_kind: CandidateKind,
    text: String,
    normalized_text: String,
    length: usize,
    item_ranges_present: bool,
    tags: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct CandidateFile {
    schema_version: String,
    fixture: String,
    baseline_source: String,
    candidates: Vec<CandidateExport>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ManualGoldLabel {
    sentence_id: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    expected_priority: String,
    #[serde(default)]
    expected_directive: String,
    #[serde(default)]
    reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    label_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    difficulty: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    annotator: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    review_status: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    allow_non_body: bool,
}

#[derive(Debug, Deserialize, Serialize)]
struct GoldLabelFile {
    labels: Vec<ManualGoldLabel>,
}

#[derive(Debug)]
struct ValidationSummary {
    candidates: CandidateFile,
    gold: GoldLabelFile,
    issues: Vec<String>,
}

fn main() -> anyhow::Result<()> {
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_default();
    match command.as_str() {
        "init" => initialize_fixture(required_arg(&mut args, "fixture")?, required_arg(&mut args, "PDF")?),
        "export" => export_candidates(required_arg(&mut args, "fixture")?),
        "validate" => validate_command(required_arg(&mut args, "fixture")?),
        "review" => review_labels(required_arg(&mut args, "fixture")?),
        _ => bail!("usage: product-fixture <init FIXTURE PDF|export FIXTURE|validate FIXTURE|review FIXTURE>"),
    }
}

fn required_arg(args: &mut impl Iterator<Item = String>, name: &str) -> anyhow::Result<String> {
    args.next()
        .with_context(|| format!("missing {name} argument"))
}

fn fixture_dir(name: &str) -> anyhow::Result<PathBuf> {
    if name.is_empty()
        || name.starts_with('.')
        || !name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        bail!("fixture name must contain only letters, numbers, '-' or '_'");
    }
    Ok(Path::new(PRODUCT_FIXTURE_ROOT).join(name))
}

fn initialize_fixture(name: String, pdf: String) -> anyhow::Result<()> {
    let fixture = fixture_dir(&name)?;
    let source_pdf = Path::new(&pdf);
    if !source_pdf.is_file() {
        bail!("PDF does not exist: {}", source_pdf.display());
    }
    fs::create_dir_all(&fixture)
        .with_context(|| format!("failed to create {}", fixture.display()))?;
    fs::copy(source_pdf, fixture.join("document.pdf"))
        .with_context(|| format!("failed to copy {}", source_pdf.display()))?;

    write_new(
        &fixture.join("baseline.template.json"),
        serde_json::to_vec_pretty(&json!({
            "reader_profile": "Describe the specific reader whose knowledge delta is being evaluated.",
            "known_concepts": [],
            "familiar_claims": [],
            "interests": [],
            "explicit_not_interested_topics": []
        }))?,
    )?;
    write_new(
        &fixture.join("fixture.json"),
        serde_json::to_vec_pretty(&json!({
            "fixture_name": name,
            "fixture_type": "realistic",
            "domain": "TODO",
            "baseline_name": format!("{}_baseline", name.replace('-', "_")),
            "source": "real_pdf",
            "notes": "Complete baseline.json and human gold labels before using this fixture as a quality gate.",
            "thresholds": {}
        }))?,
    )?;
    write_new(
        &fixture.join("README.md"),
        format!(
            "# {name}\n\nReal-PDF product-fit annotation fixture.\n\n1. Copy `baseline.template.json` to `baseline.json` and describe a specific reader.\n2. Run `make export-product-candidates FIXTURE={name}`.\n3. Copy `gold_labels.template.json` to `gold_labels.json` and label every intended candidate.\n4. Validate and review the labels before evaluation.\n"
        )
        .into_bytes(),
    )?;
    println!("Initialized product fixture: {}", fixture.display());
    Ok(())
}

fn write_new(path: &Path, bytes: Vec<u8>) -> anyhow::Result<()> {
    if path.exists() {
        println!("Preserved existing file: {}", path.display());
        return Ok(());
    }
    fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))
}

fn export_candidates(name: String) -> anyhow::Result<()> {
    let fixture = fixture_dir(&name)?;
    let baseline_path = baseline_path(&fixture)?;
    let baseline: Baseline = read_json(&baseline_path)?;
    validate_baseline(&baseline)?;
    let pdf = fs::read(fixture.join("document.pdf"))
        .with_context(|| format!("missing document.pdf in {}", fixture.display()))?;
    let model = canonical::extract_canonical_text_model(&pdf, Uuid::nil())?;
    let units = canonical::generate_canonical_reading_units(&model)?;
    let candidates = units.iter().map(export_candidate).collect::<Vec<_>>();
    let candidate_file = CandidateFile {
        schema_version: "1.0".to_owned(),
        fixture: name.clone(),
        baseline_source: baseline_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("baseline.json")
            .to_owned(),
        candidates,
    };
    fs::write(
        fixture.join("candidates.json"),
        serde_json::to_vec_pretty(&candidate_file)?,
    )?;
    fs::write(
        fixture.join("candidates.md"),
        candidates_markdown(&candidate_file),
    )?;
    let template = GoldLabelFile {
        labels: candidate_file
            .candidates
            .iter()
            .filter(|candidate| matches!(candidate.candidate_kind, CandidateKind::BodySentence))
            .map(|candidate| ManualGoldLabel {
                sentence_id: candidate.sentence_id.clone(),
                text: candidate.text.clone(),
                expected_priority: String::new(),
                expected_directive: String::new(),
                reason: String::new(),
                label_type: Some(String::new()),
                difficulty: Some(String::new()),
                tags: Vec::new(),
                annotator: Some(String::new()),
                review_status: Some(String::new()),
                allow_non_body: false,
            })
            .collect(),
    };
    fs::write(
        fixture.join("gold_labels.template.json"),
        serde_json::to_vec_pretty(&template)?,
    )?;
    println!(
        "Exported {} candidates across {} pages",
        candidate_file.candidates.len(),
        model.pages.len()
    );
    println!("candidates_json: {}/candidates.json", fixture.display());
    println!("candidates_markdown: {}/candidates.md", fixture.display());
    println!(
        "gold_template: {}/gold_labels.template.json",
        fixture.display()
    );
    Ok(())
}

fn baseline_path(fixture: &Path) -> anyhow::Result<PathBuf> {
    for filename in ["baseline.json", "baseline.template.json"] {
        let path = fixture.join(filename);
        if path.is_file() {
            return Ok(path);
        }
    }
    bail!("fixture requires baseline.json or baseline.template.json")
}

fn validate_baseline(baseline: &Baseline) -> anyhow::Result<()> {
    if baseline.reader_profile.trim().is_empty() {
        bail!("reader_profile must not be empty");
    }
    let _signal_count = baseline.known_concepts.len()
        + baseline.familiar_claims.len()
        + baseline.interests.len()
        + baseline.explicit_not_interested_topics.len();
    Ok(())
}

fn export_candidate(unit: &CanonicalReadingUnit) -> CandidateExport {
    let mut tags = Vec::new();
    if unit.normalized_text.chars().count() < 30 {
        tags.push("very_short".to_owned());
    }
    if unit
        .normalized_text
        .chars()
        .any(|character| character.is_ascii_digit())
    {
        tags.push("contains_numeric_result".to_owned());
    }
    if unit.item_ranges.len() > 1 {
        tags.push("multi_item_range".to_owned());
    }
    CandidateExport {
        sentence_id: unit.sentence_id.clone(),
        page: unit.page,
        candidate_kind: unit.candidate_kind,
        text: unit.text.clone(),
        normalized_text: unit.normalized_text.clone(),
        length: unit.normalized_text.chars().count(),
        item_ranges_present: !unit.item_ranges.is_empty(),
        tags,
    }
}

fn candidates_markdown(file: &CandidateFile) -> String {
    let mut output = format!("# Candidate annotation sheet: {}\n\n", file.fixture);
    let mut pages = BTreeMap::<usize, Vec<&CandidateExport>>::new();
    for candidate in &file.candidates {
        pages.entry(candidate.page).or_default().push(candidate);
    }
    for (page, candidates) in pages {
        output.push_str(&format!("## Page {page}\n\n"));
        for candidate in candidates {
            output.push_str(&format!(
                "### `{}`\n\n- Page: {}\n- Kind: `{:?}`\n- Length: {}\n- Text: {}\n\n- [ ] delta / highlight\n- [ ] bridge / callout\n- [ ] familiar / soft_fade\n- [ ] familiar / leave_normal\n- [ ] skip\n\n",
                candidate.sentence_id,
                candidate.page,
                candidate.candidate_kind,
                candidate.length,
                candidate.text
            ));
        }
    }
    output
}

fn validate_command(name: String) -> anyhow::Result<()> {
    let summary = load_and_validate(&name)?;
    if !summary.issues.is_empty() {
        for issue in &summary.issues {
            eprintln!("- {issue}");
        }
        bail!(
            "gold label validation failed with {} issue(s)",
            summary.issues.len()
        );
    }
    println!("Gold labels valid: {} labels", summary.gold.labels.len());
    Ok(())
}

fn load_and_validate(name: &str) -> anyhow::Result<ValidationSummary> {
    let fixture = fixture_dir(name)?;
    let candidates: CandidateFile = read_json(&fixture.join("candidates.json"))?;
    let gold_path = fixture.join("gold_labels.json");
    if !gold_path.is_file() {
        bail!("gold_labels.json does not exist: {}", gold_path.display());
    }
    let gold: GoldLabelFile = read_json(&gold_path)?;
    let candidate_by_id = candidates
        .candidates
        .iter()
        .map(|candidate| (candidate.sentence_id.as_str(), candidate))
        .collect::<HashMap<_, _>>();
    let mut seen = HashSet::new();
    let mut issues = Vec::new();
    for label in &gold.labels {
        if !seen.insert(label.sentence_id.as_str()) {
            issues.push(format!("duplicate sentence_id {}", label.sentence_id));
        }
        let Some(candidate) = candidate_by_id.get(label.sentence_id.as_str()) else {
            issues.push(format!("unknown sentence_id {}", label.sentence_id));
            continue;
        };
        if !matches!(
            label.expected_priority.as_str(),
            "delta" | "bridge" | "familiar"
        ) {
            issues.push(format!(
                "{} has invalid expected_priority",
                label.sentence_id
            ));
        }
        if !matches!(
            label.expected_directive.as_str(),
            "highlight" | "callout" | "soft_fade" | "leave_normal"
        ) {
            issues.push(format!(
                "{} has invalid expected_directive",
                label.sentence_id
            ));
        }
        if label.reason.trim().is_empty() {
            issues.push(format!("{} has an empty reason", label.sentence_id));
        }
        validate_optional(
            &mut issues,
            &label.sentence_id,
            "label_type",
            label.label_type.as_deref(),
            &["standard", "trap", "control"],
        );
        validate_optional(
            &mut issues,
            &label.sentence_id,
            "difficulty",
            label.difficulty.as_deref(),
            &["easy", "medium", "hard", "adversarial"],
        );
        validate_optional(
            &mut issues,
            &label.sentence_id,
            "review_status",
            label.review_status.as_deref(),
            &["draft", "reviewed", "approved", "rejected"],
        );
        if !matches!(candidate.candidate_kind, CandidateKind::BodySentence) && !label.allow_non_body
        {
            issues.push(format!(
                "{} targets {:?}; set allow_non_body=true only when intentional",
                label.sentence_id, candidate.candidate_kind
            ));
        }
    }
    Ok(ValidationSummary {
        candidates,
        gold,
        issues,
    })
}

fn validate_optional(
    issues: &mut Vec<String>,
    sentence_id: &str,
    field: &str,
    value: Option<&str>,
    allowed: &[&str],
) {
    if let Some(value) = value {
        if !value.is_empty() && !allowed.contains(&value) {
            issues.push(format!("{sentence_id} has invalid {field} '{value}'"));
        }
    }
}

fn review_labels(name: String) -> anyhow::Result<()> {
    let summary = load_and_validate(&name)?;
    let report = label_review_markdown(&name, &summary);
    let output_dir = Path::new("artifacts/product-eval").join(&name);
    fs::create_dir_all(&output_dir)?;
    let output = output_dir.join("label_review.md");
    fs::write(&output, report)?;
    println!("Label review: {}", output.display());
    println!("Validation issues: {}", summary.issues.len());
    Ok(())
}

fn label_review_markdown(name: &str, summary: &ValidationSummary) -> String {
    let candidates = &summary.candidates.candidates;
    let labels = &summary.gold.labels;
    let by_id = candidates
        .iter()
        .map(|candidate| (candidate.sentence_id.as_str(), candidate))
        .collect::<HashMap<_, _>>();
    let body_count = candidates
        .iter()
        .filter(|candidate| matches!(candidate.candidate_kind, CandidateKind::BodySentence))
        .count();
    let labeled_body = labels
        .iter()
        .filter(|label| {
            by_id
                .get(label.sentence_id.as_str())
                .is_some_and(|candidate| {
                    matches!(candidate.candidate_kind, CandidateKind::BodySentence)
                })
        })
        .count();
    let distribution = count_values(labels.iter().map(|label| label.expected_priority.as_str()));
    let directives = count_values(labels.iter().map(|label| label.expected_directive.as_str()));
    let pages = count_values(
        labels
            .iter()
            .filter_map(|label| {
                by_id
                    .get(label.sentence_id.as_str())
                    .map(|candidate| candidate.page.to_string())
            })
            .collect::<Vec<_>>()
            .iter()
            .map(String::as_str),
    );
    let trap_count = labels
        .iter()
        .filter(|label| {
            label.label_type.as_deref() == Some("trap")
                || label.tags.iter().any(|tag| tag == "trap")
        })
        .count();
    let draft_count = labels
        .iter()
        .filter(|label| {
            matches!(
                label.review_status.as_deref(),
                None | Some("") | Some("draft")
            )
        })
        .count();
    let reviewed_count = labels.len().saturating_sub(draft_count);
    let mut suspicious = summary.issues.clone();
    let mut highlights_per_page = BTreeMap::<usize, usize>::new();
    for label in labels {
        if label.expected_priority == "delta" && label.reason.trim().is_empty() {
            suspicious.push(format!(
                "{} is a delta with an empty reason",
                label.sentence_id
            ));
        }
        if let Some(candidate) = by_id.get(label.sentence_id.as_str()) {
            if label.expected_priority == "delta" && candidate.length < 30 {
                suspicious.push(format!(
                    "{} is a very short delta ({} chars)",
                    label.sentence_id, candidate.length
                ));
            }
            if label.expected_priority == "delta"
                && matches!(
                    candidate.candidate_kind,
                    CandidateKind::Metadata
                        | CandidateKind::Reference
                        | CandidateKind::FormulaOrTable
                )
            {
                suspicious.push(format!(
                    "{} labels {:?} as delta",
                    label.sentence_id, candidate.candidate_kind
                ));
            }
            if label.expected_directive == "highlight" {
                *highlights_per_page.entry(candidate.page).or_default() += 1;
            }
        }
    }
    for (page, count) in highlights_per_page {
        if count > 5 {
            suspicious.push(format!("page {page} has {count} highlights"));
        }
    }
    let labeled_ids = labels
        .iter()
        .map(|label| label.sentence_id.as_str())
        .collect::<HashSet<_>>();
    for candidate in candidates
        .iter()
        .filter(|candidate| matches!(candidate.candidate_kind, CandidateKind::BodySentence))
    {
        if !labeled_ids.contains(candidate.sentence_id.as_str()) {
            suspicious.push(format!("missing body candidate {}", candidate.sentence_id));
        }
    }
    suspicious.sort();
    suspicious.dedup();
    format!(
        "# Product label review: {name}\n\n- Candidate count: {}\n- Body sentence count: {body_count}\n- Labeled count: {}\n- Gold coverage ratio: {:.3}\n- Label distribution: {}\n- Directive distribution: {}\n- Labels per page: {}\n- Delta / bridge / familiar: {} / {} / {}\n- Trap labels: {trap_count}\n- Draft / reviewed: {draft_count} / {reviewed_count}\n\n## Suspicious labels ({})\n\n{}",
        candidates.len(),
        labels.len(),
        ratio(labeled_body, body_count),
        format_counts(&distribution),
        format_counts(&directives),
        format_counts(&pages),
        distribution.get("delta").copied().unwrap_or(0),
        distribution.get("bridge").copied().unwrap_or(0),
        distribution.get("familiar").copied().unwrap_or(0),
        suspicious.len(),
        if suspicious.is_empty() { "None.\n".to_owned() } else { suspicious.into_iter().map(|issue| format!("- {issue}\n")).collect() }
    )
}

fn count_values<'a>(values: impl Iterator<Item = &'a str>) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for value in values.filter(|value| !value.is_empty()) {
        *counts.entry(value.to_owned()).or_default() += 1;
    }
    counts
}

fn format_counts(counts: &BTreeMap<String, usize>) -> String {
    counts
        .iter()
        .map(|(key, count)| format!("{key}={count}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> anyhow::Result<T> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("invalid JSON in {}", path.display()))
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_contains_required_manual_annotation_fields() {
        let template = ManualGoldLabel {
            sentence_id: "cp1s1".to_owned(),
            text: "Sentence.".to_owned(),
            expected_priority: String::new(),
            expected_directive: String::new(),
            reason: String::new(),
            label_type: None,
            difficulty: None,
            tags: Vec::new(),
            annotator: None,
            review_status: Some("draft".to_owned()),
            allow_non_body: false,
        };
        let value = serde_json::to_value(template).unwrap();
        assert_eq!(value["sentence_id"], "cp1s1");
        assert!(value.get("expected_priority").is_some());
        assert!(value.get("tags").is_some());
        assert_eq!(value["review_status"], "draft");
    }

    #[test]
    fn fixture_names_cannot_escape_product_root() {
        assert!(fixture_dir("real-paper").is_ok());
        assert!(fixture_dir("../private").is_err());
        assert!(fixture_dir("with/slash").is_err());
    }
}
