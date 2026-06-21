use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default)]
pub struct BaselineEvidence<'a> {
    pub known_concepts: &'a [String],
    pub familiar_claims: &'a [String],
    pub interests: &'a [String],
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DeltaEligibilityDecision {
    pub checked: bool,
    pub eligible: bool,
    pub demoted_by_familiar_claim: bool,
    pub kept_due_to_novelty_cue: bool,
    pub interest_overlap_without_novelty: bool,
    pub familiar_claim_overlap: bool,
    pub matched_known_concepts: Vec<String>,
    pub matched_familiar_claims: Vec<String>,
    pub matched_interests: Vec<String>,
    pub novelty_cues: Vec<String>,
    pub familiar_cues: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DeltaEligibilityDiagnostics {
    pub delta_eligibility_checked: usize,
    pub delta_demoted_by_familiar_claim: usize,
    pub delta_kept_due_to_novelty_cue: usize,
    pub interest_overlap_without_novelty: usize,
    pub familiar_claim_overlap_count: usize,
}

impl DeltaEligibilityDiagnostics {
    pub fn record(&mut self, decision: &DeltaEligibilityDecision) {
        self.delta_eligibility_checked += usize::from(decision.checked);
        self.delta_demoted_by_familiar_claim += usize::from(decision.demoted_by_familiar_claim);
        self.delta_kept_due_to_novelty_cue += usize::from(decision.kept_due_to_novelty_cue);
        self.interest_overlap_without_novelty +=
            usize::from(decision.interest_overlap_without_novelty);
        self.familiar_claim_overlap_count +=
            usize::from(decision.checked && decision.familiar_claim_overlap);
    }
}

pub fn evaluate_delta_eligibility(
    text: &str,
    is_delta_highlight: bool,
    baseline: BaselineEvidence<'_>,
) -> DeltaEligibilityDecision {
    let matched_known_concepts = matched_phrases(text, baseline.known_concepts);
    let matched_familiar_claims = matched_phrases(text, baseline.familiar_claims);
    let matched_interests = matched_phrases(text, baseline.interests);
    let mut novelty_cues = detect_cues(text, NOVELTY_CUES);
    let familiar_cues = detect_cues(text, FAMILIAR_CUES);
    let normalized = normalize(text);
    if normalized.contains("not new") || normalized.contains("no new") {
        novelty_cues.retain(|cue| cue != "new");
    }
    let has_novelty = !novelty_cues.is_empty() || has_numeric_comparison(text);
    let familiar_claim_overlap = !matched_familiar_claims.is_empty() || !familiar_cues.is_empty();

    DeltaEligibilityDecision {
        checked: is_delta_highlight,
        eligible: !is_delta_highlight || !familiar_claim_overlap || has_novelty,
        demoted_by_familiar_claim: is_delta_highlight && familiar_claim_overlap && !has_novelty,
        kept_due_to_novelty_cue: is_delta_highlight && familiar_claim_overlap && has_novelty,
        interest_overlap_without_novelty: is_delta_highlight
            && !matched_interests.is_empty()
            && !has_novelty,
        familiar_claim_overlap,
        matched_known_concepts,
        matched_familiar_claims,
        matched_interests,
        novelty_cues,
        familiar_cues,
    }
}

const NOVELTY_CUES: &[&str] = &[
    "we found",
    "evaluation shows",
    "reduced",
    "increased",
    "failed when",
    "only when",
    "regressed",
    "new",
    "previously unknown",
    "changed from",
    "unlike",
    "limitation",
    "tradeoff",
];

const FAMILIAR_CUES: &[&str] = &[
    "previously validated",
    "already validated",
    "known configuration",
    "standard practice",
    "not new evidence",
    "already deployed",
    "baseline already includes",
];

fn detect_cues(text: &str, cues: &[&str]) -> Vec<String> {
    let normalized = normalize(text);
    let padded_text = format!(" {normalized} ");
    cues.iter()
        .filter(|cue| padded_text.contains(&format!(" {cue} ")))
        .map(|cue| (*cue).to_owned())
        .collect()
}

fn has_numeric_comparison(text: &str) -> bool {
    let normalized = normalize(text);
    let has_digit = normalized
        .chars()
        .any(|character| character.is_ascii_digit());
    has_digit
        && (normalized.contains('%')
            || normalized.contains(" percent")
            || normalized.contains(" versus ")
            || normalized.contains(" compared ")
            || normalized.contains(" than ")
            || normalized.contains(" from ") && normalized.contains(" to ")
            || normalized.contains('<')
            || normalized.contains('>'))
}

fn matched_phrases(text: &str, phrases: &[String]) -> Vec<String> {
    phrases
        .iter()
        .filter(|phrase| phrase_score(text, phrase) >= 0.55)
        .cloned()
        .collect()
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
        return 0.0;
    }
    phrase_tokens
        .iter()
        .filter(|token| text_tokens.contains(*token))
        .count() as f64
        / phrase_tokens.len() as f64
}

fn normalize(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, '%' | '<' | '>') {
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

fn tokens(value: &str) -> std::collections::HashSet<&str> {
    value
        .split_whitespace()
        .filter(|token| token.len() > 2)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn familiar_claim_vetoes_interest_without_novelty() {
        let familiar = vec!["previously validated configuration".to_owned()];
        let interests = vec!["adaptive freshness budget".to_owned()];
        let decision = evaluate_delta_eligibility(
            "The adaptive freshness budget was in the previously validated configuration.",
            true,
            BaselineEvidence {
                familiar_claims: &familiar,
                interests: &interests,
                ..BaselineEvidence::default()
            },
        );
        assert!(decision.demoted_by_familiar_claim);
        assert!(decision.interest_overlap_without_novelty);
        assert!(!decision.eligible);
    }

    #[test]
    fn numeric_new_result_survives_familiar_overlap() {
        let familiar = vec!["adaptive freshness budget".to_owned()];
        let decision = evaluate_delta_eligibility(
            "Evaluation shows the adaptive freshness budget reduced stale answers by 31 percent.",
            true,
            BaselineEvidence {
                familiar_claims: &familiar,
                ..BaselineEvidence::default()
            },
        );
        assert!(decision.eligible);
        assert!(decision.kept_due_to_novelty_cue);
    }

    #[test]
    fn not_new_evidence_does_not_trigger_new_cue() {
        let familiar = vec!["previously validated configuration".to_owned()];
        let decision = evaluate_delta_eligibility(
            "This was in the previously validated configuration and is not new evidence.",
            true,
            BaselineEvidence {
                familiar_claims: &familiar,
                ..BaselineEvidence::default()
            },
        );
        assert!(decision.demoted_by_familiar_claim);
        assert!(!decision.novelty_cues.iter().any(|cue| cue == "new"));
    }
}
