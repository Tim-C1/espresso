use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type DocumentId = Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocumentSession {
    pub id: DocumentId,
    pub filename: String,
    pub uploaded_at: DateTime<Utc>,
    pub page_count: usize,
    pub pdf_bytes: Vec<u8>,
    pub chunks: Vec<TextChunk>,
    pub concepts: Vec<ConceptTag>,
    pub baseline: Option<UserBaseline>,
    pub quests: Vec<Quest>,
    pub annotations: Vec<ChunkAnnotation>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TextChunk {
    pub id: String,
    pub page: usize,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConceptTag {
    pub id: String,
    pub label: String,
    pub description: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserBaseline {
    pub express_text: String,
    pub mastered_concept_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Delta,
    Bridge,
    Familiar,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReaderDirective {
    Highlight,
    SoftFade,
    Callout,
    LeaveNormal,
}

fn default_confidence() -> f32 {
    0.5
}

fn default_reader_directive() -> ReaderDirective {
    ReaderDirective::LeaveNormal
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChunkAnnotation {
    pub chunk_id: String,
    pub priority: Priority,
    #[serde(default = "default_reader_directive")]
    pub directive: ReaderDirective,
    #[serde(default = "default_confidence")]
    pub confidence: f32,
    pub rationale: String,
    #[serde(default)]
    pub reader_label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Quest {
    pub id: String,
    pub question: String,
    pub anchor_chunk_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct UploadResponse {
    pub document_id: DocumentId,
    pub page_count: usize,
    pub extraction_status: String,
}

#[derive(Debug, Deserialize)]
pub struct BaselineRequest {
    #[serde(default)]
    pub express_text: String,
    #[serde(default)]
    pub mastered_concept_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct AnalyzeResponse {
    pub quests: Vec<Quest>,
    pub chunk_annotations: Vec<ChunkAnnotation>,
}

#[derive(Debug, Serialize)]
pub struct ReaderResponse {
    pub document_id: DocumentId,
    pub filename: String,
    pub page_count: usize,
    pub chunks: Vec<TextChunk>,
    pub concepts: Vec<ConceptTag>,
    pub baseline: Option<UserBaseline>,
    pub quests: Vec<Quest>,
    pub chunk_annotations: Vec<ChunkAnnotation>,
}
