import type { AnnotationReaderCapture } from "../annotationDebug.js";
import { buildCapturedReaderReport, buildFixtureReport, fixtureNames } from "./annotationFixture.js";
import {
  diagnoseAnnotationPage,
  normalizeForMatch,
  type DiagnosticChunk
} from "../annotationDiagnostics.js";

function assert(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

const report = buildFixtureReport();
const annotations = new Map(
  report.pages.flatMap((page) => page.annotations).map((annotation) => [annotation.chunkId, annotation])
);

assert(
  annotations.get("p1-below-confidence")?.finalStatus === "dropped" &&
    annotations.get("p1-below-confidence")?.primaryDropReason === "below_confidence",
  "below-confidence annotation must report below_confidence"
);
assert(
  annotations.get("p1-unmatched")?.finalStatus === "dropped" &&
    annotations.get("p1-unmatched")?.primaryDropReason === "unmatched_text",
  "unmatched annotation must report unmatched_text"
);
assert(
  annotations.get("p2-density-cap")?.finalStatus === "dropped" &&
    annotations.get("p2-density-cap")?.primaryDropReason === "density_cap",
  "oversized line annotation must report density_cap"
);
assert(
  annotations.get("p3-overlap-loser")?.finalStatus === "dropped" &&
    annotations.get("p3-overlap-loser")?.primaryDropReason === "overlap_conflict",
  "lower-ranked duplicate annotation must report overlap_conflict"
);
assert(
  annotations.get("p1-invalid-directive")?.finalStatus === "dropped" &&
    annotations.get("p1-invalid-directive")?.primaryDropReason === "unsupported_directive" &&
    annotations.get("p1-invalid-directive")?.validationStatus === "invalid",
  "unknown directive must report unsupported_directive"
);

const valid = annotations.get("p1-valid-multispan");
assert(
  valid?.matchStatus === "usable_match" && valid.finalStatus === "rendered_inline",
  "valid annotation must match and render inline"
);
assert(valid?.matchedSpanCount === 3, "line-break match must cover three spans");
assert(valid?.matchedCharRatio === 1, "line-break normalization must produce a full match");

assert(
  annotations.get("p1-callout")?.finalStatus === "rendered_as_page_note",
  "callout must render as a page note"
);
assert(
  annotations.get("p1-leave-normal")?.finalStatus === "left_normal" &&
    !("primaryDropReason" in (annotations.get("p1-leave-normal") ?? {})),
  "leave_normal must terminate intentionally without a drop reason"
);

const { summary } = report;
assert(
  summary.generated ===
    summary.renderedInline +
      summary.inlineFallbackWithoutUsableMatch +
      summary.renderedAsPageNote +
      summary.leftNormal +
      summary.dropped,
  "generated must equal all final dispositions"
);
assert(
  summary.candidateMatches <= summary.matchAttempted &&
    summary.usableMatches <= summary.candidateMatches &&
    summary.renderedInline <= summary.usableMatches,
  "match lifecycle counts must be monotonically decreasing"
);
for (const annotation of annotations.values()) {
  const hasReason = "primaryDropReason" in annotation;
  assert(
    annotation.finalStatus !== "dropped" || hasReason,
    `${annotation.chunkId}: dropped annotation requires one primary reason`
  );
  assert(
    annotation.finalStatus === "dropped" || !hasReason,
    `${annotation.chunkId}: rendered annotation cannot have a drop reason`
  );
  assert(
    [
      "rendered_inline",
      "inline_fallback_without_usable_match",
      "rendered_as_page_note",
      "left_normal",
      "dropped"
    ].filter((status) => status === annotation.finalStatus).length === 1,
    `${annotation.chunkId}: annotation must have exactly one final status`
  );
  assert(
    annotation.finalStatus !== "dropped" ||
      annotation.primaryDropReason !== "unsupported_directive" ||
      annotation.validationStatus === "invalid",
    `${annotation.chunkId}: unsupported_directive is reserved for invalid annotations`
  );
}

for (const fixtureName of fixtureNames) {
  const fixtureReport = buildFixtureReport(fixtureName);
  const fixtureSummary = fixtureReport.summary;
  assert(
    fixtureSummary.generated ===
      fixtureSummary.renderedInline +
        fixtureSummary.inlineFallbackWithoutUsableMatch +
        fixtureSummary.renderedAsPageNote +
        fixtureSummary.leftNormal +
        fixtureSummary.dropped,
    `${fixtureName}: final dispositions must equal generated`
  );
  assert(
    fixtureSummary.candidateMatches <= fixtureSummary.matchAttempted,
    `${fixtureName}: candidate matches must not exceed match attempts`
  );
  assert(
    fixtureSummary.usableMatches <= fixtureSummary.candidateMatches,
    `${fixtureName}: usable matches must not exceed candidate matches`
  );
  assert(
    fixtureSummary.renderedInline <= fixtureSummary.usableMatches,
    `${fixtureName}: rendered inline must not exceed usable matches`
  );
  for (const [metric, expected] of Object.entries(fixtureReport.expectedSummary ?? {})) {
    assert(
      fixtureSummary[metric as keyof typeof fixtureReport.expectedSummary] === expected,
      `${fixtureName}: expected ${metric}=${expected}`
    );
  }
}

const multiline = buildFixtureReport("multiline").pages[0].annotations[0];
assert(
  multiline.matchStatus === "usable_match" && multiline.matchedSpanCount === 4,
  "multiline fixture must match one sentence across four PDF.js spans"
);

const hyphenation = buildFixtureReport("hyphenation").pages[0].annotations[0];
assert(
  hyphenation.matchStatus === "usable_match" &&
    hyphenation.finalStatus === "rendered_inline" &&
    hyphenation.matchedSpanCount === 2,
  "hyphenation fixture must match and retain both PDF.js span indexes"
);

assert(
  normalizeForMatch("trans- former") === normalizeForMatch("transformer"),
  "soft line-break hyphens inside one text item must collapse"
);
assert(
  normalizeForMatch("represen- tation") === normalizeForMatch("representation"),
  "soft line-break hyphens must collapse for general words"
);
assert(
  normalizeForMatch("self-attention") !== normalizeForMatch("selfattention"),
  "ordinary semantic hyphens must not collapse words"
);
assert(
  normalizeForMatch("state-of-the-art") === "state of the art",
  "ordinary multi-hyphen terms must retain word boundaries"
);

const dense = buildFixtureReport("real-pdf-dense");
const denseAnnotations = new Map(
  dense.pages.flatMap((page) => page.annotations).map((annotation) => [annotation.chunkId, annotation])
);
assert(dense.pageCount === 3, "dense real-PDF fixture must contain three pages");
assert(dense.summary.generated >= 20, "dense real-PDF fixture must contain at least 20 annotations");
assert(
  new Set(dense.pages.flatMap((page) => page.annotations.map((annotation) => annotation.directive)))
    .size >= 4,
  "dense fixture must cover all directive categories"
);
assert(
  denseAnnotations.get("d-p1-multiline")?.matchedSpanIndexes.join(",") === "0,1",
  "dense multiline annotation must map to both source spans"
);
assert(
  denseAnnotations.get("d-p2-hyphen")?.matchedSpanIndexes.join(",") === "7,8",
  "dense hyphenation annotation must map across its PDF.js item boundary"
);
assert(
  denseAnnotations.get("d-p3-repeat-context")?.matchedSpanIndexes.join(",") === "4,5",
  "contextual repeated phrase must select the later occurrence"
);
assert(
  denseAnnotations.get("d-p2-unmatched")?.finalStatus === "dropped" &&
    denseAnnotations.get("d-p2-unmatched")?.primaryDropReason === "unmatched_text",
  "dense fixture must include an unmatched annotation"
);
assert(
  denseAnnotations.get("d-p2-density")?.finalStatus === "dropped" &&
    denseAnnotations.get("d-p2-density")?.primaryDropReason === "density_cap",
  "dense fixture must include a density-cap rejection"
);
assert(
  denseAnnotations.get("d-p2-overlap-lose")?.finalStatus === "dropped" &&
    denseAnnotations.get("d-p2-overlap-lose")?.primaryDropReason === "overlap_conflict",
  "dense fixture must include an overlap conflict"
);

const capturedReport = buildCapturedReaderReport(
  {
    fixtureType: "captured_reader_state",
    mode: "filtered",
    name: "captured-reader-test.pdf",
    pages: [
      {
        items: [
          "Short fragment",
          "Fallback lexical evidence appears",
          "alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima mike november oscar papa",
          ...Array.from({ length: 17 }, (_, index) => `filler ${index}`)
        ],
        lineTops: Array.from({ length: 20 }, (_, index) => index * 20),
        pageNumber: 1
      }
    ],
    reader: {
      chunk_annotations: [
        {
          chunk_id: "capture-p1c1",
          confidence: 0.95,
          directive: "highlight",
          priority: "delta",
          rationale: "captured",
          reader_label: "Captured",
          source: "ai"
        },
        {
          chunk_id: "capture-p1c3",
          confidence: 0.92,
          directive: "highlight",
          priority: "delta",
          rationale: "captured partial",
          reader_label: "Partial",
          source: "ai"
        },
        {
          chunk_id: "capture-p1c2",
          confidence: 0.9,
          directive: "highlight",
          priority: "delta",
          rationale: "captured fallback",
          reader_label: "Fallback",
          source: "ai"
        }
      ],
      chunks: [
        { id: "capture-p1c1", page: 1, text: "Short fragment" },
        {
          id: "capture-p1c2",
          page: 1,
          text: "Fallback lexical evidence differs completely from annotation wording"
        },
        {
          id: "capture-p1c3",
          page: 1,
          text: "alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima mike november oscar papa quartz rocket silver timber uniform velvet winter xenon yellow zephyr canyon forest garden harbor island jungle kernel lantern meadow nectar orange prairie"
        }
      ],
      concepts: [],
      document_id: "capture-test",
      filename: "captured-reader-test.pdf",
      page_count: 1,
      baseline: null,
      quests: []
    },
    schemaVersion: 1
  } satisfies AnnotationReaderCapture,
  "capture-test.json"
);
assert(capturedReport.summary.renderedInline === 1, "captured reader state must run diagnostics");
assert(
  capturedReport.summary.inlineFallbackWithoutUsableMatch === 2 &&
    capturedReport.pages[0].annotations.find((item) => item.chunkId === "capture-p1c2")
      ?.finalStatus === "inline_fallback_without_usable_match",
  "unmatched lexical fallback must not count as rendered inline"
);
const partialCandidate = capturedReport.pages[0].annotations.find(
  (item) => item.chunkId === "capture-p1c3"
);
assert(
  partialCandidate?.candidateMatch === true &&
    partialCandidate.usableMatch === false &&
    partialCandidate.finalStatus === "inline_fallback_without_usable_match",
  "low-ratio candidate must not count as usable or rendered inline"
);
assert(
  capturedReport.aiOutput.shorterThan30Characters === 1 &&
    capturedReport.aiOutput.notCompleteSentences === 3,
  "captured reader state must report AI output quality metrics"
);

const anchorCaptureReport = buildCapturedReaderReport(
  {
    fixtureType: "captured_reader_state",
    mode: "filtered",
    name: "anchor-capture.pdf",
    pages: [
      {
        items: [
          "This sentence anchor is complete and directly renderable.",
          ...Array.from({ length: 19 }, (_, index) => `anchor filler ${index}`)
        ],
        lineTops: Array.from({ length: 20 }, (_, index) => index * 20),
        pageNumber: 1
      }
    ],
    reader: {
      baseline: null,
      chunk_annotations: [
        {
          chunk_id: "legacy-chunk",
          confidence: 0.9,
          directive: "highlight",
          priority: "delta",
          rationale: "legacy",
          reader_label: "Legacy",
          source: "ai"
        }
      ],
      chunks: [
        {
          id: "legacy-chunk",
          page: 1,
          text: "This sentence anchor is complete and directly renderable. Additional legacy chunk text should not become the render source."
        }
      ],
      concepts: [],
      document_id: "anchor-capture",
      filename: "anchor-capture.pdf",
      page_count: 1,
      quests: [],
      reading_anchors: [
        {
          anchor_id: "a-legacy-chunks1-legacy-chunks1",
          sentence_ids: ["legacy-chunks1"],
          chunk_id: "legacy-chunk",
          page: 1,
          text: "This sentence anchor is complete and directly renderable.",
          char_start: 0,
          char_end: 57,
          source: "ai",
          priority: "delta",
          directive: "highlight",
          confidence: 0.9,
          rationale: "anchor",
          reader_label: "Anchor"
        }
      ],
      sentence_candidates: []
    },
    schemaVersion: 1
  },
  "anchor-capture.json"
);
assert(
  anchorCaptureReport.pages[0].annotations[0].annotationKind === "sentence_anchor" &&
    anchorCaptureReport.aiOutput.annotationKindDistribution.sentence_anchor === 1,
  "captured diagnostics must prefer sentence anchors over legacy chunk annotations"
);
assert(
  anchorCaptureReport.textSourceIntegrity?.summary.intendedInlineFoundInBackendText === 1 &&
    anchorCaptureReport.textSourceIntegrity.summary.intendedInlineFoundInPdfjsPageText === 1 &&
    anchorCaptureReport.textSourceIntegrity.summary.pagesMissingPdfjsTextContent === 0,
  "text-source integrity must detect matching backend and PDF.js text"
);

const canonicalCaptureReport = buildCapturedReaderReport(
  {
    fixtureType: "captured_reader_state",
    mode: "filtered",
    name: "canonical-capture",
    pages: [
      {
        items: ["This canonical sentence is directly grounded in PDF.js text."],
        lineTops: [10],
        pageNumber: 1
      }
    ],
    reader: {
      baseline: null,
      canonical_sentence_annotations: [
        {
          annotation_id: "ca-cp1s1",
          candidate_kind: "body_sentence",
          confidence: 0.92,
          directive: "highlight",
          item_ranges: [
            { item_id: "p1i1", normalized_end: 60, normalized_start: 0 }
          ],
          page: 1,
          priority: "delta",
          quote_selector: {
            exact: "This canonical sentence is directly grounded in PDF.js text.",
            prefix: "",
            suffix: ""
          },
          rationale: "canonical",
          reader_label: "Canonical",
          sentence_id: "cp1s1",
          source: "ai",
          source_text: "This canonical sentence is directly grounded in PDF.js text."
        }
      ],
      canonical_text_model_available: true,
      chunk_annotations: [],
      chunks: [],
      concepts: [],
      document_id: "canonical-capture",
      filename: "canonical-capture.pdf",
      page_count: 1,
      quests: [],
      reading_anchors: [],
      sentence_candidates: []
    },
    schemaVersion: 1
  },
  "canonical-capture.json"
);
assert(
  canonicalCaptureReport.pages[0].annotations[0].annotationKind ===
    "canonical_sentence_annotation" &&
    canonicalCaptureReport.aiOutput.annotationKindDistribution.canonical_sentence_annotation ===
      1 &&
    canonicalCaptureReport.aiOutput.annotationKindDistribution.legacy_chunk_annotation ===
      undefined,
  "captured diagnostics must prefer canonical annotations over legacy data"
);
assert(
  canonicalCaptureReport.aiOutput.missingItemRanges === 0 &&
    canonicalCaptureReport.aiOutput.selectedCandidateKindDistribution.body_sentence === 1,
  "canonical diagnostics must report item-range and candidate-kind quality"
);
const canonicalSingle = canonicalCaptureReport.pages[0].annotations[0];
assert(
  canonicalSingle.finalStatus === "rendered_inline" &&
    canonicalSingle.renderedByDirectRange &&
    !canonicalSingle.legacyTextSearchUsed,
  "single canonical item range must render directly without source-text search"
);

const directItems = [
  "Canonical sentence",
  "spans multiple items.",
  ...Array.from({ length: 18 }, (_, index) => `direct filler ${index}`)
];
const directBase: DiagnosticChunk = {
  annotationGenerated: true,
  annotationKind: "canonical_sentence_annotation",
  confidence: 0.95,
  directive: "highlight",
  id: "ca-direct-multi",
  itemRanges: [
    { item_id: "p1i1", normalized_end: 18, normalized_start: 0 },
    { item_id: "p1i2", normalized_end: 21, normalized_start: 0 }
  ],
  page: 1,
  priority: "delta",
  source: "ai",
  text: "This deliberately does not match the PDF.js source text."
};
const directMulti = diagnoseAnnotationPage({
  chunks: [directBase],
  items: directItems,
  mode: "filtered",
  pageNumber: 1,
  spans: directItems.map((_, index) => ({ offsetTop: index * 20 }))
}).debug.annotations[0];
assert(
  directMulti.finalStatus === "rendered_inline" &&
    directMulti.renderedByDirectRange &&
    directMulti.matchedSpanIndexes.join(",") === "0,1" &&
    !directMulti.legacyTextSearchUsed &&
    !directMulti.fuzzyRepairUsed,
  "multi-item canonical ranges must render directly without searching source_text"
);

const partialRange = diagnoseAnnotationPage({
  chunks: [
    {
      ...directBase,
      id: "ca-partial",
      itemRanges: [{ item_id: "p1i1", normalized_end: 12, normalized_start: 3 }]
    }
  ],
  items: directItems,
  mode: "filtered",
  pageNumber: 1,
  spans: directItems.map((_, index) => ({ offsetTop: index * 20 }))
});
assert(
  partialRange.debug.annotations[0].renderedByRectOverlay &&
    partialRange.rectOverlays.length === 1,
  "partial canonical item offsets must produce a rect overlay instruction"
);

const invalidDirect = diagnoseAnnotationPage({
  chunks: [
    {
      ...directBase,
      id: "ca-invalid",
      itemRanges: [{ item_id: "p1i999", normalized_end: 4, normalized_start: 0 }]
    }
  ],
  items: directItems,
  mode: "filtered",
  pageNumber: 1,
  spans: directItems.map((_, index) => ({ offsetTop: index * 20 }))
}).debug.annotations[0];
assert(
  invalidDirect.directRangeInvalidItem &&
    invalidDirect.finalStatus === "dropped" &&
    invalidDirect.primaryDropReason === "unmatched_text" &&
    !invalidDirect.legacyTextSearchUsed,
  "invalid canonical item IDs must be explicit and must not fall back to text search"
);

const missingDirectSpan = diagnoseAnnotationPage({
  chunks: [directBase],
  items: directItems,
  mode: "filtered",
  pageNumber: 1,
  spans: []
}).debug.annotations[0];
assert(
  missingDirectSpan.directRangeMissingSpan && !missingDirectSpan.directRangeInvalidItem,
  "valid item IDs without rendered spans must report direct_range_missing_span"
);

assert(
  Boolean(valid?.legacyTextSearchUsed && !valid.canonicalDirectRangeAvailable),
  "legacy annotations must retain the old text matcher"
);

console.log(`annotation diagnostics tests: ${fixtureNames.length} fixtures passed`);
