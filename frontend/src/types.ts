export type Priority = "delta" | "bridge" | "familiar";
export type ReaderDirective = "highlight" | "soft_fade" | "callout" | "leave_normal";
export type AnnotationSource = "ai" | "mock" | "fallback";
export type CandidateKind =
  | "body_sentence"
  | "heading"
  | "metadata"
  | "reference"
  | "formula_or_table"
  | "short_fragment"
  | "unknown";

export interface UploadResponse {
  document_id: string;
  page_count: number;
  extraction_status: string;
}

export interface ConceptTag {
  id: string;
  label: string;
  description: string;
}

export interface UserBaseline {
  express_text: string;
  mastered_concept_ids: string[];
}

export interface TextChunk {
  id: string;
  page: number;
  text: string;
}

export interface SentenceCandidate {
  sentence_id: string;
  chunk_id: string;
  page: number;
  text: string;
  char_start: number;
  char_end: number;
}

export interface ChunkAnnotation {
  chunk_id: string;
  source: AnnotationSource;
  priority: Priority;
  directive: ReaderDirective;
  confidence: number;
  rationale: string;
  reader_label: string;
}

export interface ReadingAnchor {
  anchor_id: string;
  sentence_ids: string[];
  chunk_id: string;
  page: number;
  text: string;
  char_start: number;
  char_end: number;
  source: AnnotationSource;
  priority: Priority;
  directive: ReaderDirective;
  confidence: number;
  rationale: string;
  reader_label: string;
}

export interface CanonicalItemRange {
  item_id: string;
  normalized_start: number;
  normalized_end: number;
}

export interface TextQuoteSelector {
  exact: string;
  prefix: string;
  suffix: string;
}

export interface CanonicalSentenceAnnotation {
  annotation_id: string;
  sentence_id: string;
  page: number;
  directive: ReaderDirective;
  priority: Priority;
  confidence: number;
  source: AnnotationSource;
  source_text: string;
  item_ranges: CanonicalItemRange[];
  quote_selector: TextQuoteSelector;
  candidate_kind: CandidateKind;
  rationale: string;
  reader_label: string;
}

export interface Quest {
  id: string;
  question: string;
  anchor_chunk_ids: string[];
}

export interface DeltaEligibilityDiagnostics {
  delta_eligibility_checked: number;
  delta_demoted_by_familiar_claim: number;
  delta_kept_due_to_novelty_cue: number;
  interest_overlap_without_novelty: number;
  familiar_claim_overlap_count: number;
}

export interface AnalyzeResponse {
  quests: Quest[];
  chunk_annotations: ChunkAnnotation[];
  reading_anchors?: ReadingAnchor[];
  canonical_sentence_annotations?: CanonicalSentenceAnnotation[];
  delta_eligibility_diagnostics?: DeltaEligibilityDiagnostics;
}

export interface ReaderResponse {
  document_id: string;
  filename: string;
  page_count: number;
  chunks: TextChunk[];
  sentence_candidates?: SentenceCandidate[];
  canonical_text_model_available?: boolean;
  concepts: ConceptTag[];
  baseline: UserBaseline | null;
  quests: Quest[];
  chunk_annotations: ChunkAnnotation[];
  reading_anchors?: ReadingAnchor[];
  canonical_sentence_annotations?: CanonicalSentenceAnnotation[];
  delta_eligibility_diagnostics?: DeltaEligibilityDiagnostics;
}
