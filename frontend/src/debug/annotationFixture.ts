import baselineFixture from "../../fixtures/annotation-baseline.json" with { type: "json" };
import deterministicFixture from "../../fixtures/annotation-debug.json" with { type: "json" };
import hyphenationFixture from "../../fixtures/annotation-hyphenation.json" with { type: "json" };
import multilineFixture from "../../fixtures/annotation-multiline.json" with { type: "json" };
import realPdfDenseFixture from "../../fixtures/annotation-real-pdf-dense.json" with { type: "json" };
import realPdfFixture from "../../fixtures/annotation-real-pdf.json" with { type: "json" };
import {
  MIN_USABLE_MATCH_RATIO,
  diagnoseAnnotationPage,
  normalizeForMatch,
  normalizeTextItemsForMatch,
  type DiagnosticChunk,
  type StyleMode
} from "../annotationDiagnostics.js";
import type {
  AnnotationLifecycleCounts,
  AnnotationReaderCapture,
  CapturedTextLayerPage,
  PageAnnotationDebug
} from "../annotationDebug.js";
import type {
  AnnotationSource,
  CanonicalSentenceAnnotation,
  ChunkAnnotation,
  Priority,
  ReadingAnchor,
  ReaderDirective,
  TextChunk,
  UserBaseline
} from "../types.js";

type ExpectedSummary = Partial<
  Omit<AnnotationLifecycleCounts, "dropReasons" | "generatedBySource">
>;

interface FixtureAnnotation {
  chunk_id: string;
  confidence: number;
  directive: string;
  priority: Priority;
  source: AnnotationSource;
}

interface FixtureData {
  annotations: FixtureAnnotation[];
  chunks: TextChunk[];
  expectedSummary: ExpectedSummary;
  fixtureType: "synthetic_text_layer" | "captured_pdfjs_text_content";
  mode: StyleMode;
  name: string;
  pages: CapturedTextLayerPage[];
}

export interface AiOutputSummary {
  annotationKindDistribution: Record<string, number>;
  annotationsPerPage: Record<string, number>;
  anchorsLongerThan500Characters: number;
  anchorsShorterThan30Characters: number;
  averageAnchorLength: number;
  averageSourceTextLength: number;
  completeSentenceRatio: number;
  directiveDistribution: Record<string, number>;
  notCompleteSentences: number;
  priorityDistribution: Record<string, number>;
  shorterThan30Characters: number;
  missingItemRanges: number;
  selectedCandidateKindDistribution: Record<string, number>;
}

export type TextSourceRootCause =
  | "no_pdfjs_text_content"
  | "backend_pdfjs_text_mismatch"
  | "page_number_mismatch"
  | "normalization_gap"
  | "true_unmatched";

export interface PageTextSourceStats {
  first120NormalizedCharacters: string;
  hasTextContent: boolean;
  normalizedTextLength: number;
  pageNumber: number;
  rawTextLength: number;
  textItemCount: number;
}

export interface UnmatchedAnchorIntegrity {
  anchorId: string;
  anchorLength: number;
  anchorNormalizedLength: number;
  anchorNormalizedPreview: string;
  bestPageCandidatePreview: string;
  bestSimilarityScore: number;
  existsInBackendChunk: boolean;
  existsInPdfjsPageText: boolean;
  finalDropReason: string | null;
  finalStatus: string;
  page: number;
  possiblePageMatch: number | null;
  rootCause: TextSourceRootCause;
}

export interface TextSourceIntegrityReport {
  pages: PageTextSourceStats[];
  summary: {
    anchorsWithPossiblePageMismatch: number;
    intendedInlineFoundInBackendText: number;
    intendedInlineFoundInPdfjsPageText: number;
    intendedInlineNotFoundAnywhere: number;
    pagesMissingPdfjsTextContent: number;
  };
  unmatchedAnchors: UnmatchedAnchorIntegrity[];
}

const FIXTURES: Record<string, FixtureData> = {
  deterministic: deterministicFixture as FixtureData,
  hyphenation: hyphenationFixture as FixtureData,
  multiline: multilineFixture as FixtureData,
  "real-pdf-dense": realPdfDenseFixture as FixtureData,
  "real-pdf": realPdfFixture as FixtureData
};

export const fixtureNames = Object.keys(FIXTURES).sort();

