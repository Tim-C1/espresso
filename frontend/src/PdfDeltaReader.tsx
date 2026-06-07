import { useEffect, useMemo, useRef, useState } from "react";
import * as pdfjsLib from "pdfjs-dist";
import pdfWorkerUrl from "pdfjs-dist/build/pdf.worker.mjs?url";
import type {
  ChunkAnnotation,
  Priority,
  ReaderDirective,
  ReaderResponse,
  TextChunk
} from "./types";

pdfjsLib.GlobalWorkerOptions.workerSrc = pdfWorkerUrl;

type StyleMode = "filtered" | "focus" | "normal";

type PageChunk = TextChunk & {
  confidence: number;
  directive: ReaderDirective;
  priority: Priority;
  rationale: string;
  readerLabel: string;
};

type PageRenderState = {
  pageNumber: number;
  pdf: pdfjsLib.PDFDocumentProxy;
  chunks: PageChunk[];
  mode: StyleMode;
  scale: number;
};

const PRIORITY_RANK: Record<Priority, number> = {
  familiar: 1,
  bridge: 2,
  delta: 3
};

const DIRECTIVE_RANK: Record<ReaderDirective, number> = {
  leave_normal: 0,
  callout: 1,
  soft_fade: 2,
  highlight: 3
};

export default function PdfDeltaReader({
  pdfSource,
  reader
}: {
  pdfSource: string;
  reader: ReaderResponse;
}) {
  const [mode, setMode] = useState<StyleMode>("filtered");
  const [currentPage, setCurrentPage] = useState(1);
  const [scale, setScale] = useState(1.35);
  const [pdf, setPdf] = useState<pdfjsLib.PDFDocumentProxy | null>(null);
  const [error, setError] = useState<string | null>(null);
  const pageRefs = useRef<Map<number, HTMLDivElement>>(new Map());

  const chunksByPage = useMemo(() => groupChunksByPage(reader), [reader]);
  const annotationCounts = useMemo(() => countPriorities(reader), [reader]);

  useEffect(() => {
    let cancelled = false;

    async function loadPdf() {
      setError(null);
      try {
        const loadedPdf = await pdfjsLib.getDocument(pdfSource).promise;
        if (!cancelled) {
          setPdf(loadedPdf);
        }
      } catch (err) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : "Could not load PDF");
        }
      }
    }

    loadPdf();
    return () => {
      cancelled = true;
    };
  }, [pdfSource]);

  useEffect(() => {
    const observer = new IntersectionObserver(
      (entries) => {
        const visible = entries
          .filter((entry) => entry.isIntersecting)
          .map((entry) => Number((entry.target as HTMLElement).dataset.pageNumber))
          .filter(Number.isFinite)
          .sort((a, b) => a - b);
        if (visible.length > 0) {
          setCurrentPage(visible[0]);
        }
      },
      { rootMargin: "-40% 0px -50% 0px", threshold: 0.01 }
    );

    for (const element of pageRefs.current.values()) {
      observer.observe(element);
    }

    return () => observer.disconnect();
  }, [pdf]);

  function scrollToPage(page: number) {
    const bounded = Math.min(Math.max(page, 1), reader.page_count);
    pageRefs.current.get(bounded)?.scrollIntoView({ behavior: "smooth", block: "start" });
  }

  return (
    <section className="pdf-reader-shell">
      <header className="pdf-reader-toolbar">
        <div>
          <strong>{reader.filename}</strong>
          <span>
            Page {currentPage} of {reader.page_count}
          </span>
        </div>
        <div className="toolbar-actions">
          <button disabled={currentPage <= 1} onClick={() => scrollToPage(currentPage - 1)}>
            Previous
          </button>
          <button
            disabled={currentPage >= reader.page_count}
            onClick={() => scrollToPage(currentPage + 1)}
          >
            Next
          </button>
          <button onClick={() => setScale((value) => Math.max(0.85, value - 0.15))}>
            Zoom -
          </button>
          <span className="zoom-label">{Math.round((scale / 1.35) * 100)}%</span>
          <button onClick={() => setScale((value) => Math.min(2.1, value + 0.15))}>
            Zoom +
          </button>
          <button
            className={mode === "filtered" ? "active" : ""}
            onClick={() => setMode("filtered")}
          >
            Filtered
          </button>
          <button className={mode === "focus" ? "active" : ""} onClick={() => setMode("focus")}>
            Focus
          </button>
          <button className={mode === "normal" ? "active" : ""} onClick={() => setMode("normal")}>
            Normal PDF
          </button>
        </div>
      </header>

      <div className="filter-legend">
        <span>
          <i className="legend-swatch delta" /> Delta {annotationCounts.delta}
        </span>
        <span>
          <i className="legend-swatch bridge" /> Bridge {annotationCounts.bridge}
        </span>
        <span>
          <i className="legend-swatch familiar" /> Familiar {annotationCounts.familiar}
        </span>
        <span className="legend-note">
          Filtered highlights confident delta lines. Focus shows only strongest delta cues. Normal
          removes AI overlays.
        </span>
      </div>

      <div className="quest-strip">
        <strong>Pre-reading quests</strong>
        {reader.quests.map((quest) => (
          <button
            key={quest.id}
            onClick={() => {
              const firstAnchor = quest.anchor_chunk_ids[0];
              const anchor = reader.chunks.find((chunk) => chunk.id === firstAnchor);
              if (anchor) scrollToPage(anchor.page);
            }}
            type="button"
          >
            {quest.question}
          </button>
        ))}
      </div>

      {error && <div className="error">{error}</div>}

      <div className="pdf-scroll">
        {pdf &&
          Array.from({ length: reader.page_count }, (_, index) => {
            const pageNumber = index + 1;
            return (
              <div
                className="pdf-page-frame"
                data-page-number={pageNumber}
                key={pageNumber}
                ref={(element) => {
                  if (element) pageRefs.current.set(pageNumber, element);
                  else pageRefs.current.delete(pageNumber);
                }}
              >
                <LazyPdfPage
                  chunks={chunksByPage.get(pageNumber) ?? []}
                  mode={mode}
                  pageNumber={pageNumber}
                  pdf={pdf}
                  scale={scale}
                />
              </div>
            );
          })}
      </div>
    </section>
  );
}

