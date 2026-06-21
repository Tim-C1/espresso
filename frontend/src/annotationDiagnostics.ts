import type {
  AnnotationDropReason,
  AnnotationRenderDebug,
  PageAnnotationDebug
} from "./annotationDebug.js";
import type {
  AnnotationSource,
  CanonicalItemRange,
  Priority,
  ReaderDirective,
  TextChunk
} from "./types.js";

export type StyleMode = "filtered" | "focus" | "normal";
export type InlineStyle = "delta" | "familiar";

export type DiagnosticChunk = TextChunk & {
  annotationKind: "canonical_sentence_annotation" | "sentence_anchor" | "legacy_chunk_annotation";
  annotationGenerated: boolean;
  confidence: number;
  directive: ReaderDirective;
  priority: Priority;
  source: AnnotationSource;
  itemRanges?: CanonicalItemRange[];
};

export type SpanGeometry = {
  offsetTop: number;
};

type StyleCandidate = {
  chunkId: string;
  confidence: number;
  indexes: number[];
  style: InlineStyle;
  renderMethod: "direct_range" | "legacy_text_search" | "rect_overlay";
};

export type RectOverlayInstruction = {
  end: number;
  index: number;
  start: number;
  style: InlineStyle;
};

type DirectRangeFragment = Omit<RectOverlayInstruction, "style"> & {
  fullItem: boolean;
};

type DirectRangeResolution = {
  available: boolean;
  fragments: DirectRangeFragment[];
  invalidItem: boolean;
  missingSpan: boolean;
};

type EligibilityDecision =
  | { status: "eligible"; target: "inline"; style: InlineStyle }
  | { status: "eligible"; target: "page_note" | "left_normal"; style: null }
  | { reason: AnnotationDropReason; status: "ineligible"; target: null; style: null };

type WorkingAnnotation = Omit<
  AnnotationRenderDebug,
  "finalStatus" | "primaryDropReason"
> & {
  eligibility: EligibilityDecision;
  provisionalDropReason?: AnnotationDropReason;
};

type TextMatch = {
  end: number;
  matchedWords: number;
  start: number;
  type: "exact" | "phrase";
};

export type PageDiagnosticInput = {
  chunks: DiagnosticChunk[];
  items: string[];
  mode: StyleMode;
  pageNumber: number;
  spans: SpanGeometry[];
};

export type PageDiagnosticResult = {
  debug: PageAnnotationDebug;
  rectOverlays: RectOverlayInstruction[];
  spanStyles: Map<number, InlineStyle>;
};

const INLINE_STYLE_RANK: Record<InlineStyle, number> = {
  familiar: 1,
  delta: 2
};

const INLINE_STYLE_LIMITS: Record<
  StyleMode,
  Record<InlineStyle, { maxRatio: number; minConfidence: number }>
> = {
  filtered: {
    delta: { maxRatio: 0.18, minConfidence: 0.68 },
    familiar: { maxRatio: 0.34, minConfidence: 0.7 }
  },
  focus: {
    delta: { maxRatio: 0.1, minConfidence: 0.78 },
    familiar: { maxRatio: 0.22, minConfidence: 0.82 }
  },
  normal: {
    delta: { maxRatio: 0, minConfidence: 1 },
    familiar: { maxRatio: 0, minConfidence: 1 }
  }
};

export const MIN_USABLE_MATCH_RATIO = 0.5;