export interface AnnotationFixtureReport {
  aiOutput: AiOutputSummary;
  baseline: UserBaseline | null;
  expectedSummary?: ExpectedSummary;
  fixture: string;
  fixtureKey: string;
  fixtureType: "synthetic_text_layer" | "captured_pdfjs_text_content" | "captured_reader_state";
  mode: StyleMode;
  minimumUsableMatchRatio: number;
  pageCount: number;
  pages: PageAnnotationDebug[];
  summary: AnnotationLifecycleCounts;
  textSourceIntegrity?: TextSourceIntegrityReport;
}

export function buildFixtureReport(fixtureKey = "deterministic"): AnnotationFixtureReport {
  const fixture = FIXTURES[fixtureKey];
  if (!fixture) {
    throw new Error(`Unknown fixture '${fixtureKey}'. Available: ${fixtureNames.join(", ")}`);
  }
  const report = buildReport({
    annotations: fixture.annotations,
    baseline: baselineFixture as UserBaseline,
    chunks: fixture.chunks,
    expectedSummary: fixture.expectedSummary,
    fixture: fixture.name,
    fixtureKey,
    fixtureType: fixture.fixtureType,
    mode: fixture.mode,
    pages: fixture.pages
  });
  return report;
}

export function buildCapturedReaderReport(
  capture: AnnotationReaderCapture,
  fixtureKey: string
): AnnotationFixtureReport {
  if (capture.schemaVersion !== 1 || capture.fixtureType !== "captured_reader_state") {
    throw new Error("Unsupported annotation reader capture format");
  }
  if (capture.pages.length !== capture.reader.page_count) {
    throw new Error(
      `Incomplete reader capture: expected ${capture.reader.page_count} pages, found ${capture.pages.length}`
    );
  }
  const report = buildReport({
    annotations: capture.reader.chunk_annotations,
    canonicalSentenceAnnotations: capture.reader.canonical_sentence_annotations,
    readingAnchors: capture.reader.reading_anchors,
    baseline: capture.reader.baseline,
    chunks: capture.reader.chunks,
    fixture: capture.name,
    fixtureKey,
    fixtureType: capture.fixtureType,
    mode: capture.mode,
    pages: capture.pages
  });
  return {
    ...report,
    textSourceIntegrity: analyzeTextSourceIntegrity(capture, report)
  };
}

function buildReport(input: {
  annotations: Array<FixtureAnnotation | ChunkAnnotation>;
  canonicalSentenceAnnotations?: CanonicalSentenceAnnotation[];
  readingAnchors?: ReadingAnchor[];
  baseline: UserBaseline | null;
  chunks: TextChunk[];
  expectedSummary?: ExpectedSummary;
  fixture: string;
  fixtureKey: string;
  fixtureType: AnnotationFixtureReport["fixtureType"];
  mode: StyleMode;
  pages: CapturedTextLayerPage[];
}): AnnotationFixtureReport {
  const annotations = new Map(
    input.annotations.map((annotation) => [annotation.chunk_id, annotation])
  );
  const chunks: DiagnosticChunk[] = input.canonicalSentenceAnnotations?.length
    ? input.canonicalSentenceAnnotations.map((annotation) => ({
        annotationGenerated: true,
        annotationKind: "canonical_sentence_annotation",
        confidence: annotation.confidence,
        directive: annotation.directive,
        id: annotation.annotation_id,
        itemRanges: annotation.item_ranges,
        page: annotation.page,
        priority: annotation.priority,
        source: annotation.source,
        text: annotation.source_text
      }))
    : input.readingAnchors?.length
    ? input.readingAnchors.map((anchor) => ({
        annotationGenerated: true,
        annotationKind: "sentence_anchor",
        confidence: anchor.confidence,
        directive: anchor.directive,
        id: anchor.anchor_id,
        page: anchor.page,
        priority: anchor.priority,
        source: anchor.source,
        text: anchor.text
      }))
    : input.chunks.map((chunk): DiagnosticChunk => {
    const annotation = annotations.get(chunk.id);
    if (!annotation) {
      return {
        ...chunk,
        annotationGenerated: false,
        annotationKind: "legacy_chunk_annotation",
        confidence: 0.5,
        directive: "leave_normal",
        priority: "bridge",
        source: "fallback"
      };
    }
    return {
      ...chunk,
      annotationGenerated: true,
      annotationKind: "legacy_chunk_annotation",
      confidence: annotation.confidence,
      directive: annotation.directive as ReaderDirective,
      priority: annotation.priority,
      source: annotation.source ?? "fallback"
    };
    });
  const pages = input.pages
    .slice()
    .sort((a, b) => a.pageNumber - b.pageNumber)
    .map((page) =>
      diagnoseAnnotationPage({
        chunks: chunks.filter((chunk) => chunk.page === page.pageNumber),
        items: page.items,
        mode: input.mode,
        pageNumber: page.pageNumber,
        spans: page.lineTops.map((offsetTop) => ({ offsetTop }))
      }).debug
    );

  return {
    aiOutput: summarizeAiOutput(
      input.chunks,
      input.annotations,
      input.readingAnchors,
      input.canonicalSentenceAnnotations
    ),
    baseline: input.baseline,
    expectedSummary: input.expectedSummary,
    fixture: input.fixture,
    fixtureKey: input.fixtureKey,
    fixtureType: input.fixtureType,
    mode: input.mode,
    minimumUsableMatchRatio: MIN_USABLE_MATCH_RATIO,
    pageCount: pages.length,
    pages,
    summary: summarizePages(pages)
  };
}

