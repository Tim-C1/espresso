use std::{env, sync::Arc};

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::delta_policy::{
    evaluate_delta_eligibility, BaselineEvidence, DeltaEligibilityDiagnostics,
};
use crate::models::*;

pub type DynChatProvider = Arc<dyn ChatProvider>;

#[async_trait]
pub trait ChatProvider: Send + Sync {
    async fn generate_concepts(&self, chunks: &[TextChunk]) -> anyhow::Result<Vec<ConceptTag>>;
    async fn analyze_delta(
        &self,
        chunks: &[TextChunk],
        canonical_candidates: &[CanonicalReadingUnit],
        legacy_sentence_candidates: &[SentenceCandidate],
        concepts: &[ConceptTag],
        baseline: &UserBaseline,
    ) -> anyhow::Result<AiAnalysis>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiAnalysis {
    pub quests: Vec<Quest>,
    pub annotations: Vec<ChunkAnnotation>,
    pub reading_anchors: Vec<ReadingAnchor>,
    pub canonical_annotations: Vec<CanonicalSentenceAnnotation>,
}

pub fn provider_from_env() -> DynChatProvider {
    match AiProviderConfig::from_env() {
        AiProviderConfig::Mock => Arc::new(MockProvider),
        AiProviderConfig::ChatCompletions(config) => Arc::new(ChatCompletionsProvider::new(config)),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AiProviderConfig {
    Mock,
    ChatCompletions(ChatCompletionsConfig),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ChatCompletionsConfig {
    provider: ProviderKind,
    base_url: String,
    api_key: String,
    model: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProviderKind {
    OpenAi,
    Gemini,
    Custom,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ApiKeySource {
    Generic,
    Gemini,
    OpenAi,
}

impl AiProviderConfig {
    fn from_env() -> Self {
        let requested_provider = env::var("AI_PROVIDER").ok();
        if matches!(requested_provider.as_deref(), Some("mock")) {
            return Self::Mock;
        }

        let Some(api_key) = ApiKey::from_env() else {
            return Self::Mock;
        };

        let provider = requested_provider
            .as_deref()
            .map(ProviderKind::from_provider_name)
            .unwrap_or_else(|| ProviderKind::from_key_source(api_key.source));
        Self::ChatCompletions(ChatCompletionsConfig {
            provider,
            base_url: env::var("AI_BASE_URL").unwrap_or_else(|_| provider.default_base_url()),
            api_key: api_key.value,
            model: env::var("AI_MODEL").unwrap_or_else(|_| provider.default_model()),
        })
    }
}

impl ProviderKind {
    fn from_provider_name(provider: &str) -> Self {
        match provider {
            "gemini" => Self::Gemini,
            "openai" | "openai-compatible" => Self::OpenAi,
            "custom" => Self::Custom,
            _ => Self::Custom,
        }
    }

    fn from_key_source(source: ApiKeySource) -> Self {
        match source {
            ApiKeySource::Gemini => Self::Gemini,
            ApiKeySource::OpenAi | ApiKeySource::Generic => Self::OpenAi,
        }
    }

    fn default_base_url(self) -> String {
        match self {
            Self::Gemini => "https://generativelanguage.googleapis.com/v1beta/openai".to_owned(),
            Self::OpenAi | Self::Custom => "https://api.openai.com/v1".to_owned(),
        }
    }

    fn default_model(self) -> String {
        match self {
            Self::Gemini => "gemini-2.5-flash".to_owned(),
            Self::OpenAi | Self::Custom => "gpt-4.1-mini".to_owned(),
        }
    }
}

struct ApiKey {
    source: ApiKeySource,
    value: String,
}

impl ApiKey {
    fn from_env() -> Option<Self> {
        env::var("AI_API_KEY")
            .map(|value| Self {
                source: ApiKeySource::Generic,
                value,
            })
            .or_else(|_| {
                env::var("GEMINI_API_KEY").map(|value| Self {
                    source: ApiKeySource::Gemini,
                    value,
                })
            })
            .or_else(|_| {
                env::var("OPENAI_API_KEY").map(|value| Self {
                    source: ApiKeySource::OpenAi,
                    value,
                })
            })
            .ok()
            .filter(|key| !key.value.trim().is_empty())
    }
}

pub struct MockProvider;

#[async_trait]
impl ChatProvider for MockProvider {
    async fn generate_concepts(&self, chunks: &[TextChunk]) -> anyhow::Result<Vec<ConceptTag>> {
        let joined = chunks
            .iter()
            .take(8)
            .map(|chunk| chunk.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let mut labels = keyword_candidates(&joined);
        labels.truncate(12);

        if labels.is_empty() {
            labels = vec![
                "Core Argument".to_owned(),
                "Evidence".to_owned(),
                "Method".to_owned(),
            ];
        }

        Ok(labels
            .into_iter()
            .enumerate()
            .map(|(index, label)| ConceptTag {
                id: format!("concept-{}", index + 1),
                description: format!("Document concept related to {label}."),
                label,
            })
            .collect())
    }

    async fn analyze_delta(
        &self,
        chunks: &[TextChunk],
        canonical_candidates: &[CanonicalReadingUnit],
        legacy_sentence_candidates: &[SentenceCandidate],
        concepts: &[ConceptTag],
        baseline: &UserBaseline,
    ) -> anyhow::Result<AiAnalysis> {
        if !canonical_candidates.is_empty() {
            return Ok(mock_canonical_analysis(
                canonical_candidates,
                concepts,
                baseline,
            ));
        }

        let baseline_lc = baseline.express_text.to_lowercase();
        let mastered: Vec<String> = concepts
            .iter()
            .filter(|concept| baseline.mastered_concept_ids.contains(&concept.id))
            .map(|concept| concept.label.to_lowercase())
            .collect();

        let annotations = chunks
            .iter()
            .map(|chunk| {
                let text_lc = chunk.text.to_lowercase();
                let familiar_hit = mastered.iter().any(|label| text_lc.contains(label))
                    || baseline_lc
                        .split_whitespace()
                        .filter(|word| word.len() > 5)
                        .any(|word| text_lc.contains(word));
                let priority = if familiar_hit {
                    Priority::Familiar
                } else if chunk.text.len() > 1_100 {
                    Priority::Delta
                } else {
                    Priority::Bridge
                };
                let rationale = match priority {
                    Priority::Delta => {
                        "Likely contains dense or novel material relative to the stated baseline."
                    }
                    Priority::Bridge => "Connects familiar context to the likely novel argument.",
                    Priority::Familiar => {
                        "Overlaps with concepts the reader marked as already known."
                    }
                };

                ChunkAnnotation {
                    chunk_id: chunk.id.clone(),
                    source: AnnotationSource::Mock,
                    priority,
                    directive: directive_for_priority(priority),
                    confidence: confidence_for_priority(priority),
                    rationale: rationale.to_owned(),
                    reader_label: reader_label_for_priority(priority).to_owned(),
                }
            })
            .collect::<Vec<_>>();

        let reading_anchors = annotations
            .iter()
            .filter_map(|annotation| {
                let sentence = legacy_sentence_candidates.iter().find(|candidate| {
                    candidate.chunk_id == annotation.chunk_id
                        && candidate.text.chars().count() <= 500
                        && (candidate.text.chars().count() >= 30 || is_heading(&candidate.text))
                })?;
                Some(anchor_from_sentences(
                    &[sentence],
                    annotation.priority,
                    annotation.directive,
                    annotation.confidence,
                    annotation.rationale.clone(),
                    annotation.reader_label.clone(),
                    AnnotationSource::Mock,
                ))
            })
            .collect::<Vec<_>>();
        let anchor_ids = reading_anchors
            .iter()
            .filter(|anchor| matches!(anchor.priority, Priority::Delta))
            .take(5)
            .map(|anchor| anchor.chunk_id.clone())
            .collect::<Vec<_>>();

        let quests = if anchor_ids.is_empty() {
            vec![Quest {
                id: "quest-1".to_owned(),
                question: "Which part of the author's argument changes what you already believed?"
                    .to_owned(),
                anchor_chunk_ids: chunks
                    .iter()
                    .take(2)
                    .map(|chunk| chunk.id.clone())
                    .collect(),
            }]
        } else {
            anchor_ids
                .iter()
                .enumerate()
                .map(|(index, chunk_id)| Quest {
                    id: format!("quest-{}", index + 1),
                    question: format!(
                        "What new claim or mechanism appears around highlighted section {}?",
                        index + 1
                    ),
                    anchor_chunk_ids: vec![chunk_id.clone()],
                })
                .collect()
        };

        Ok(AiAnalysis {
            quests,
            annotations: Vec::new(),
            reading_anchors,
            canonical_annotations: Vec::new(),
        })
    }
}

pub struct ChatCompletionsProvider {
    client: Client,
    config: ChatCompletionsConfig,
}

impl ChatCompletionsProvider {
    fn new(config: ChatCompletionsConfig) -> Self {
        Self {
            client: Client::new(),
            config,
        }
    }

    async fn chat_json<T: for<'de> Deserialize<'de>>(
        &self,
        system: &str,
        user: &str,
    ) -> anyhow::Result<T> {
        let response: ChatResponse = self
            .client
            .post(format!(
                "{}/chat/completions",
                self.config.base_url.trim_end_matches('/')
            ))
            .bearer_auth(&self.config.api_key)
            .json(&json!({
                "model": self.config.model,
                "response_format": { "type": "json_object" },
                "messages": [
                    { "role": "system", "content": system },
                    { "role": "user", "content": user }
                ]
            }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let content = response
            .choices
            .first()
            .and_then(|choice| choice.message.content.as_deref())
            .ok_or_else(|| anyhow::anyhow!("AI response did not include message content"))?;

        Ok(serde_json::from_str(content)?)
    }

    fn analysis_batch_size(&self) -> usize {
        env::var("AI_ANALYSIS_BATCH_SIZE")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| (1..=60).contains(value))
            .unwrap_or(18)
    }

    async fn analyze_canonical(
        &self,
        candidates: &[CanonicalReadingUnit],
        concepts: &[ConceptTag],
        baseline: &UserBaseline,
    ) -> anyhow::Result<AiAnalysis> {
        #[derive(Deserialize)]
        struct BatchEnvelope {
            annotations: Vec<RawCanonicalAnnotation>,
        }

        let targets = candidates
            .iter()
            .filter(|candidate| matches!(candidate.candidate_kind, CandidateKind::BodySentence))
            .collect::<Vec<_>>();
        let mut canonical_annotations = Vec::new();

        for batch in targets.chunks(self.analysis_batch_size()) {
            let pages = batch
                .iter()
                .map(|candidate| candidate.page)
                .collect::<std::collections::HashSet<_>>();
            let heading_context = candidates
                .iter()
                .filter(|candidate| {
                    pages.contains(&candidate.page)
                        && matches!(candidate.candidate_kind, CandidateKind::Heading)
                })
                .map(compact_canonical_candidate)
                .collect::<Vec<_>>();
            let payload = json!({
                "baseline": baseline,
                "concepts": concepts,
                "sentence_candidates": batch.iter().map(|candidate| compact_canonical_candidate(candidate)).collect::<Vec<_>>(),
                "heading_context": heading_context,
                "task": "Select only provided body_sentence IDs as reading annotations. Return each sentence_id at most once. Headings are context only. Do not select metadata, references, formulas/tables, or short fragments. Use highlight only for genuinely novel delta content, soft_fade only for clearly familiar content, callout for bridge guidance, and leave_normal only for an intentional no-op."
            });
            let result: BatchEnvelope = self
                .chat_json(
                    "Return strict JSON only. Shape: {\"annotations\":[{\"sentence_id\":\"cp1s2\",\"priority\":\"delta|bridge|familiar\",\"directive\":\"highlight|soft_fade|callout|leave_normal\",\"confidence\":0.0,\"reader_label\":\"short UI label\",\"rationale\":\"short reader-facing reason\"}]}. Use only supplied sentence_id values. Confidence must be 0 to 1.",
                    &payload.to_string(),
                )
                .await?;
            let validation_candidates = batch
                .iter()
                .map(|candidate| (*candidate).clone())
                .collect::<Vec<_>>();
            canonical_annotations.extend(validate_canonical_annotations(
                &validation_candidates,
                result.annotations,
                AnnotationSource::Ai,
            ));
        }

        Ok(AiAnalysis {
            quests: build_quests_from_canonical_annotations(&canonical_annotations),
            annotations: Vec::new(),
            reading_anchors: Vec::new(),
            canonical_annotations,
        })
    }
}

#[async_trait]
impl ChatProvider for ChatCompletionsProvider {
    async fn generate_concepts(&self, chunks: &[TextChunk]) -> anyhow::Result<Vec<ConceptTag>> {
        #[derive(Deserialize)]
        struct ConceptEnvelope {
            concepts: Vec<ConceptTag>,
        }

        let sample = chunks
            .iter()
            .take(20)
            .map(|chunk| format!("{}: {}", chunk.id, chunk.text))
            .collect::<Vec<_>>()
            .join("\n\n");

        let result: ConceptEnvelope = self
            .chat_json(
                "Return strict JSON only. Extract 8-12 high-signal conceptual tags from the document. Shape: {\"concepts\":[{\"id\":\"concept-1\",\"label\":\"...\",\"description\":\"...\"}]}",
                &sample,
            )
            .await?;
        Ok(result.concepts)
    }

    async fn analyze_delta(
        &self,
        chunks: &[TextChunk],
        canonical_candidates: &[CanonicalReadingUnit],
        legacy_sentence_candidates: &[SentenceCandidate],
        concepts: &[ConceptTag],
        baseline: &UserBaseline,
    ) -> anyhow::Result<AiAnalysis> {
        if !canonical_candidates.is_empty() {
            return self
                .analyze_canonical(canonical_candidates, concepts, baseline)
                .await;
        }

        #[derive(Deserialize)]
        struct BatchEnvelope {
            anchors: Vec<RawReadingAnchor>,
        }

        let mut reading_anchors = Vec::new();
        for batch in legacy_sentence_candidates.chunks(self.analysis_batch_size() * 3) {
            let chunk_ids = batch
                .iter()
                .map(|candidate| candidate.chunk_id.as_str())
                .collect::<std::collections::HashSet<_>>();
            let payload = json!({
                "baseline": baseline,
                "concepts": concepts,
                "chunks": chunks.iter().filter(|chunk| chunk_ids.contains(chunk.id.as_str())).map(compact_chunk).collect::<Vec<_>>(),
                "sentence_candidates": batch,
                "task": "Select reading anchors by sentence_id. Each anchor must contain 1-3 consecutive sentence_ids from one chunk and form a complete sentence or coherent passage. Never return isolated phrases or an entire long chunk. Use highlight only for genuinely novel high-value anchors, soft_fade only for clearly familiar anchors, callout for bridge context, and leave_normal for intentional no-op guidance. Be selective; omit ordinary candidates rather than anchoring whole chunks."
            });

            let result: BatchEnvelope = self
                .chat_json(
                    "Return strict JSON only. Shape: {\"anchors\":[{\"sentence_ids\":[\"p1c1s2\"],\"priority\":\"delta|bridge|familiar\",\"directive\":\"highlight|soft_fade|callout|leave_normal\",\"confidence\":0.0,\"reader_label\":\"short UI label\",\"rationale\":\"short reader-facing reason\"}]}. Select only provided sentence_ids, 1-3 consecutive sentences per anchor, and complete coherent passages under 500 characters. Confidence must be 0 to 1.",
                    &payload.to_string(),
                )
                .await?;

            reading_anchors.extend(validate_reading_anchors(batch, result.anchors));
        }

        let quests = build_quests_from_anchors(chunks, &reading_anchors);

        Ok(AiAnalysis {
            quests,
            annotations: Vec::new(),
            reading_anchors,
            canonical_annotations: Vec::new(),
        })
    }
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Deserialize)]
struct ChatMessage {
    content: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg(test)]
struct RawChunkAnnotation {
    chunk_id: String,
    #[serde(default)]
    priority: String,
    #[serde(default)]
    directive: String,
    confidence: Option<f32>,
    #[serde(default)]
    rationale: String,
    #[serde(default)]
    reader_label: String,
}

#[derive(Clone, Debug, Deserialize)]
struct RawReadingAnchor {
    #[serde(default)]
    sentence_ids: Vec<String>,
    #[serde(default)]
    priority: String,
    #[serde(default)]
    directive: String,
    confidence: Option<f32>,
    #[serde(default)]
    rationale: String,
    #[serde(default)]
    reader_label: String,
}

#[derive(Clone, Debug, Deserialize)]
struct RawCanonicalAnnotation {
    #[serde(default)]
    sentence_id: String,
    #[serde(default)]
    priority: String,
    #[serde(default)]
    directive: String,
    confidence: Option<f32>,
    #[serde(default)]
    rationale: String,
    #[serde(default)]
    reader_label: String,
}

fn compact_canonical_candidate(candidate: &CanonicalReadingUnit) -> serde_json::Value {
    json!({
        "sentence_id": candidate.sentence_id,
        "page": candidate.page,
        "text": candidate.normalized_text,
        "candidate_kind": candidate.candidate_kind,
    })
}

fn mock_canonical_analysis(
    candidates: &[CanonicalReadingUnit],
    concepts: &[ConceptTag],
    baseline: &UserBaseline,
) -> AiAnalysis {
    let baseline_lc = baseline.express_text.to_lowercase();
    let mastered = concepts
        .iter()
        .filter(|concept| baseline.mastered_concept_ids.contains(&concept.id))
        .map(|concept| concept.label.to_lowercase())
        .collect::<Vec<_>>();
    let mut selected_per_page = std::collections::BTreeMap::<usize, usize>::new();
    let canonical_annotations = candidates
        .iter()
        .filter(|candidate| matches!(candidate.candidate_kind, CandidateKind::BodySentence))
        .filter(|candidate| {
            let count = selected_per_page.entry(candidate.page).or_default();
            if *count >= 2 {
                false
            } else {
                *count += 1;
                true
            }
        })
        .map(|candidate| {
            let text_lc = candidate.normalized_text.to_lowercase();
            let familiar = mastered.iter().any(|label| text_lc.contains(label))
                || baseline_lc
                    .split_whitespace()
                    .filter(|word| word.len() > 5)
                    .any(|word| text_lc.contains(word));
            let priority = if familiar {
                Priority::Familiar
            } else if candidate.normalized_text.chars().count() >= 100 {
                Priority::Delta
            } else {
                Priority::Bridge
            };
            canonical_annotation_from_candidate(
                candidate,
                priority,
                directive_for_priority(priority),
                confidence_for_priority(priority),
                "Deterministic canonical sentence selection for local development.".to_owned(),
                reader_label_for_priority(priority).to_owned(),
                AnnotationSource::Mock,
            )
        })
        .collect::<Vec<_>>();

    AiAnalysis {
        quests: build_quests_from_canonical_annotations(&canonical_annotations),
        annotations: Vec::new(),
        reading_anchors: Vec::new(),
        canonical_annotations,
    }
}

fn validate_canonical_annotations(
    candidates: &[CanonicalReadingUnit],
    raw_annotations: Vec<RawCanonicalAnnotation>,
    source: AnnotationSource,
) -> Vec<CanonicalSentenceAnnotation> {
    let by_id = candidates
        .iter()
        .map(|candidate| (candidate.sentence_id.as_str(), candidate))
        .collect::<std::collections::HashMap<_, _>>();
    let mut seen = std::collections::HashSet::new();

    raw_annotations
        .into_iter()
        .filter_map(|raw| {
            if !seen.insert(raw.sentence_id.clone()) {
                return None;
            }
            let candidate = by_id.get(raw.sentence_id.as_str()).copied()?;
            if candidate.item_ranges.is_empty() {
                return None;
            }
            let priority = parse_priority(&raw.priority)?;
            let directive = parse_directive(&raw.directive)?;
            if !valid_directive_priority(directive, priority)
                || !candidate_kind_allows_directive(candidate.candidate_kind, directive)
            {
                return None;
            }
            if matches!(
                directive,
                ReaderDirective::Highlight | ReaderDirective::SoftFade
            ) && candidate.normalized_text.chars().count() > 500
            {
                return None;
            }
            Some(canonical_annotation_from_candidate(
                candidate,
                priority,
                directive,
                raw.confidence.unwrap_or(0.5).clamp(0.0, 1.0),
                raw.rationale,
                if raw.reader_label.trim().is_empty() {
                    reader_label_for_priority(priority).to_owned()
                } else {
                    raw.reader_label
                },
                source,
            ))
        })
        .collect()
}

fn candidate_kind_allows_directive(kind: CandidateKind, directive: ReaderDirective) -> bool {
    match directive {
        ReaderDirective::Highlight | ReaderDirective::SoftFade => {
            matches!(kind, CandidateKind::BodySentence)
        }
        ReaderDirective::Callout | ReaderDirective::LeaveNormal => true,
    }
}

fn valid_directive_priority(directive: ReaderDirective, priority: Priority) -> bool {
    match directive {
        ReaderDirective::Highlight => matches!(priority, Priority::Delta),
        ReaderDirective::SoftFade => matches!(priority, Priority::Familiar),
        ReaderDirective::Callout | ReaderDirective::LeaveNormal => true,
    }
}

fn canonical_annotation_from_candidate(
    candidate: &CanonicalReadingUnit,
    priority: Priority,
    directive: ReaderDirective,
    confidence: f32,
    rationale: String,
    reader_label: String,
    source: AnnotationSource,
) -> CanonicalSentenceAnnotation {
    CanonicalSentenceAnnotation {
        annotation_id: format!("ca-{}", candidate.sentence_id),
        sentence_id: candidate.sentence_id.clone(),
        page: candidate.page,
        directive,
        priority,
        confidence,
        source,
        source_text: candidate.normalized_text.clone(),
        item_ranges: candidate.item_ranges.clone(),
        quote_selector: candidate.quote_selector.clone(),
        candidate_kind: candidate.candidate_kind,
        rationale,
        reader_label,
    }
}

fn keyword_candidates(text: &str) -> Vec<String> {
    let stop = [
        "about", "after", "again", "author", "because", "before", "between", "could", "document",
        "first", "from", "have", "into", "more", "paper", "that", "their", "there", "these",
        "this", "through", "with", "would",
    ];

    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for raw in text.split(|c: char| !c.is_alphanumeric()) {
        let word = raw.trim().to_lowercase();
        if word.len() < 6 || stop.contains(&word.as_str()) {
            continue;
        }
        *counts.entry(title_case(&word)).or_default() += 1;
    }

    let mut pairs = counts.into_iter().collect::<Vec<_>>();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    pairs.into_iter().map(|(word, _)| word).collect()
}

fn compact_chunk(chunk: &TextChunk) -> serde_json::Value {
    json!({
        "id": chunk.id,
        "page": chunk.page,
        "text": truncate_chars(&chunk.text, 1_200)
    })
}

fn validate_reading_anchors(
    candidates: &[SentenceCandidate],
    raw_anchors: Vec<RawReadingAnchor>,
) -> Vec<ReadingAnchor> {
    let by_id = candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| (candidate.sentence_id.as_str(), (index, candidate)))
        .collect::<std::collections::HashMap<_, _>>();

    raw_anchors
        .into_iter()
        .filter_map(|raw| {
            if raw.sentence_ids.is_empty() || raw.sentence_ids.len() > 3 {
                return None;
            }
            let selected = raw
                .sentence_ids
                .iter()
                .map(|id| by_id.get(id.as_str()).copied())
                .collect::<Option<Vec<_>>>()?;
            let first = selected.first()?;
            if selected.iter().any(|(_, sentence)| {
                sentence.chunk_id != first.1.chunk_id || sentence.page != first.1.page
            }) || selected
                .windows(2)
                .any(|window| window[1].0 != window[0].0 + 1)
            {
                return None;
            }
            let sentences = selected
                .iter()
                .map(|(_, sentence)| *sentence)
                .collect::<Vec<_>>();
            let text = sentences
                .iter()
                .map(|sentence| sentence.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            if text.chars().count() > 500 || (text.chars().count() < 30 && !is_heading(&text)) {
                return None;
            }
            let priority = parse_priority(&raw.priority)?;
            let directive = parse_directive(&raw.directive)?;
            Some(anchor_from_sentences(
                &sentences,
                priority,
                directive,
                raw.confidence.unwrap_or(0.5).clamp(0.0, 1.0),
                raw.rationale,
                if raw.reader_label.trim().is_empty() {
                    reader_label_for_priority(priority).to_owned()
                } else {
                    raw.reader_label
                },
                AnnotationSource::Ai,
            ))
        })
        .collect()
}

fn anchor_from_sentences(
    sentences: &[&SentenceCandidate],
    priority: Priority,
    directive: ReaderDirective,
    confidence: f32,
    rationale: String,
    reader_label: String,
    source: AnnotationSource,
) -> ReadingAnchor {
    let first = sentences.first().expect("anchor requires a sentence");
    let last = sentences.last().expect("anchor requires a sentence");
    ReadingAnchor {
        anchor_id: format!("a-{}-{}", first.sentence_id, last.sentence_id),
        sentence_ids: sentences
            .iter()
            .map(|sentence| sentence.sentence_id.clone())
            .collect(),
        chunk_id: first.chunk_id.clone(),
        page: first.page,
        text: sentences
            .iter()
            .map(|sentence| sentence.text.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        char_start: first.char_start,
        char_end: last.char_end,
        source,
        priority,
        directive,
        confidence,
        rationale,
        reader_label,
    }
}

fn is_heading(text: &str) -> bool {
    text.chars().count() <= 100
        && text.split_whitespace().count() <= 12
        && !text.trim_end().ends_with(['.', '!', '?'])
}

#[cfg(test)]
fn validate_annotations(
    chunks: &[TextChunk],
    annotations: Vec<RawChunkAnnotation>,
) -> Vec<ChunkAnnotation> {
    let by_id = annotations
        .into_iter()
        .map(|annotation| (annotation.chunk_id.clone(), annotation))
        .collect::<std::collections::HashMap<_, _>>();

    chunks
        .iter()
        .map(|chunk| {
            by_id
                .get(&chunk.id)
                .map(|annotation| normalize_annotation(chunk, annotation))
                .filter(|annotation| !annotation.rationale.trim().is_empty())
                .unwrap_or_else(|| fallback_annotation(chunk))
        })
        .collect()
}

#[cfg(test)]
fn normalize_annotation(chunk: &TextChunk, annotation: &RawChunkAnnotation) -> ChunkAnnotation {
    let directive = parse_directive(&annotation.directive).unwrap_or(ReaderDirective::LeaveNormal);
    let priority = parse_priority(&annotation.priority).unwrap_or_else(|| match directive {
        ReaderDirective::Highlight => Priority::Delta,
        ReaderDirective::SoftFade => Priority::Familiar,
        ReaderDirective::Callout | ReaderDirective::LeaveNormal => fallback_priority(chunk),
    });
    let reader_label = if annotation.reader_label.trim().is_empty() {
        reader_label_for_priority(priority).to_owned()
    } else {
        annotation.reader_label.clone()
    };

    ChunkAnnotation {
        chunk_id: chunk.id.clone(),
        source: AnnotationSource::Ai,
        priority,
        directive,
        confidence: annotation.confidence.unwrap_or(0.5).clamp(0.0, 1.0),
        rationale: annotation.rationale.clone(),
        reader_label,
    }
}

#[cfg(test)]
fn fallback_annotation(chunk: &TextChunk) -> ChunkAnnotation {
    let priority = fallback_priority(chunk);
    ChunkAnnotation {
        chunk_id: chunk.id.clone(),
        source: AnnotationSource::Fallback,
        priority,
        directive: ReaderDirective::LeaveNormal,
        confidence: 0.45,
        rationale: "Fallback classification used because the AI response omitted this chunk."
            .to_owned(),
        reader_label: reader_label_for_priority(priority).to_owned(),
    }
}

#[cfg(test)]
fn fallback_priority(chunk: &TextChunk) -> Priority {
    if chunk.text.len() > 900 {
        Priority::Delta
    } else {
        Priority::Bridge
    }
}

fn parse_priority(value: &str) -> Option<Priority> {
    match normalize_enum_token(value).as_str() {
        "delta" => Some(Priority::Delta),
        "bridge" => Some(Priority::Bridge),
        "familiar" => Some(Priority::Familiar),
        _ => None,
    }
}

fn parse_directive(value: &str) -> Option<ReaderDirective> {
    match normalize_enum_token(value).as_str() {
        "highlight" => Some(ReaderDirective::Highlight),
        "soft_fade" | "softfade" | "fade" => Some(ReaderDirective::SoftFade),
        "callout" => Some(ReaderDirective::Callout),
        "leave_normal" | "leavenormal" | "normal" => Some(ReaderDirective::LeaveNormal),
        _ => None,
    }
}

fn normalize_enum_token(value: &str) -> String {
    value.trim().to_lowercase().replace(['-', ' '], "_")
}

fn build_quests_from_annotations(
    chunks: &[TextChunk],
    annotations: &[ChunkAnnotation],
) -> Vec<Quest> {
    let chunk_by_id = chunks
        .iter()
        .map(|chunk| (chunk.id.as_str(), chunk))
        .collect::<std::collections::HashMap<_, _>>();
    let delta_ids = annotations
        .iter()
        .filter(|annotation| matches!(annotation.priority, Priority::Delta))
        .filter_map(|annotation| chunk_by_id.get(annotation.chunk_id.as_str()))
        .take(5)
        .collect::<Vec<_>>();

    if delta_ids.is_empty() {
        return vec![Quest {
            id: "quest-1".to_owned(),
            question: "Which part of the author's argument changes what you already believed?"
                .to_owned(),
            anchor_chunk_ids: chunks
                .iter()
                .take(2)
                .map(|chunk| chunk.id.clone())
                .collect(),
        }];
    }

    delta_ids
        .iter()
        .enumerate()
        .map(|(index, chunk)| Quest {
            id: format!("quest-{}", index + 1),
            question: format!(
                "On page {}, what new claim, mechanism, or tradeoff is worth slowing down for?",
                chunk.page
            ),
            anchor_chunk_ids: vec![chunk.id.clone()],
        })
        .collect()
}

fn build_quests_from_anchors(chunks: &[TextChunk], anchors: &[ReadingAnchor]) -> Vec<Quest> {
    let legacy = anchors
        .iter()
        .map(|anchor| ChunkAnnotation {
            chunk_id: anchor.chunk_id.clone(),
            source: anchor.source,
            priority: anchor.priority,
            directive: anchor.directive,
            confidence: anchor.confidence,
            rationale: anchor.rationale.clone(),
            reader_label: anchor.reader_label.clone(),
        })
        .collect::<Vec<_>>();
    build_quests_from_annotations(chunks, &legacy)
}

fn build_quests_from_canonical_annotations(
    annotations: &[CanonicalSentenceAnnotation],
) -> Vec<Quest> {
    let selected = annotations
        .iter()
        .filter(|annotation| matches!(annotation.priority, Priority::Delta))
        .take(5)
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return vec![Quest {
            id: "quest-1".to_owned(),
            question: "Which part of the author's argument changes what you already believed?"
                .to_owned(),
            anchor_chunk_ids: annotations
                .iter()
                .take(2)
                .map(|annotation| annotation.sentence_id.clone())
                .collect(),
        }];
    }
    selected
        .iter()
        .enumerate()
        .map(|(index, annotation)| Quest {
            id: format!("quest-{}", index + 1),
            question: format!(
                "On page {}, what new claim, mechanism, or tradeoff is worth slowing down for?",
                annotation.page
            ),
            anchor_chunk_ids: vec![annotation.sentence_id.clone()],
        })
        .collect()
}

pub(crate) fn rebuild_canonical_quests(annotations: &[CanonicalSentenceAnnotation]) -> Vec<Quest> {
    build_quests_from_canonical_annotations(annotations)
}

pub(crate) fn apply_delta_eligibility_gate(
    annotations: &mut [CanonicalSentenceAnnotation],
    concepts: &[ConceptTag],
    baseline: &UserBaseline,
) -> DeltaEligibilityDiagnostics {
    let known_concepts = concepts
        .iter()
        .filter(|concept| baseline.mastered_concept_ids.contains(&concept.id))
        .map(|concept| concept.label.clone())
        .collect::<Vec<_>>();
    let familiar_claims = baseline_claims(&baseline.express_text);
    let interests = Vec::new();
    let evidence = BaselineEvidence {
        known_concepts: &known_concepts,
        familiar_claims: &familiar_claims,
        interests: &interests,
    };
    let mut diagnostics = DeltaEligibilityDiagnostics::default();

    for annotation in annotations {
        let is_delta_highlight = matches!(annotation.priority, Priority::Delta)
            && matches!(annotation.directive, ReaderDirective::Highlight);
        if !is_delta_highlight {
            continue;
        }
        let decision = evaluate_delta_eligibility(
            &annotation.source_text,
            is_delta_highlight,
            evidence.clone(),
        );
        diagnostics.record(&decision);
        if decision.demoted_by_familiar_claim {
            annotation.priority = Priority::Familiar;
            annotation.directive = ReaderDirective::LeaveNormal;
            annotation.reader_label = reader_label_for_priority(Priority::Familiar).to_owned();
            annotation.rationale = format!(
                "{} Delta eligibility gate demoted this annotation because it overlaps familiar baseline evidence without a novelty cue.",
                annotation.rationale.trim()
            )
            .trim()
            .to_owned();
        }
    }
    diagnostics
}

fn baseline_claims(text: &str) -> Vec<String> {
    text.split(['\n', '.', ';'])
        .map(str::trim)
        .filter(|claim| !claim.is_empty())
        .map(str::to_owned)
        .collect()
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let mut output = String::new();
    for character in text.chars().take(max_chars) {
        output.push(character);
    }
    output
}

fn directive_for_priority(priority: Priority) -> ReaderDirective {
    match priority {
        Priority::Delta => ReaderDirective::Highlight,
        Priority::Bridge => ReaderDirective::Callout,
        Priority::Familiar => ReaderDirective::SoftFade,
    }
}

fn confidence_for_priority(priority: Priority) -> f32 {
    match priority {
        Priority::Delta => 0.74,
        Priority::Bridge => 0.55,
        Priority::Familiar => 0.68,
    }
}

fn reader_label_for_priority(priority: Priority) -> &'static str {
    match priority {
        Priority::Delta => "New insight",
        Priority::Bridge => "Bridge context",
        Priority::Familiar => "Likely familiar",
    }
}

fn title_case(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    fn clear_ai_env() {
        env::remove_var("AI_PROVIDER");
        env::remove_var("AI_BASE_URL");
        env::remove_var("AI_MODEL");
        env::remove_var("AI_API_KEY");
        env::remove_var("GEMINI_API_KEY");
        env::remove_var("OPENAI_API_KEY");
    }

    #[tokio::test]
    async fn mock_provider_marks_mastered_concepts_familiar() {
        let provider = MockProvider;
        let chunks = vec![TextChunk {
            id: "p1c1".to_owned(),
            page: 1,
            text: "This section explains transformers and attention.".to_owned(),
        }];
        let concepts = vec![ConceptTag {
            id: "concept-1".to_owned(),
            label: "Transformers".to_owned(),
            description: "Model family".to_owned(),
        }];
        let baseline = UserBaseline {
            express_text: String::new(),
            mastered_concept_ids: vec!["concept-1".to_owned()],
        };
        let sentence_candidates = crate::pdf::extract_sentence_candidates(&chunks);

        let result = provider
            .analyze_delta(&chunks, &[], &sentence_candidates, &concepts, &baseline)
            .await
            .expect("mock analysis should succeed");

        assert!(result.annotations.is_empty());
        assert!(matches!(
            result.reading_anchors[0].priority,
            Priority::Familiar
        ));
        assert!(matches!(
            result.reading_anchors[0].source,
            AnnotationSource::Mock
        ));
        assert!(matches!(
            result.reading_anchors[0].directive,
            ReaderDirective::SoftFade
        ));
    }

    #[test]
    fn fallback_annotations_do_not_manipulate_pdf_text() {
        let chunks = vec![TextChunk {
            id: "p1c1".to_owned(),
            page: 1,
            text: "A long omitted chunk. ".repeat(80),
        }];

        let annotations = validate_annotations(&chunks, Vec::new());

        assert!(matches!(annotations[0].priority, Priority::Delta));
        assert!(matches!(annotations[0].source, AnnotationSource::Fallback));
        assert!(matches!(
            annotations[0].directive,
            ReaderDirective::LeaveNormal
        ));
    }

    #[test]
    fn rejects_unknown_sentence_ids() {
        let candidates = vec![SentenceCandidate {
            sentence_id: "p1c1s1".to_owned(),
            chunk_id: "p1c1".to_owned(),
            page: 1,
            text: "This complete sentence is long enough to anchor safely.".to_owned(),
            char_start: 0,
            char_end: 54,
        }];
        let anchors = validate_reading_anchors(
            &candidates,
            vec![RawReadingAnchor {
                sentence_ids: vec!["unknown".to_owned()],
                priority: "delta".to_owned(),
                directive: "highlight".to_owned(),
                confidence: Some(0.9),
                rationale: "unknown".to_owned(),
                reader_label: String::new(),
            }],
        );
        assert!(anchors.is_empty());
    }

    #[test]
    fn canonical_validation_rejects_unknown_and_missing_ranges() {
        let valid = canonical_candidate("cp1s1", CandidateKind::BodySentence);
        let mut missing_ranges = canonical_candidate("cp1s2", CandidateKind::BodySentence);
        missing_ranges.item_ranges.clear();
        let raw = |sentence_id: &str| RawCanonicalAnnotation {
            sentence_id: sentence_id.to_owned(),
            priority: "delta".to_owned(),
            directive: "highlight".to_owned(),
            confidence: Some(0.9),
            rationale: "Novel mechanism.".to_owned(),
            reader_label: String::new(),
        };

        assert!(validate_canonical_annotations(
            &[valid.clone(), missing_ranges.clone()],
            vec![raw("unknown")],
            AnnotationSource::Ai,
        )
        .is_empty());
        assert!(validate_canonical_annotations(
            &[valid, missing_ranges],
            vec![raw("cp1s2")],
            AnnotationSource::Ai,
        )
        .is_empty());
        assert!(validate_canonical_annotations(
            &[canonical_candidate("cp1s1", CandidateKind::BodySentence)],
            vec![RawCanonicalAnnotation {
                sentence_id: "cp1s1".to_owned(),
                priority: "novel".to_owned(),
                directive: "paint".to_owned(),
                confidence: Some(0.9),
                rationale: "Invalid enums.".to_owned(),
                reader_label: String::new(),
            }],
            AnnotationSource::Ai,
        )
        .is_empty());
    }

    #[test]
    fn canonical_annotation_resolves_ranges_and_quote_selector() {
        let candidate = canonical_candidate("cp1s1", CandidateKind::BodySentence);
        let annotations = validate_canonical_annotations(
            std::slice::from_ref(&candidate),
            vec![RawCanonicalAnnotation {
                sentence_id: candidate.sentence_id.clone(),
                priority: "delta".to_owned(),
                directive: "highlight".to_owned(),
                confidence: Some(0.91),
                rationale: "Novel mechanism.".to_owned(),
                reader_label: String::new(),
            }],
            AnnotationSource::Ai,
        );

        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].item_ranges.len(), 1);
        assert_eq!(annotations[0].item_ranges[0].item_id, "p1i1");
        assert_eq!(
            annotations[0].quote_selector.exact,
            candidate.normalized_text
        );
        assert_eq!(annotations[0].quote_selector.prefix, "Earlier context");
        assert_eq!(annotations[0].quote_selector.suffix, "Later context");
        assert_eq!(annotations[0].source_text, candidate.normalized_text);
    }

    #[test]
    fn metadata_and_references_cannot_be_inline_targets() {
        let metadata = canonical_candidate("cp1s1", CandidateKind::Metadata);
        let reference = canonical_candidate("cp9s1", CandidateKind::Reference);
        let raw = |sentence_id: &str, directive: &str, priority: &str| RawCanonicalAnnotation {
            sentence_id: sentence_id.to_owned(),
            priority: priority.to_owned(),
            directive: directive.to_owned(),
            confidence: Some(0.9),
            rationale: "Context.".to_owned(),
            reader_label: String::new(),
        };

        assert!(validate_canonical_annotations(
            &[metadata.clone(), reference.clone()],
            vec![raw("cp1s1", "highlight", "delta")],
            AnnotationSource::Ai,
        )
        .is_empty());
        assert!(validate_canonical_annotations(
            &[metadata.clone(), reference],
            vec![raw("cp9s1", "soft_fade", "familiar")],
            AnnotationSource::Ai,
        )
        .is_empty());
        let callout = validate_canonical_annotations(
            &[metadata],
            vec![raw("cp1s1", "callout", "bridge")],
            AnnotationSource::Ai,
        );
        assert_eq!(callout.len(), 1, "non-inline callouts remain valid");
    }

    #[test]
    fn mock_canonical_analysis_prefers_body_sentences() {
        let candidates = vec![
            canonical_candidate("cp1s1", CandidateKind::Metadata),
            canonical_candidate("cp1s2", CandidateKind::BodySentence),
            canonical_candidate("cp1s3", CandidateKind::Reference),
        ];
        let analysis = mock_canonical_analysis(
            &candidates,
            &[],
            &UserBaseline {
                express_text: String::new(),
                mastered_concept_ids: Vec::new(),
            },
        );

        assert_eq!(analysis.canonical_annotations.len(), 1);
        assert_eq!(analysis.canonical_annotations[0].sentence_id, "cp1s2");
        assert_eq!(
            analysis.canonical_annotations[0].candidate_kind,
            CandidateKind::BodySentence
        );
    }

    fn canonical_candidate(sentence_id: &str, kind: CandidateKind) -> CanonicalReadingUnit {
        let text = "This canonical body sentence is long enough for a safe inline annotation.";
        CanonicalReadingUnit {
            sentence_id: sentence_id.to_owned(),
            page: if sentence_id.starts_with("cp9") { 9 } else { 1 },
            text: text.to_owned(),
            normalized_text: text.to_owned(),
            norm_start: 0,
            norm_end: text.encode_utf16().count(),
            start_item_id: "p1i1".to_owned(),
            end_item_id: "p1i1".to_owned(),
            item_ranges: vec![CanonicalItemRange {
                item_id: "p1i1".to_owned(),
                normalized_start: 0,
                normalized_end: text.encode_utf16().count(),
            }],
            quote_selector: TextQuoteSelector {
                exact: text.to_owned(),
                prefix: "Earlier context".to_owned(),
                suffix: "Later context".to_owned(),
            },
            candidate_kind: kind,
        }
    }

    #[test]
    fn post_validation_gate_demotes_familiar_delta_and_keeps_new_result() {
        let mut familiar_candidate = canonical_candidate("cp1s1", CandidateKind::BodySentence);
        familiar_candidate.normalized_text =
            "This was part of the previously validated configuration.".to_owned();
        let mut novel_candidate = canonical_candidate("cp1s2", CandidateKind::BodySentence);
        novel_candidate.normalized_text =
            "Evaluation shows the previously validated configuration reduced errors by 31 percent."
                .to_owned();
        let mut annotations = vec![
            canonical_annotation_from_candidate(
                &familiar_candidate,
                Priority::Delta,
                ReaderDirective::Highlight,
                0.9,
                "Predicted delta.".to_owned(),
                "New insight".to_owned(),
                AnnotationSource::Ai,
            ),
            canonical_annotation_from_candidate(
                &novel_candidate,
                Priority::Delta,
                ReaderDirective::Highlight,
                0.9,
                "Predicted delta.".to_owned(),
                "New insight".to_owned(),
                AnnotationSource::Ai,
            ),
        ];
        let diagnostics = apply_delta_eligibility_gate(
            &mut annotations,
            &[],
            &UserBaseline {
                express_text: "previously validated configuration".to_owned(),
                mastered_concept_ids: Vec::new(),
            },
        );

        assert_eq!(annotations[0].priority, Priority::Familiar);
        assert_eq!(annotations[0].directive, ReaderDirective::LeaveNormal);
        assert_eq!(annotations[1].priority, Priority::Delta);
        assert_eq!(diagnostics.delta_eligibility_checked, 2);
        assert_eq!(diagnostics.delta_demoted_by_familiar_claim, 1);
        assert_eq!(diagnostics.delta_kept_due_to_novelty_cue, 1);
        assert_eq!(diagnostics.familiar_claim_overlap_count, 2);
    }

    #[test]
    fn rejects_chunk_sized_reading_anchors() {
        let long_text = format!("{}.", "Long sentence content ".repeat(30));
        let candidates = vec![SentenceCandidate {
            sentence_id: "p1c1s1".to_owned(),
            chunk_id: "p1c1".to_owned(),
            page: 1,
            text: long_text.clone(),
            char_start: 0,
            char_end: long_text.chars().count(),
        }];
        let anchors = validate_reading_anchors(
            &candidates,
            vec![RawReadingAnchor {
                sentence_ids: vec!["p1c1s1".to_owned()],
                priority: "delta".to_owned(),
                directive: "highlight".to_owned(),
                confidence: Some(0.9),
                rationale: "too long".to_owned(),
                reader_label: String::new(),
            }],
        );
        assert!(anchors.is_empty());
    }

    #[test]
    fn normalizes_confidence_and_missing_label() {
        let chunk = TextChunk {
            id: "p1c1".to_owned(),
            page: 1,
            text: "Novel mechanism.".to_owned(),
        };
        let raw = RawChunkAnnotation {
            chunk_id: "p1c1".to_owned(),
            priority: "delta".to_owned(),
            directive: "highlight".to_owned(),
            confidence: Some(2.5),
            rationale: "Useful novelty.".to_owned(),
            reader_label: String::new(),
        };

        let annotation = normalize_annotation(&chunk, &raw);

        assert_eq!(annotation.confidence, 1.0);
        assert_eq!(annotation.reader_label, "New insight");
        assert!(matches!(annotation.source, AnnotationSource::Ai));
    }

    #[test]
    fn misplaced_directive_in_priority_is_conservative_not_error() {
        let chunks = vec![TextChunk {
            id: "p1c1".to_owned(),
            page: 1,
            text: "Brief context.".to_owned(),
        }];
        let annotations = validate_annotations(
            &chunks,
            vec![RawChunkAnnotation {
                chunk_id: "p1c1".to_owned(),
                priority: "leave_normal".to_owned(),
                directive: "leave_normal".to_owned(),
                confidence: Some(0.73),
                rationale: "Ordinary background.".to_owned(),
                reader_label: String::new(),
            }],
        );

        assert!(matches!(annotations[0].priority, Priority::Bridge));
        assert!(matches!(
            annotations[0].directive,
            ReaderDirective::LeaveNormal
        ));
        assert_eq!(annotations[0].confidence, 0.73);
    }

    #[test]
    fn gemini_key_selects_gemini_defaults() {
        let _guard = env_lock();
        clear_ai_env();
        env::set_var("GEMINI_API_KEY", "test-key");

        assert_eq!(
            AiProviderConfig::from_env(),
            AiProviderConfig::ChatCompletions(ChatCompletionsConfig {
                provider: ProviderKind::Gemini,
                base_url: "https://generativelanguage.googleapis.com/v1beta/openai".to_owned(),
                api_key: "test-key".to_owned(),
                model: "gemini-2.5-flash".to_owned(),
            })
        );

        clear_ai_env();
    }

    #[test]
    fn ai_api_key_takes_precedence_over_provider_specific_keys() {
        let _guard = env_lock();
        clear_ai_env();
        env::set_var("AI_API_KEY", "ai-key");
        env::set_var("GEMINI_API_KEY", "gemini-key");
        env::set_var("OPENAI_API_KEY", "openai-key");

        assert_eq!(
            AiProviderConfig::from_env(),
            AiProviderConfig::ChatCompletions(ChatCompletionsConfig {
                provider: ProviderKind::OpenAi,
                base_url: "https://api.openai.com/v1".to_owned(),
                api_key: "ai-key".to_owned(),
                model: "gpt-4.1-mini".to_owned(),
            })
        );

        clear_ai_env();
    }

    #[test]
    fn explicit_provider_and_overrides_select_custom_chat_adapter() {
        let _guard = env_lock();
        clear_ai_env();
        env::set_var("AI_PROVIDER", "custom");
        env::set_var("AI_API_KEY", "custom-key");
        env::set_var("AI_BASE_URL", "https://example.test/v1");
        env::set_var("AI_MODEL", "custom-model");

        assert_eq!(
            AiProviderConfig::from_env(),
            AiProviderConfig::ChatCompletions(ChatCompletionsConfig {
                provider: ProviderKind::Custom,
                base_url: "https://example.test/v1".to_owned(),
                api_key: "custom-key".to_owned(),
                model: "custom-model".to_owned(),
            })
        );

        clear_ai_env();
    }

    #[test]
    fn no_key_uses_mock_provider() {
        let _guard = env_lock();
        clear_ai_env();

        assert_eq!(AiProviderConfig::from_env(), AiProviderConfig::Mock);

        clear_ai_env();
    }
}