function LazyPdfPage({ pageNumber, pdf, chunks, mode, scale }: PageRenderState) {
  const wrapperRef = useRef<HTMLDivElement | null>(null);
  const [visible, setVisible] = useState(pageNumber <= 2);

  useEffect(() => {
    if (!wrapperRef.current || visible) return;

    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry.isIntersecting) {
          setVisible(true);
          observer.disconnect();
        }
      },
      { rootMargin: "900px 0px" }
    );

    observer.observe(wrapperRef.current);
    return () => observer.disconnect();
  }, [visible]);

  return (
    <div className="pdf-page-lazy" ref={wrapperRef}>
      {visible ? (
        <PdfPage chunks={chunks} mode={mode} pageNumber={pageNumber} pdf={pdf} scale={scale} />
      ) : (
        <div className="pdf-page-placeholder">Page {pageNumber}</div>
      )}
    </div>
  );
}

function PdfPage({ pageNumber, pdf, chunks, mode, scale }: PageRenderState) {
  const pageRef = useRef<HTMLDivElement | null>(null);
  const canvasSlotRef = useRef<HTMLDivElement | null>(null);
  const textLayerRef = useRef<HTMLDivElement | null>(null);
  const textItemsRef = useRef<unknown[]>([]);
  const styleStateRef = useRef({ chunks, mode });
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    styleStateRef.current = { chunks, mode };
  }, [chunks, mode]);

  useEffect(() => {
    let cancelled = false;
    let textLayer: { cancel: () => void; render: () => Promise<unknown> } | null = null;
    let renderTask: { cancel: () => void; promise: Promise<unknown> } | null = null;

    async function renderPage() {
      if (!pageRef.current || !canvasSlotRef.current || !textLayerRef.current) return;

      setError(null);
      textLayerRef.current.innerHTML = "";

      try {
        const page = await pdf.getPage(pageNumber);
        const viewport = page.getViewport({ scale });
        const canvas = document.createElement("canvas");
        const context = canvas.getContext("2d");
        if (!context) return;

        pageRef.current.style.width = `${viewport.width}px`;
        pageRef.current.style.height = `${viewport.height}px`;
        canvas.width = viewport.width;
        canvas.height = viewport.height;
        canvas.className = "pdf-page-canvas";

        renderTask = page.render({ canvasContext: context, viewport });
        await renderTask.promise;
        if (cancelled || !canvasSlotRef.current || !textLayerRef.current) return;

        canvasSlotRef.current.replaceChildren(canvas);

        const textContent = await page.getTextContent();
        textItemsRef.current = textContent.items;
        textLayer = new pdfjsLib.TextLayer({
          container: textLayerRef.current,
          textContentSource: textContent,
          viewport
        });
        await textLayer.render();

        if (!cancelled) {
          syncDeltaStyles(
            textLayerRef.current,
            textItemsRef.current,
            styleStateRef.current.chunks,
            styleStateRef.current.mode
          );
        }
      } catch (err) {
        if (cancelled) return;
        if (!cancelled) {
          setError(err instanceof Error ? err.message : `Could not render page ${pageNumber}`);
        }
      }
    }

    renderPage();
    return () => {
      cancelled = true;
      renderTask?.cancel();
      textLayer?.cancel();
    };
  }, [pageNumber, pdf, scale]);

  useEffect(() => {
    if (!textLayerRef.current || textItemsRef.current.length === 0) return;
    syncDeltaStyles(textLayerRef.current, textItemsRef.current, chunks, mode);
  }, [chunks, mode]);

  return (
    <>
      {error && <div className="error">{error}</div>}
      <div className="pdf-page-view" ref={pageRef}>
        <div className="pdf-canvas-slot" ref={canvasSlotRef} />
        <div className="textLayer delta-text-layer" ref={textLayerRef} />
        {mode !== "normal" && <PageGuidance chunks={chunks} mode={mode} />}
      </div>
    </>
  );
}

