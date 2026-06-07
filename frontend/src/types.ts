export type Priority = "delta" | "bridge" | "familiar";
export type ReaderDirective = "highlight" | "soft_fade" | "callout" | "leave_normal";

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

export interface ChunkAnnotation {
  chunk_id: string;
  priority: Priority;
  directive: ReaderDirective;
  confidence: number;
  rationale: string;
  reader_label: string;
}

export interface Quest {
  id: string;
  question: string;
  anchor_chunk_ids: string[];
}

export interface AnalyzeResponse {
  quests: Quest[];
  chunk_annotations: ChunkAnnotation[];
}

export interface ReaderResponse {
  document_id: string;
  filename: string;
  page_count: number;
  chunks: TextChunk[];
  concepts: ConceptTag[];
  baseline: UserBaseline | null;
  quests: Quest[];
  chunk_annotations: ChunkAnnotation[];
}