function summarizeAiOutput(
  chunks: TextChunk[],
  annotations: Array<FixtureAnnotation | ChunkAnnotation>,
  readingAnchors?: ReadingAnchor[],
  canonicalSentenceAnnotations?: CanonicalSentenceAnnotation[]
): AiOutputSummary {
  const chunkById = new Map(chunks.map((chunk) => [chunk.id, chunk]));
  const units = canonicalSentenceAnnotations?.length
    ? canonicalSentenceAnnotations.map((annotation) => ({
        candidateKind: annotation.candidate_kind,
        directive: annotation.directive,
        kind: "canonical_sentence_annotation",
        page: annotation.page,
        priority: annotation.priority,
        text: annotation.source_text
      }))
    : readingAnchors?.length
    ? readingAnchors.map((anchor) => ({
        candidateKind: undefined,
        directive: anchor.directive,
        kind: "sentence_anchor",
        page: anchor.page,
        priority: anchor.priority,
        text: anchor.text
      }))
    : annotations.map((annotation) => {
        const chunk = chunkById.get(annotation.chunk_id);
        return {
          candidateKind: undefined,
          directive: annotation.directive,
          kind: "legacy_chunk_annotation",
          page: chunk?.page,
          priority: annotation.priority,
          text: chunk?.text ?? ""
        };
      });
  const lengths = units.map((unit) => unit.text.length);
  const complete = units.filter((unit) => isCompleteSentence(unit.text)).length;
  return {
    annotationKindDistribution: distribution(units.map((unit) => unit.kind)),
    annotationsPerPage: units.reduce<Record<string, number>>((counts, unit) => {
      const page = String(unit.page ?? "unknown");
      counts[page] = (counts[page] ?? 0) + 1;
      return counts;
    }, {}),
    anchorsLongerThan500Characters: lengths.filter((length) => length > 500).length,
    anchorsShorterThan30Characters: lengths.filter((length) => length < 30).length,
    averageAnchorLength:
      lengths.length === 0
        ? 0
        : Number((lengths.reduce((total, length) => total + length, 0) / lengths.length).toFixed(1)),
    averageSourceTextLength:
      lengths.length === 0
        ? 0
        : Number((lengths.reduce((total, length) => total + length, 0) / lengths.length).toFixed(1)),
    completeSentenceRatio: lengths.length === 0 ? 0 : Number((complete / lengths.length).toFixed(3)),
    directiveDistribution: distribution(units.map((unit) => unit.directive)),
    notCompleteSentences: units.length - complete,
    priorityDistribution: distribution(units.map((unit) => unit.priority)),
    shorterThan30Characters: lengths.filter((length) => length < 30).length,
    missingItemRanges:
      canonicalSentenceAnnotations?.filter((annotation) => annotation.item_ranges.length === 0)
        .length ?? 0,
    selectedCandidateKindDistribution: distribution(
      units.flatMap((unit) => (unit.candidateKind ? [unit.candidateKind] : []))
    )
  };
}