function PageGuidance({ chunks, mode }: { chunks: PageChunk[]; mode: StyleMode }) {
  const visibleChunks = chunks.filter((chunk) => {
    if (chunk.directive === "leave_normal") return false;
    if (mode === "focus") return chunk.directive === "highlight" && chunk.confidence >= 0.65;
    return chunk.directive === "highlight" || chunk.directive === "callout";
  });
  const counts = visibleChunks.reduce(
    (acc, chunk) => {
      acc[chunk.directive] += 1;
      return acc;
    },
    { callout: 0, highlight: 0, leave_normal: 0, soft_fade: 0 }
  );
  const topCue = visibleChunks
    .slice()
    .sort((a, b) => b.confidence - a.confidence)
    .find((chunk) => chunk.directive === "highlight" || chunk.directive === "callout");

  if (visibleChunks.length === 0) return null;

  return (
    <aside className="page-guidance">
      <div className="page-guidance-counts">
        {counts.highlight > 0 && <span className="delta">Highlight {counts.highlight}</span>}
        {mode === "filtered" && counts.callout > 0 && (
          <span className="bridge">Callout {counts.callout}</span>
        )}
      </div>
      {topCue && (
        <p title={topCue.rationale}>
          <strong>{topCue.readerLabel}</strong>
          {topCue.rationale ? `: ${topCue.rationale}` : ""}
        </p>
      )}
    </aside>
  );
}