export function diagnoseAnnotationPage(input: PageDiagnosticInput): PageDiagnosticResult {
  const ranges = buildTextRanges(input.items);
  const lineGroups = groupSpanIndexesByLine(input.spans);
  const candidates: StyleCandidate[] = [];
  const workingAnnotations: WorkingAnnotation[] = [];
  const directFragmentsByChunk = new Map<string, DirectRangeFragment[]>();
  const renderMethodByChunk = new Map<
    string,
    "direct_range" | "legacy_text_search" | "rect_overlay"
  >();

  for (const chunk of input.chunks) {
    const validationStatus = isValidAnnotation(chunk) ? "validated" : "invalid";
    const eligibility = eligibilityForChunk(chunk, input.mode, validationStatus);
    const inlineEligible = eligibility.status === "eligible" && eligibility.target === "inline";
    const canonical = chunk.annotationKind === "canonical_sentence_annotation";
    const directResolution = canonical
      ? resolveCanonicalItemRanges(chunk.itemRanges, input)
      : emptyDirectRangeResolution();
    const legacyTextSearchUsed = !canonical && inlineEligible;
    const matches = legacyTextSearchUsed
        ? findChunkMatches(ranges.fullText, chunk.text)
        : [];
    const matchedIndexes = new Set<number>();
    const directUsable =
      inlineEligible &&
      directResolution.available &&
      !directResolution.invalidItem &&
      !directResolution.missingSpan &&
      directResolution.fragments.length > 0;
    if (directUsable) {
      for (const fragment of directResolution.fragments) matchedIndexes.add(fragment.index);
    } else if (!canonical) {
      for (const match of matches) {
        for (const index of indexesForMatch(ranges, match)) {
          for (const expandedIndex of expandIndexesToWholeLines(
            [index],
            input.spans,
            lineGroups
          )) {
            matchedIndexes.add(expandedIndex);
          }
        }
      }
    }

    const sourceTextLength = normalizeForMatch(chunk.text).length;
    const matchedTextLength = directUsable
      ? sourceTextLength
      : matches.reduce((total, match) => total + match.end - match.start, 0);
    const matchedCharRatio = directUsable
      ? 1
      : sourceTextLength === 0
        ? 0
        : Math.min(1, matchedTextLength / sourceTextLength);
    const safeMatches =
      !canonical && inlineEligible
        ? matches.filter((match) => isReaderSafeMatch(match, chunk, input.mode, eligibility.style))
        : [];
    const candidateMatch = directUsable || matches.length > 0;
    const usableMatch =
      directUsable || (safeMatches.length > 0 && matchedCharRatio >= MIN_USABLE_MATCH_RATIO);
    const annotation: WorkingAnnotation = {
      annotationKind: chunk.annotationKind,
      candidateMatch,
      canonicalDirectRangeAvailable: directResolution.available,
      chunkId: chunk.id,
      confidence: chunk.confidence,
      directive: chunk.directive,
      directRangeInvalidItem: directResolution.invalidItem,
      directRangeMissingSpan: directResolution.missingSpan,
      eligibility,
      eligibilityStatus: eligibility.status,
      fuzzyRepairUsed: matches.some((match) => match.type === "phrase"),
      generated: chunk.annotationGenerated,
      intendedInline:
        validationStatus === "validated" &&
        (chunk.directive === "highlight" || chunk.directive === "soft_fade"),
      legacyTextSearchUsed,
      matchAttempted: inlineEligible,
      matchedCharRatio,
      matchedSpanCount: matchedIndexes.size,
      matchedSpanIndexes: Array.from(matchedIndexes).sort((a, b) => a - b),
      matchedTextLength,
      matchStatus:
        eligibility.status !== "eligible" || eligibility.target !== "inline"
          ? "not_attempted"
          : usableMatch
            ? "usable_match"
            : candidateMatch
              ? "candidate_match"
            : "unmatched",
      priority: chunk.priority,
      renderedByDirectRange: false,
      renderedByRectOverlay: false,
      renderedSpanCount: 0,
      source: chunk.source,
      sourceTextLength,
      usableMatch,
      validationStatus
    };
    workingAnnotations.push(annotation);

    if (!inlineEligible) continue;
    const style = eligibility.style;

    if (directUsable) {
      const renderMethod = directResolution.fragments.some((fragment) => !fragment.fullItem)
        ? "rect_overlay"
        : "direct_range";
      directFragmentsByChunk.set(chunk.id, directResolution.fragments);
      renderMethodByChunk.set(chunk.id, renderMethod);
      candidates.push({
        chunkId: chunk.id,
        confidence: chunk.confidence,
        indexes: Array.from(matchedIndexes),
        renderMethod,
        style,
      });
    } else if (!canonical) {
      for (const match of safeMatches) {
        candidates.push({
          chunkId: chunk.id,
          confidence: chunk.confidence,
          indexes: expandIndexesToWholeLines(
            indexesForMatch(ranges, match),
            input.spans,
            lineGroups
          ),
          renderMethod: "legacy_text_search",
          style,
        });
      }

      if (style === "delta" && matches.length === 0) {
        for (const index of fallbackDeltaLineIndexes(ranges, chunk.text, input.mode)) {
          candidates.push({
            chunkId: chunk.id,
            confidence: chunk.confidence * 0.82,
            indexes: expandIndexesToWholeLines([index], input.spans, lineGroups),
            renderMethod: "legacy_text_search",
            style,
          });
          annotation.fuzzyRepairUsed = true;
        }
      }
      if (candidates.some((candidate) => candidate.chunkId === chunk.id)) {
        renderMethodByChunk.set(chunk.id, "legacy_text_search");
      }
    }

    const chunkCandidates = candidates.filter((candidate) => candidate.chunkId === chunk.id);
    annotation.provisionalDropReason =
      chunkCandidates.length > 0
        ? "density_cap"
        : "unmatched_text";
  }

  const allocation = applyDensityCaps(input.spans.length, candidates, input.mode);
  const spanStyles = new Map<number, InlineStyle>();
  for (const [index, style] of allocation.styles) {
    const owner = allocation.owners.get(index);
    if (owner && renderMethodByChunk.get(owner) !== "rect_overlay") spanStyles.set(index, style);
  }
  const rectOverlays = Array.from(directFragmentsByChunk.entries()).flatMap(
    ([chunkId, fragments]) => {
      if (renderMethodByChunk.get(chunkId) !== "rect_overlay") return [];
      const style = candidates.find((candidate) => candidate.chunkId === chunkId)?.style;
      if (!style) return [];
      return fragments
        .filter((fragment) => allocation.owners.get(fragment.index) === chunkId)
        .map(({ end, index, start }) => ({ end, index, start, style }));
    }
  );
  const annotations = workingAnnotations.map((annotation): AnnotationRenderDebug => {
    const { eligibility, provisionalDropReason, ...lifecycle } = annotation;
    if (eligibility.status === "eligible" && eligibility.target === "page_note") {
      return { ...lifecycle, finalStatus: "rendered_as_page_note" };
    }
    if (eligibility.status === "eligible" && eligibility.target === "left_normal") {
      return { ...lifecycle, finalStatus: "left_normal" };
    }
    if (eligibility.status === "ineligible") {
      return { ...lifecycle, finalStatus: "dropped", primaryDropReason: eligibility.reason };
    }

    const renderedSpanCount = Array.from(allocation.owners.values()).filter(
      (chunkId) => chunkId === annotation.chunkId
    ).length;
    const renderMethod = renderMethodByChunk.get(annotation.chunkId);
    const renderedLifecycle = {
      ...lifecycle,
      renderedByDirectRange: renderedSpanCount > 0 && renderMethod === "direct_range",
      renderedByRectOverlay: renderedSpanCount > 0 && renderMethod === "rect_overlay",
      renderedSpanCount,
    };
    if (renderedSpanCount > 0) {
      return {
        ...renderedLifecycle,
        finalStatus:
          annotation.usableMatch
            ? "rendered_inline"
            : "inline_fallback_without_usable_match"
      };
    }
    if (allocation.densityRejected.has(annotation.chunkId)) {
      return {
        ...renderedLifecycle,
        finalStatus: "dropped",
        primaryDropReason: "density_cap"
      };
    }
    if (allocation.overlapRejected.has(annotation.chunkId)) {
      return {
        ...renderedLifecycle,
        finalStatus: "dropped",
        primaryDropReason: "overlap_conflict"
      };
    }
    return {
      ...renderedLifecycle,
      finalStatus: "dropped",
      primaryDropReason: provisionalDropReason ?? "unmatched_text"
    };
  });

  return {
    debug: buildPageDebug(input.pageNumber, input.mode, input.spans.length, annotations),
    rectOverlays,
    spanStyles,
  };
}

