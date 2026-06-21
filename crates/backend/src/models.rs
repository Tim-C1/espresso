use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type DocumentId = Uuid;

pub const CANONICAL_TEXT_SCHEMA_VERSION: &str = "1.0";

/// PDF.js-derived text used as the canonical coordinate space for future
/// sentence anchors. Offsets are UTF-16 code-unit offsets so they have the
/// same indexing semantics as JavaScript and PDF.js text items.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CanonicalTextModel {
    pub schema_version: String,
    pub document_id: DocumentId,
    pub pdf_hash: String,
    pub extractor: CanonicalExtractorMetadata,
    pub pages: Vec<CanonicalTextPage>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CanonicalExtractorMetadata {
    pub name: String,
    pub version: String,
    pub options: CanonicalExtractorOptions,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CanonicalExtractorOptions {
    pub disable_font_face: bool,
    pub include_marked_content: bool,
    pub use_system_fonts: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CanonicalTextPage {
    pub page: usize,
    pub raw_text: String,
    pub normalized_text: String,
    pub text_items: Vec<CanonicalTextItem>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CanonicalTextItem {
    pub item_id: String,
    pub page: usize,
    pub r#str: String,
    pub normalized_str: String,
    pub raw_start: usize,
    pub raw_end: usize,
    pub normalized_start: usize,
    pub normalized_end: usize,
    pub transform: [f64; 6],
    pub width: f64,
    pub height: f64,
    pub has_eol: bool,
    pub bbox: Option<CanonicalTextRect>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CanonicalTextRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A sentence-sized unit derived exclusively from a canonical PDF.js page.
/// Text and offsets use the page's normalized UTF-16 coordinate space.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CanonicalReadingUnit {
    pub sentence_id: String,
    pub page: usize,
    pub text: String,
    pub normalized_text: String,
    pub norm_start: usize,
    pub norm_end: usize,
    pub start_item_id: String,
    pub end_item_id: String,
    pub item_ranges: Vec<CanonicalItemRange>,
    pub quote_selector: TextQuoteSelector,
    pub candidate_kind: CandidateKind,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateKind {
    BodySentence,
    Heading,
    Metadata,
    Reference,
    FormulaOrTable,
    ShortFragment,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CanonicalItemRange {
    pub item_id: String,
    pub normalized_start: usize,
    pub normalized_end: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TextQuoteSelector {
    pub exact: String,
    pub prefix: String,
    pub suffix: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocumentSession {
    pub id: DocumentId,
    pub filename: String,
    pub uploaded_at: DateTime<Utc>,
    pub page_count: usize,
    pub pdf_bytes: Vec<u8>,
    pub chunks: Vec<TextChunk>,
    pub sentence_candidates: Vec<SentenceCandidate>,
    pub canonical_text_model: Option<CanonicalTextModel>,
    pub canonical_sentence_candidates: Vec<CanonicalReadingUnit>,
    pub concepts: Vec<ConceptTag>,
    pub baseline: Option<UserBaseline>,
    pub quests: Vec<Quest>,
    pub annotations: Vec<ChunkAnnotation>,
    pub reading_anchors: Vec<ReadingAnchor>,
    pub canonical_sentence_annotations: Vec<CanonicalSentenceAnnotation>,
    pub delta_eligibility_diagnostics: crate::delta_policy::DeltaEligibilityDiagnostics,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TextChunk {
    pub id: String,
    pub page: usize,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SentenceCandidate {
    pub sentence_id: String,
    pub chunk_id: String,
    pub page: usize,
    pub text: String,
    pub char_start: usize,
    pub char_end: usize,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Delta,
    Bridge,
    Familiar,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReaderDirective {
    Highlight,
    SoftFade,
    Callout,
    LeaveNormal,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationSource {
    Ai,
    Mock,
    #[default]
    Fallback,
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
    #[serde(default)]
    pub source: AnnotationSource,
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
pub struct ReadingAnchor {
    pub anchor_id: String,
    pub sentence_ids: Vec<String>,
    pub chunk_id: String,
    pub page: usize,
    pub text: String,
    pub char_start: usize,
    pub char_end: usize,
    pub source: AnnotationSource,
    pub priority: Priority,
    pub directive: ReaderDirective,
    pub confidence: f32,
    pub rationale: String,
    pub reader_label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CanonicalSentenceAnnotation {
    pub annotation_id: String,
    pub sentence_id: String,
    pub page: usize,
    pub directive: ReaderDirective,
    pub priority: Priority,
    pub confidence: f32,
    pub source: AnnotationSource,
    pub source_text: String,
    pub item_ranges: Vec<CanonicalItemRange>,
    pub quote_selector: TextQuoteSelector,
    pub candidate_kind: CandidateKind,
    pub rationale: String,
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
    pub reading_anchors: Vec<ReadingAnchor>,
    pub canonical_sentence_annotations: Vec<CanonicalSentenceAnnotation>,
    pub delta_eligibility_diagnostics: crate::delta_policy::DeltaEligibilityDiagnostics,
}

#[derive(Debug, Serialize)]
pub struct ReaderResponse {
    pub document_id: DocumentId,
    pub filename: String,
    pub page_count: usize,
    pub chunks: Vec<TextChunk>,
    pub sentence_candidates: Vec<SentenceCandidate>,
    pub canonical_text_model_available: bool,
    pub concepts: Vec<ConceptTag>,
    pub baseline: Option<UserBaseline>,
    pub quests: Vec<Quest>,
    pub chunk_annotations: Vec<ChunkAnnotation>,
    pub reading_anchors: Vec<ReadingAnchor>,
    pub canonical_sentence_annotations: Vec<CanonicalSentenceAnnotation>,
    pub delta_eligibility_diagnostics: crate::delta_policy::DeltaEligibilityDiagnostics,
}