function groupChunksByPage(reader: ReaderResponse): Map<number, PageChunk[]> {
  const annotations = new Map<string, ChunkAnnotation>(
    reader.chunk_annotations.map((annotation) => [annotation.chunk_id, annotation])
  );
  const byPage = new Map<number, PageChunk[]>();

  for (const chunk of reader.chunks) {
    const annotation = annotations.get(chunk.id);
    const pageChunks = byPage.get(chunk.page) ?? [];
    pageChunks.push({
      ...chunk,
      confidence: normalizeConfidence(annotation?.confidence),
      directive: annotation?.directive ?? directiveForPriority(annotation?.priority ?? "bridge"),
      priority: annotation?.priority ?? "bridge",
      rationale: annotation?.rationale ?? "",
      readerLabel: annotation?.reader_label ?? labelForPriority(annotation?.priority ?? "bridge")
    });
    byPage.set(chunk.page, pageChunks);
  }

  return byPage;
}

function countPriorities(reader: ReaderResponse): Record<Priority, number> {
  return reader.chunk_annotations.reduce(
    (counts, annotation) => {
      counts[annotation.priority] += 1;
      return counts;
    },
    { bridge: 0, delta: 0, familiar: 0 }
  );
}

function syncDeltaStyles(
  layer: HTMLElement,
  rawItems: unknown[],
  chunks: PageChunk[],
  mode: StyleMode
) {
  const spans = Array.from(layer.querySelectorAll<HTMLElement>("span"));
  clearDeltaStyles(spans);

  if (mode === "normal") return;

  const items = rawItems.map((item) => {
    if (item && typeof item === "object" && "str" in item) {
      return String((item as { str: unknown }).str ?? "");
    }
    return "";
  });
  const ranges = buildTextRanges(items);
  const spanPriorities = new Map<number, Priority>();

  for (const chunk of chunks) {
    if (!shouldStyleChunk(chunk, mode)) continue;

    const matches = findChunkMatches(ranges.fullText, chunk.text);
    for (const match of matches) {
      if (!isReaderSafeMatch(match, chunk, mode)) continue;

      ranges.items.forEach((range, index) => {
        if (range.end <= match.start || range.start >= match.end) return;
        const current = spanPriorities.get(index);
        if (!current || PRIORITY_RANK[chunk.priority] > PRIORITY_RANK[current]) {
          spanPriorities.set(index, chunk.priority);
        }
      });
    }

    if (chunk.directive === "highlight" && matches.length === 0) {
      for (const index of fallbackDeltaLineIndexes(ranges, chunk.text, mode)) {
        spanPriorities.set(index, "delta");
      }
    }
  }

  const expandedPriorities = expandToWholeLines(spans, spanPriorities);
  spans.forEach((span, index) => {
    const priority = expandedPriorities.get(index);
    if (priority) {
      span.classList.add(`${priority}-token`);
    }
  });
}

function clearDeltaStyles(spans: HTMLElement[]) {
  spans.forEach((span) => {
    span.classList.remove("delta-token", "bridge-token", "familiar-token", "focus-hidden-token");
  });
}

function buildTextRanges(items: string[]) {
  let cursor = 0;
  const ranges = items.map((item) => {
    const normalized = normalizeForMatch(item);
    const start = cursor;
    cursor += normalized.length;
    const end = cursor;
    cursor += 1;
    return { end, normalized, start };
  });

  return {
    fullText: ranges.map((range) => range.normalized).join(" "),
    items: ranges
  };
}

type TextMatch = {
  end: number;
  matchedWords: number;
  start: number;
  type: "exact" | "phrase";
};