function emptyDirectRangeResolution(): DirectRangeResolution {
  return { available: false, fragments: [], invalidItem: false, missingSpan: false };
}

function resolveCanonicalItemRanges(
  itemRanges: CanonicalItemRange[] | undefined,
  input: PageDiagnosticInput
): DirectRangeResolution {
  if (!itemRanges?.length) return emptyDirectRangeResolution();

  const itemToSpan = new Map<number, number>();
  let spanIndex = 0;
  input.items.forEach((item, itemIndex) => {
    if (item.length > 0) {
      itemToSpan.set(itemIndex, spanIndex);
      spanIndex += 1;
    }
  });

  const fragments: DirectRangeFragment[] = [];
  let invalidItem = false;
  let missingSpan = false;
  for (const itemRange of itemRanges) {
    const parsed = /^p(\d+)i(\d+)$/.exec(itemRange.item_id);
    if (!parsed) {
      invalidItem = true;
      continue;
    }
    const rangePage = Number(parsed[1]);
    const itemIndex = Number(parsed[2]) - 1;
    if (
      rangePage !== input.pageNumber ||
      !Number.isInteger(itemIndex) ||
      itemIndex < 0 ||
      itemIndex >= input.items.length
    ) {
      invalidItem = true;
      continue;
    }
    const mappedSpan = itemToSpan.get(itemIndex);
    if (mappedSpan === undefined || mappedSpan >= input.spans.length) {
      missingSpan = true;
      continue;
    }
    const normalizedLength = normalizeCanonicalItem(input.items[itemIndex]).length;
    const start = itemRange.normalized_start;
    const end = itemRange.normalized_end;
    if (
      !Number.isInteger(start) ||
      !Number.isInteger(end) ||
      start < 0 ||
      end <= start ||
      end > normalizedLength
    ) {
      invalidItem = true;
      continue;
    }
    fragments.push({
      end,
      fullItem: start === 0 && end === normalizedLength,
      index: mappedSpan,
      start,
    });
  }

  return { available: true, fragments, invalidItem, missingSpan };
}

