import type { AnnotationSource, Priority, ReaderDirective, ReaderResponse } from "./types.js";

export type AnnotationDropReason =
  | "below_confidence"
  | "density_cap"
  | "overlap_conflict"
  | "unmatched_text"
  | "unsupported_directive";

export type AnnotationValidationStatus = "validated" | "invalid";
export type AnnotationEligibilityStatus = "eligible" | "ineligible";
export type AnnotationMatchStatus =
  | "not_attempted"
  | "candidate_match"
  | "usable_match"
  | "unmatched";
export type AnnotationFinalStatus =
  | "rendered_inline"
  | "inline_fallback_without_usable_match"
  | "rendered_as_page_note"
  | "left_normal"
  | "dropped";

interface AnnotationLifecycleBase {
  annotationKind: "canonical_sentence_annotation" | "sentence_anchor" | "legacy_chunk_annotation";
  chunkId: string;
  candidateMatch: boolean;
  canonicalDirectRangeAvailable: boolean;
  confidence: number;
  directive: ReaderDirective;
  directRangeInvalidItem: boolean;
  directRangeMissingSpan: boolean;
  eligibilityStatus: AnnotationEligibilityStatus;
  generated: boolean;
  intendedInline: boolean;
  matchAttempted: boolean;
  matchedCharRatio: number;
  matchedSpanCount: number;
  matchedSpanIndexes: number[];
  matchedTextLength: number;
  matchStatus: AnnotationMatchStatus;
  legacyTextSearchUsed: boolean;
  fuzzyRepairUsed: boolean;
  priority: Priority;
  renderedSpanCount: number;
  renderedByDirectRange: boolean;
  renderedByRectOverlay: boolean;
  source: AnnotationSource;
  sourceTextLength: number;
  usableMatch: boolean;
  validationStatus: AnnotationValidationStatus;
}

export type AnnotationRenderDebug = AnnotationLifecycleBase &
  (
    | {
        finalStatus:
          | "rendered_inline"
          | "inline_fallback_without_usable_match"
          | "rendered_as_page_note"
          | "left_normal";
        primaryDropReason?: never;
      }
    | {
        finalStatus: "dropped";
        primaryDropReason: AnnotationDropReason;
      }
  );

export interface AnnotationLifecycleCounts {
  candidateMatches: number;
  canonicalDirectRangeAvailable: number;
  directRangeInvalidItem: number;
  directRangeMissingSpan: number;
  dropped: number;
  dropReasons: Partial<Record<AnnotationDropReason, number>>;
  eligibleAfterFilters: number;
  generated: number;
  generatedBySource: Record<AnnotationSource, number>;
  invalid: number;
  inlineFallbackWithoutUsableMatch: number;
  intendedInline: number;
  leftNormal: number;
  matchAttempted: number;
  legacyTextSearchUsed: number;
  fuzzyRepairUsed: number;
  renderedAsPageNote: number;
  renderedInline: number;
  renderedByDirectRange: number;
  renderedByRectOverlay: number;
  usableMatches: number;
  validated: number;
}

export interface PageAnnotationDebug {
  annotations: AnnotationRenderDebug[];
  counts: AnnotationLifecycleCounts;
  mode: "filtered" | "focus" | "normal";
  pageNumber: number;
  spanCount: number;
}

export interface CapturedTextLayerPage {
  items: string[];
  lineTops: number[];
  pageNumber: number;
}

export interface AnnotationReaderCapture {
  fixtureType: "captured_reader_state";
  mode: "filtered" | "focus" | "normal";
  name: string;
  pages: CapturedTextLayerPage[];
  reader: ReaderResponse;
  schemaVersion: 1;
}

export const ANNOTATION_DEBUG_ENABLED =
  import.meta.env.DEV && import.meta.env.VITE_ANNOTATION_DEBUG === "true";