function findChunkMatches(pageText: string, chunkText: string): TextMatch[] {
  const normalizedChunk = normalizeForMatch(chunkText);
  if (!normalizedChunk) return [];

  const chunkWords = meaningfulWords(normalizedChunk);
  const exact = pageText.indexOf(normalizedChunk);
  if (exact >= 0) {
    return [
      {
        end: exact + normalizedChunk.length,
        matchedWords: chunkWords.length,
        start: exact,
        type: "exact"
      }
    ];
  }

  const words = chunkWords;
  const matches: TextMatch[] = [];
  const seen = new Set<string>();
  const windowSizes = [24, 20, 16, 12];

  for (const size of windowSizes) {
    for (let index = 0; index + size <= words.length; index += Math.max(1, Math.floor(size / 2))) {
      const phrase = words.slice(index, index + size).join(" ");
      const matchStart = pageText.indexOf(phrase);
      if (matchStart >= 0) {
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
    }
    if (matches.length >= 4) break;
  }

  return matches.sort((a, b) => a.start - b.start);
}

function overlapsExisting(
  matches: TextMatch[],
  candidate: { start: number; end: number }
): boolean {
  return matches.some((match) => candidate.start < match.end && candidate.end > match.start);
}

function shouldStyleChunk(chunk: PageChunk, mode: StyleMode): boolean {
  if (chunk.directive !== "highlight") return false;
  if (chunk.confidence < (mode === "focus" ? 0.72 : 0.58)) return false;
  return chunk.priority === "delta";
}

function isReaderSafeMatch(match: TextMatch, chunk: PageChunk, mode: StyleMode): boolean {
  if (chunk.directive === "highlight") {
    const minimumWords = mode === "focus" ? 18 : Math.max(10, Math.round(18 - chunk.confidence * 8));
    return match.type === "exact" || match.matchedWords >= minimumWords;
  }
  return false;
}

function fallbackDeltaLineIndexes(
  ranges: ReturnType<typeof buildTextRanges>,
  chunkText: string,
  mode: StyleMode
): number[] {
  const chunkWords = new Set(meaningfulWords(normalizeForMatch(chunkText)));
  if (chunkWords.size === 0) return [];

  const scored = ranges.items
    .map((range, index) => {
      const words = meaningfulWords(range.normalized);
      const hits = words.filter((word) => chunkWords.has(word)).length;
      return {
        index,
        score: words.length === 0 ? 0 : hits / words.length,
        words: words.length
      };
    })
    .filter((item) => item.words >= 3 && item.score >= (mode === "focus" ? 0.55 : 0.38))
    .sort((a, b) => b.score - a.score)
    .slice(0, mode === "focus" ? 3 : 6);

  return scored.map((item) => item.index);
}

function expandToWholeLines(
  spans: HTMLElement[],
  priorities: Map<number, Priority>
): Map<number, Priority> {
  const expanded = new Map<number, Priority>();
  const lineByTop = new Map<number, number[]>();

  spans.forEach((span, index) => {
    const top = Math.round(span.offsetTop / 4) * 4;
    const line = lineByTop.get(top) ?? [];
    line.push(index);
    lineByTop.set(top, line);
  });

  for (const [index, priority] of priorities.entries()) {
    const span = spans[index];
    if (!span) continue;
    const top = Math.round(span.offsetTop / 4) * 4;
    const line = lineByTop.get(top) ?? [index];
    for (const lineIndex of line) {
      const current = expanded.get(lineIndex);
      if (!current || PRIORITY_RANK[priority] > PRIORITY_RANK[current]) {
        expanded.set(lineIndex, priority);
      }
    }
  }

  return expanded;
}

function normalizeForMatch(text: string): string {
  return text
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

function meaningfulWords(text: string): string[] {
  return text.split(" ").filter((word) => word.length > 2);
}

function normalizeConfidence(value: number | undefined): number {
  if (typeof value !== "number" || Number.isNaN(value)) return 0.5;
  return Math.max(0, Math.min(1, value));
}

function directiveForPriority(priority: Priority): ReaderDirective {
  if (priority === "delta") return "highlight";
  if (priority === "bridge") return "callout";
  return "leave_normal";
}

function labelForPriority(priority: Priority): string {
  if (priority === "delta") return "New insight";
  if (priority === "bridge") return "Bridge context";
  return "Likely familiar";
}