function normalizeCanonicalItem(text: string): string {
  return text
    .normalize("NFKC")
    .replace(/\u00ad/g, "")
    .replace(/\s+/gu, " ")
    .trim();
}

function indexesForMatch(ranges: ReturnType<typeof buildTextRanges>, match: TextMatch): number[] {
  return ranges.items.flatMap((range, index) =>
    range.end <= match.start || range.start >= match.end ? [] : [index]
  );
}

function buildTextRanges(items: string[]) {
  let fullText = "";
  const ranges = items.map((item, index) => {
    const normalized = normalizeForMatch(item);
    if (index > 0 && !isSoftHyphenItemBoundary(items[index - 1], item)) {
      fullText += " ";
    }
    const start = fullText.length;
    fullText += normalized;
    const end = fullText.length;
    return { end, normalized, start };
  });
  return {
    fullText,
    items: ranges
  };
}

export function normalizeTextItemsForMatch(items: string[]): string {
  return buildTextRanges(items).fullText;
}

function isSoftHyphenItemBoundary(left: string, right: string): boolean {
  return /[a-z]-\s*$/i.test(left) && /^\s*[a-z]/i.test(right);
}

function findChunkMatches(pageText: string, chunkText: string): TextMatch[] {
  const normalizedChunk = normalizeForMatch(chunkText);
  if (!normalizedChunk) return [];

  const chunkWords = meaningfulWords(normalizedChunk);
  const exact = pageText.indexOf(normalizedChunk);
  if (exact >= 0) {
    return [{ end: exact + normalizedChunk.length, matchedWords: chunkWords.length, start: exact, type: "exact" }];
  }

  const matches: TextMatch[] = [];
  const seen = new Set<string>();
  for (const size of [24, 20, 16, 12]) {
    for (
      let index = 0;
      index + size <= chunkWords.length;
      index += Math.max(1, Math.floor(size / 2))
    ) {
      const phrase = chunkWords.slice(index, index + size).join(" ");
      const matchStart = pageText.indexOf(phrase);
      if (matchStart < 0) continue;
      const match: TextMatch = {
        end: matchStart + phrase.length,
        matchedWords: size,
        start: matchStart,
        type: "phrase"
      };
      const key = `${match.start}:${match.end}`;
      if (!seen.has(key) && !overlapsExisting(matches, match)) {
        seen.add(key);
        matches.push(match);
      }
    }
    if (matches.length >= 4) break;
  }
  return matches.sort((a, b) => a.start - b.start);
}

