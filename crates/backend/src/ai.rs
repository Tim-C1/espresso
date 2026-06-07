use std::{env, sync::Arc};

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::models::*;

pub type DynChatProvider = Arc<dyn ChatProvider>;

#[async_trait]
pub trait ChatProvider: Send + Sync {
    async fn generate_concepts(&self, chunks: &[TextChunk]) -> anyhow::Result<Vec<ConceptTag>>;
    async fn analyze_delta(
        &self,
        chunks: &[TextChunk],
        concepts: &[ConceptTag],
        baseline: &UserBaseline,
    ) -> anyhow::Result<AiAnalysis>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiAnalysis {
    pub quests: Vec<Quest>,
    pub annotations: Vec<ChunkAnnotation>,
}

pub fn provider_from_env() -> DynChatProvider {
    match env::var("AI_PROVIDER")
        .unwrap_or_else(|_| "mock".to_owned())
        .as_str()
    {
        "openai-compatible" => Arc::new(OpenAiCompatibleProvider::from_env()),
        _ => Arc::new(MockProvider),
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
        concepts: &[ConceptTag],
        baseline: &UserBaseline,
    ) -> anyhow::Result<AiAnalysis> {
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
                } else if chunk.text.len() > 900 {
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
                    priority,
                    directive: directive_for_priority(priority),
                    confidence: confidence_for_priority(priority),
                    rationale: rationale.to_owned(),
                    reader_label: reader_label_for_priority(priority).to_owned(),
                }
            })
            .collect::<Vec<_>>();

        let anchor_ids = annotations
            .iter()
            .filter(|annotation| matches!(annotation.priority, Priority::Delta))
            .take(5)
            .map(|annotation| annotation.chunk_id.clone())
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
            annotations,
        })
    }
}

pub struct OpenAiCompatibleProvider {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl OpenAiCompatibleProvider {
    pub fn from_env() -> Self {
        Self {
            client: Client::new(),
            base_url: env::var("AI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_owned()),
            api_key: env::var("AI_API_KEY").unwrap_or_default(),
            model: env::var("AI_MODEL").unwrap_or_else(|_| "gpt-4.1-mini".to_owned()),
        }
    }

    async fn chat_json<T: for<'de> Deserialize<'de>>(
        &self,
        system: &str,
        user: &str,
    ) -> anyhow::Result<T> {
        if self.api_key.is_empty() {
            anyhow::bail!("AI_API_KEY is required for openai-compatible provider");
        }

        let response: ChatResponse = self
            .client
            .post(format!(
                "{}/chat/completions",
                self.base_url.trim_end_matches('/')
            ))
            .bearer_auth(&self.api_key)
            .json(&json!({
                "model": self.model,
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
}

#[async_trait]
impl ChatProvider for OpenAiCompatibleProvider {
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
        concepts: &[ConceptTag],
        baseline: &UserBaseline,
    ) -> anyhow::Result<AiAnalysis> {
        #[derive(Deserialize)]
        struct BatchEnvelope {
            annotations: Vec<ChunkAnnotation>,
        }

        let mut annotations = Vec::with_capacity(chunks.len());
        for batch in chunks.chunks(self.analysis_batch_size()) {
            let payload = json!({
                "baseline": baseline,
                "concepts": concepts,
                "chunks": batch.iter().map(compact_chunk).collect::<Vec<_>>(),
                "task": "For each chunk, decide how a PDF reading UI should guide the reader without harming readability. Use priority for knowledge value. Use directive highlight only for high-value content that is safe to visually emphasize, callout for useful but risky/low-precision guidance, soft_fade only for clearly redundant background, and leave_normal when the UI should not manipulate the PDF text. Return one annotation for every chunk id."
            });

            let result: BatchEnvelope = self
                .chat_json(
                    "Return strict JSON only. Shape: {\"annotations\":[{\"chunk_id\":\"p1c1\",\"priority\":\"delta|bridge|familiar\",\"directive\":\"highlight|soft_fade|callout|leave_normal\",\"confidence\":0.0,\"reader_label\":\"short UI label\",\"rationale\":\"short reader-facing reason\"}]}. Confidence must be 0 to 1. Prefer callout over highlight when matching exact PDF text may be unreliable.",
                    &payload.to_string(),
                )
                .await?;

            annotations.extend(validate_annotations(batch, result.annotations));
        }

        let quests = build_quests_from_annotations(chunks, &annotations);

        Ok(AiAnalysis {
            quests,
            annotations,
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

fn validate_annotations(
    chunks: &[TextChunk],
    annotations: Vec<ChunkAnnotation>,
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
                .cloned()
                .map(normalize_annotation)
                .filter(|annotation| !annotation.rationale.trim().is_empty())
                .unwrap_or_else(|| fallback_annotation(chunk))
        })
        .collect()
}

fn normalize_annotation(mut annotation: ChunkAnnotation) -> ChunkAnnotation {
    annotation.confidence = annotation.confidence.clamp(0.0, 1.0);
    if annotation.reader_label.trim().is_empty() {
        annotation.reader_label = reader_label_for_priority(annotation.priority).to_owned();
    }
    annotation
}

fn fallback_annotation(chunk: &TextChunk) -> ChunkAnnotation {
    let priority = if chunk.text.len() > 900 {
        Priority::Delta
    } else {
        Priority::Bridge
    };
    ChunkAnnotation {
        chunk_id: chunk.id.clone(),
        priority,
        directive: directive_for_priority(priority),
        confidence: 0.45,
        rationale: "Fallback classification used because the AI response omitted this chunk."
            .to_owned(),
        reader_label: reader_label_for_priority(priority).to_owned(),
    }
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
        Priority::Familiar => ReaderDirective::LeaveNormal,
    }
}

fn confidence_for_priority(priority: Priority) -> f32 {
    match priority {
        Priority::Delta => 0.7,
        Priority::Bridge => 0.55,
        Priority::Familiar => 0.6,
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

        let result = provider
            .analyze_delta(&chunks, &concepts, &baseline)
            .await
            .expect("mock analysis should succeed");

        assert!(matches!(result.annotations[0].priority, Priority::Familiar));
    }
}