function isCompleteSentence(text: string): boolean {
  return /[.!?]["')\]]?\s*$/.test(text.trim());
}

function analyzeTextSourceIntegrity(
  capture: AnnotationReaderCapture,
  report: AnnotationFixtureReport
): TextSourceIntegrityReport {
  const normalizedPages = new Map(
    capture.pages.map((page) => [page.pageNumber, normalizeTextItemsForMatch(page.items)])
  );
  const chunkById = new Map(capture.reader.chunks.map((chunk) => [chunk.id, chunk]));
  const anchorsById = new Map(
    (capture.reader.reading_anchors ?? []).map((anchor) => [anchor.anchor_id, anchor])
  );
  const canonicalById = new Map(
    (capture.reader.canonical_sentence_annotations ?? []).map((annotation) => [
      annotation.annotation_id,
      annotation
    ])
  );
  const pageStats = capture.pages.map((page) => {
    const normalized = normalizedPages.get(page.pageNumber) ?? "";
    const rawTextLength = page.items.reduce((total, item) => total + item.length, 0);
    return {
      first120NormalizedCharacters: normalized.slice(0, 120),
      hasTextContent: page.items.some((item) => item.trim().length > 0),
      normalizedTextLength: normalized.length,
      pageNumber: page.pageNumber,
      rawTextLength,
      textItemCount: page.items.length
    };
  });

  let intendedInlineFoundInBackendText = 0;
  let intendedInlineFoundInPdfjsPageText = 0;
  let intendedInlineNotFoundAnywhere = 0;
  let anchorsWithPossiblePageMismatch = 0;
  const unmatchedAnchors: UnmatchedAnchorIntegrity[] = [];

  for (const { annotation, assignedPage } of report.pages.flatMap((page) =>
    page.annotations.map((annotation) => ({ annotation, assignedPage: page.pageNumber }))
  )) {
    if (!annotation.intendedInline) continue;
    const canonical = canonicalById.get(annotation.chunkId);
    const anchor = anchorsById.get(annotation.chunkId);
    const chunk = anchor ? chunkById.get(anchor.chunk_id) : chunkById.get(annotation.chunkId);
    const anchorText = canonical?.source_text ?? anchor?.text ?? chunk?.text ?? "";
    const anchorNormalized = normalizeForMatch(anchorText);
    const pageText = normalizedPages.get(assignedPage) ?? "";
    const existsInBackendChunk = canonical
      ? canonical.quote_selector.exact === canonical.source_text
      : Boolean(chunk && normalizeForMatch(chunk.text).includes(anchorNormalized));
    const existsInPdfjsPageText =
      anchorNormalized.length > 0 && pageText.includes(anchorNormalized);
    if (existsInBackendChunk) intendedInlineFoundInBackendText += 1;
    if (existsInPdfjsPageText) intendedInlineFoundInPdfjsPageText += 1;

    const otherPage = Array.from(normalizedPages.entries()).find(
      ([pageNumber, text]) =>
        pageNumber !== assignedPage &&
        anchorNormalized.length > 0 &&
        text.includes(anchorNormalized)
    )?.[0];
    if (otherPage !== undefined) anchorsWithPossiblePageMismatch += 1;
    if (!existsInBackendChunk && !existsInPdfjsPageText && otherPage === undefined) {
      intendedInlineNotFoundAnywhere += 1;
    }

    if (!annotation.matchAttempted || annotation.candidateMatch) continue;
    const fuzzy = bestFuzzyWindow(anchorNormalized, pageText);
    const pageHasText = pageStats.find((page) => page.pageNumber === assignedPage)
      ?.hasTextContent;
    const compactAnchor = anchorNormalized.replace(/\s+/g, "");
    const compactPage = pageText.replace(/\s+/g, "");
    const rootCause: TextSourceRootCause = !pageHasText
      ? "no_pdfjs_text_content"
      : otherPage !== undefined
        ? "page_number_mismatch"
        : compactAnchor.length > 0 && compactPage.includes(compactAnchor)
          ? "normalization_gap"
          : fuzzy.score >= 0.8
            ? "normalization_gap"
            : existsInBackendChunk
              ? "backend_pdfjs_text_mismatch"
              : "true_unmatched";
    unmatchedAnchors.push({
      anchorId: annotation.chunkId,
      anchorLength: anchorText.length,
      anchorNormalizedLength: anchorNormalized.length,
      anchorNormalizedPreview: anchorNormalized.slice(0, 120),
      bestPageCandidatePreview: fuzzy.text.slice(0, 120),
      bestSimilarityScore: Number(fuzzy.score.toFixed(3)),
      existsInBackendChunk,
      existsInPdfjsPageText,
      finalDropReason:
        annotation.finalStatus === "dropped" ? annotation.primaryDropReason : null,
      finalStatus: annotation.finalStatus,
      page: assignedPage,
      possiblePageMatch: otherPage ?? null,
      rootCause
    });
  }

  return {
    pages: pageStats,
    summary: {
      anchorsWithPossiblePageMismatch,
      intendedInlineFoundInBackendText,
      intendedInlineFoundInPdfjsPageText,
      intendedInlineNotFoundAnywhere,
      pagesMissingPdfjsTextContent: pageStats.filter((page) => !page.hasTextContent).length
    },
    unmatchedAnchors
  };
}

function bestFuzzyWindow(anchor: string, page: string): { score: number; text: string } {
  const anchorWords = anchor.split(" ").filter(Boolean);
  const pageWords = page.split(" ").filter(Boolean);
  if (anchorWords.length === 0 || pageWords.length === 0) return { score: 0, text: "" };
  const windowSize = Math.min(anchorWords.length, pageWords.length);
  let best = { score: 0, text: "" };
  for (let start = 0; start + windowSize <= pageWords.length; start += 1) {
    const window = pageWords.slice(start, start + windowSize);
    const score = tokenDice(anchorWords, window);
    if (score > best.score) best = { score, text: window.join(" ") };
  }
  return best;
}

function tokenDice(left: string[], right: string[]): number {
  const leftSet = new Set(left);
  const rightSet = new Set(right);
  let overlap = 0;
  for (const word of leftSet) if (rightSet.has(word)) overlap += 1;
  return (2 * overlap) / (leftSet.size + rightSet.size);
}

function distribution(values: string[]): Record<string, number> {
  return values.reduce<Record<string, number>>((counts, value) => {
    counts[value] = (counts[value] ?? 0) + 1;
    return counts;
  }, {});
}

function summarizePages(pages: PageAnnotationDebug[]): AnnotationLifecycleCounts {
  return pages.reduce<AnnotationLifecycleCounts>(
    (total, page) => ({
      candidateMatches: total.candidateMatches + page.counts.candidateMatches,
      canonicalDirectRangeAvailable:
        total.canonicalDirectRangeAvailable + page.counts.canonicalDirectRangeAvailable,
      directRangeInvalidItem:
        total.directRangeInvalidItem + page.counts.directRangeInvalidItem,
      directRangeMissingSpan:
        total.directRangeMissingSpan + page.counts.directRangeMissingSpan,
      dropped: total.dropped + page.counts.dropped,
      dropReasons: mergeCounts(total.dropReasons, page.counts.dropReasons),
      eligibleAfterFilters: total.eligibleAfterFilters + page.counts.eligibleAfterFilters,
      generated: total.generated + page.counts.generated,
      generatedBySource: {
        ai: total.generatedBySource.ai + page.counts.generatedBySource.ai,
        fallback: total.generatedBySource.fallback + page.counts.generatedBySource.fallback,
        mock: total.generatedBySource.mock + page.counts.generatedBySource.mock
      },
      invalid: total.invalid + page.counts.invalid,
      inlineFallbackWithoutUsableMatch:
        total.inlineFallbackWithoutUsableMatch + page.counts.inlineFallbackWithoutUsableMatch,
      intendedInline: total.intendedInline + page.counts.intendedInline,
      legacyTextSearchUsed: total.legacyTextSearchUsed + page.counts.legacyTextSearchUsed,
      fuzzyRepairUsed: total.fuzzyRepairUsed + page.counts.fuzzyRepairUsed,
      leftNormal: total.leftNormal + page.counts.leftNormal,
      matchAttempted: total.matchAttempted + page.counts.matchAttempted,
      renderedAsPageNote: total.renderedAsPageNote + page.counts.renderedAsPageNote,
      renderedInline: total.renderedInline + page.counts.renderedInline,
      renderedByDirectRange:
        total.renderedByDirectRange + page.counts.renderedByDirectRange,
      renderedByRectOverlay:
        total.renderedByRectOverlay + page.counts.renderedByRectOverlay,
      usableMatches: total.usableMatches + page.counts.usableMatches,
      validated: total.validated + page.counts.validated
    }),
    {
      candidateMatches: 0,
      canonicalDirectRangeAvailable: 0,
      directRangeInvalidItem: 0,
      directRangeMissingSpan: 0,
      dropped: 0,
      dropReasons: {},
      eligibleAfterFilters: 0,
      generated: 0,
      generatedBySource: { ai: 0, fallback: 0, mock: 0 },
      invalid: 0,
      inlineFallbackWithoutUsableMatch: 0,
      intendedInline: 0,
      legacyTextSearchUsed: 0,
      fuzzyRepairUsed: 0,
      leftNormal: 0,
      matchAttempted: 0,
      renderedAsPageNote: 0,
      renderedInline: 0,
      renderedByDirectRange: 0,
      renderedByRectOverlay: 0,
      usableMatches: 0,
      validated: 0
    }
  );
}

function mergeCounts<T extends string>(
  left: Partial<Record<T, number>>,
  right: Partial<Record<T, number>>
): Partial<Record<T, number>> {
  const merged = { ...left };
  for (const [key, value] of Object.entries(right) as [T, number][]) {
    merged[key] = (merged[key] ?? 0) + value;
  }
  return merged;
}