function overlapsExisting(matches: TextMatch[], candidate: { start: number; end: number }) {
  return matches.some((match) => candidate.start < match.end && candidate.end > match.start);
}

function isValidAnnotation(chunk: DiagnosticChunk): boolean {
  return (
    chunk.annotationGenerated &&
    Number.isFinite(chunk.confidence) &&
    chunk.confidence >= 0 &&
    chunk.confidence <= 1 &&
    ["delta", "bridge", "familiar"].includes(chunk.priority) &&
    ["highlight", "soft_fade", "callout", "leave_normal"].includes(chunk.directive)
  );
}

function eligibilityForChunk(
  chunk: DiagnosticChunk,
  mode: StyleMode,
  validationStatus: "validated" | "invalid"
): EligibilityDecision {
  if (validationStatus === "invalid") {
    return { reason: "unsupported_directive", status: "ineligible", style: null, target: null };
  }
  if (mode === "normal" || chunk.directive === "leave_normal") {
    return { status: "eligible", style: null, target: "left_normal" };
  }
  if (chunk.directive === "callout" && mode === "filtered") {
    return { status: "eligible", style: null, target: "page_note" };
  }
  if (chunk.directive === "callout") {
    return { status: "eligible", style: null, target: "left_normal" };
  }
  if (chunk.directive === "highlight") {
    if (chunk.priority !== "delta") {
      return { reason: "unsupported_directive", status: "ineligible", style: null, target: null };
    }
    if (chunk.confidence < INLINE_STYLE_LIMITS[mode].delta.minConfidence) {
      return { reason: "below_confidence", status: "ineligible", style: null, target: null };
    }
    return { status: "eligible", style: "delta", target: "inline" };
  }
  if (chunk.directive === "soft_fade") {
    if (chunk.priority !== "familiar") {
      return { reason: "unsupported_directive", status: "ineligible", style: null, target: null };
    }
    if (chunk.confidence < INLINE_STYLE_LIMITS[mode].familiar.minConfidence) {
      return { reason: "below_confidence", status: "ineligible", style: null, target: null };
    }
    return { status: "eligible", style: "familiar", target: "inline" };
  }
  return { reason: "unsupported_directive", status: "ineligible", style: null, target: null };
}

function isReaderSafeMatch(
  match: TextMatch,
  chunk: DiagnosticChunk,
  mode: StyleMode,
  style: InlineStyle
) {
  if (style === "delta") {
    const minimumWords = mode === "focus" ? 20 : Math.max(12, Math.round(20 - chunk.confidence * 8));
    return match.type === "exact" || match.matchedWords >= minimumWords;
  }
  const minimumWords = mode === "focus" ? 24 : 18;
  return match.type === "exact" || match.matchedWords >= minimumWords;
}

function fallbackDeltaLineIndexes(
  ranges: ReturnType<typeof buildTextRanges>,
  chunkText: string,
  mode: StyleMode
) {
  const chunkWords = new Set(meaningfulWords(normalizeForMatch(chunkText)));
  if (chunkWords.size === 0) return [];
  return ranges.items
    .map((range, index) => {
      const words = meaningfulWords(range.normalized);
      const hits = words.filter((word) => chunkWords.has(word)).length;
      return { index, score: words.length === 0 ? 0 : hits / words.length, words: words.length };
    })
    .filter((item) => item.words >= 3 && item.score >= (mode === "focus" ? 0.55 : 0.38))
    .sort((a, b) => b.score - a.score)
    .slice(0, mode === "focus" ? 3 : 6)
    .map((item) => item.index);
}

function groupSpanIndexesByLine(spans: SpanGeometry[]) {
  const lineByTop = new Map<number, number[]>();
  spans.forEach((span, index) => {
    const top = Math.round(span.offsetTop / 4) * 4;
    const line = lineByTop.get(top) ?? [];
    line.push(index);
    lineByTop.set(top, line);
  });
  return lineByTop;
}

function expandIndexesToWholeLines(
  indexes: number[],
  spans: SpanGeometry[],
  lineGroups: Map<number, number[]>
) {
  const expanded = new Set<number>();
  for (const index of indexes) {
    const span = spans[index];
    if (!span) continue;
    const top = Math.round(span.offsetTop / 4) * 4;
    for (const lineIndex of lineGroups.get(top) ?? [index]) expanded.add(lineIndex);
  }
  return Array.from(expanded);
}

function applyDensityCaps(spanCount: number, candidates: StyleCandidate[], mode: StyleMode) {
  const styles = new Map<number, InlineStyle>();
  const owners = new Map<number, string>();
  const densityRejected = new Set<string>();
  const overlapRejected = new Set<string>();
  const sorted = candidates
    .filter((candidate) => candidate.indexes.length > 0)
    .sort(
      (a, b) =>
        INLINE_STYLE_RANK[b.style] - INLINE_STYLE_RANK[a.style] ||
        b.confidence - a.confidence ||
        b.indexes.length - a.indexes.length
    );

  for (const style of ["delta", "familiar"] as const) {
    const maxStyledSpans = Math.max(
      1,
      Math.floor(spanCount * INLINE_STYLE_LIMITS[mode][style].maxRatio)
    );
    let styledSpans = 0;
    for (const candidate of sorted.filter((item) => item.style === style)) {
      if (styledSpans >= maxStyledSpans) {
        densityRejected.add(candidate.chunkId);
        continue;
      }
      const openIndexes = candidate.indexes.filter((index) => {
        const current = styles.get(index);
        return !current || INLINE_STYLE_RANK[style] > INLINE_STYLE_RANK[current];
      });
      if (openIndexes.length === 0) {
        overlapRejected.add(candidate.chunkId);
        continue;
      }
      if (styledSpans + openIndexes.length > maxStyledSpans) {
        densityRejected.add(candidate.chunkId);
        continue;
      }
      for (const index of openIndexes) {
        styles.set(index, style);
        owners.set(index, candidate.chunkId);
      }
      styledSpans += openIndexes.length;
    }
  }
  return { densityRejected, overlapRejected, owners, styles };
}

function buildPageDebug(
  pageNumber: number,
  mode: StyleMode,
  spanCount: number,
  annotations: AnnotationRenderDebug[]
): PageAnnotationDebug {
  const generated = annotations.filter((annotation) => annotation.generated);
  const dropped = generated.filter((annotation) => annotation.finalStatus === "dropped");
  const debug: PageAnnotationDebug = {
    annotations: annotations.slice().sort((a, b) => a.chunkId.localeCompare(b.chunkId)),
    counts: {
      candidateMatches: generated.filter((annotation) => annotation.candidateMatch).length,
      canonicalDirectRangeAvailable: generated.filter(
        (annotation) => annotation.canonicalDirectRangeAvailable
      ).length,
      directRangeInvalidItem: generated.filter((annotation) => annotation.directRangeInvalidItem)
        .length,
      directRangeMissingSpan: generated.filter((annotation) => annotation.directRangeMissingSpan)
        .length,
      dropped: dropped.length,
      dropReasons: dropped.reduce<Partial<Record<AnnotationDropReason, number>>>(
        (counts, annotation) => {
          counts[annotation.primaryDropReason] =
            (counts[annotation.primaryDropReason] ?? 0) + 1;
          return counts;
        },
        {}
      ),
      eligibleAfterFilters: generated.filter(
        (annotation) => annotation.eligibilityStatus === "eligible"
      ).length,
      generated: generated.length,
      generatedBySource: generated.reduce<Record<AnnotationSource, number>>(
        (counts, annotation) => {
          counts[annotation.source] += 1;
          return counts;
        },
        { ai: 0, fallback: 0, mock: 0 }
      ),
      invalid: generated.filter((annotation) => annotation.validationStatus === "invalid").length,
      inlineFallbackWithoutUsableMatch: generated.filter(
        (annotation) => annotation.finalStatus === "inline_fallback_without_usable_match"
      ).length,
      intendedInline: generated.filter((annotation) => annotation.intendedInline).length,
      legacyTextSearchUsed: generated.filter((annotation) => annotation.legacyTextSearchUsed)
        .length,
      fuzzyRepairUsed: generated.filter((annotation) => annotation.fuzzyRepairUsed).length,
      leftNormal: generated.filter((annotation) => annotation.finalStatus === "left_normal").length,
      matchAttempted: generated.filter((annotation) => annotation.matchAttempted).length,
      renderedAsPageNote: generated.filter(
        (annotation) => annotation.finalStatus === "rendered_as_page_note"
      ).length,
      renderedInline: generated.filter((annotation) => annotation.finalStatus === "rendered_inline")
        .length,
      renderedByDirectRange: generated.filter((annotation) => annotation.renderedByDirectRange)
        .length,
      renderedByRectOverlay: generated.filter((annotation) => annotation.renderedByRectOverlay)
        .length,
      usableMatches: generated.filter((annotation) => annotation.usableMatch).length,
      validated: generated.filter((annotation) => annotation.validationStatus === "validated").length
    },
    mode,
    pageNumber,
    spanCount
  };
  assertPageLifecycleConsistency(debug);
  return debug;
}

export function assertPageLifecycleConsistency(debug: PageAnnotationDebug): void {
  const { counts } = debug;
  if (
    counts.generated !==
    counts.renderedInline +
      counts.inlineFallbackWithoutUsableMatch +
      counts.renderedAsPageNote +
      counts.leftNormal +
      counts.dropped
  ) {
    throw new Error(`Page ${debug.pageNumber}: final annotation counts do not equal generated`);
  }
  if (counts.candidateMatches > counts.matchAttempted) {
    throw new Error(`Page ${debug.pageNumber}: candidate matches exceed match attempts`);
  }
  if (counts.usableMatches > counts.candidateMatches) {
    throw new Error(`Page ${debug.pageNumber}: usable matches exceed candidate matches`);
  }
  if (counts.renderedInline > counts.usableMatches) {
    throw new Error(`Page ${debug.pageNumber}: rendered inline exceeds usable matches`);
  }
  for (const annotation of debug.annotations.filter((item) => item.generated)) {
    const chunkId = annotation.chunkId;
    const hasReason = "primaryDropReason" in annotation;
    if (annotation.finalStatus === "dropped" && !hasReason) {
      throw new Error(`${chunkId}: dropped annotation has no primary drop reason`);
    }
    if (annotation.finalStatus !== "dropped" && hasReason) {
      throw new Error(`${chunkId}: rendered annotation has a primary drop reason`);
    }
    if (annotation.finalStatus === "rendered_inline" && !annotation.usableMatch) {
      throw new Error(`${chunkId}: rendered inline annotation had no usable match`);
    }
    if (
      annotation.annotationKind === "canonical_sentence_annotation" &&
      annotation.finalStatus === "rendered_inline" &&
      !annotation.renderedByDirectRange &&
      !annotation.renderedByRectOverlay
    ) {
      throw new Error(`${chunkId}: canonical inline annotation did not use a direct render path`);
    }
    if (
      annotation.annotationKind === "canonical_sentence_annotation" &&
      annotation.legacyTextSearchUsed
    ) {
      throw new Error(`${chunkId}: canonical annotation used legacy source-text search`);
    }
    if (
      annotation.finalStatus === "inline_fallback_without_usable_match" &&
      annotation.usableMatch
    ) {
      throw new Error(`${chunkId}: inline fallback annotation had a usable match`);
    }
  }
}

export function normalizeForMatch(text: string) {
  return text
    .toLowerCase()
    .replace(/([a-z])-\s+(?=[a-z])/g, "$1")
    .replace(/[^a-z0-9]+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

function meaningfulWords(text: string) {
  return text.split(" ").filter((word) => word.length > 2);
}
