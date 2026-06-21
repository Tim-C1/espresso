import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import * as pdfjsLib from "pdfjs-dist";
import pdfWorkerUrl from "pdfjs-dist/build/pdf.worker.mjs?url";
import {
  ANNOTATION_DEBUG_ENABLED,
  type AnnotationReaderCapture,
  type CapturedTextLayerPage,
  type PageAnnotationDebug
} from "./annotationDebug";
import {
  diagnoseAnnotationPage,
  type DiagnosticChunk,
  type StyleMode
} from "./annotationDiagnostics";
import type {
  AnnotationSource,
  ChunkAnnotation,
  Priority,
  ReaderDirective,
  ReaderResponse,
  TextChunk
} from "./types";

pdfjsLib.GlobalWorkerOptions.workerSrc = pdfWorkerUrl;

type PageChunk = DiagnosticChunk & {
  rationale: string;
  readerLabel: string;
};

type PageRenderState = {
  pageNumber: number;
  pdf: pdfjsLib.PDFDocumentProxy;
  chunks: PageChunk[];
  mode: StyleMode;
  onTextLayerCapture?: (page: CapturedTextLayerPage) => void;
  scale: number;
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
  const [capturedPageCount, setCapturedPageCount] = useState(0);
  const pageRefs = useRef<Map<number, HTMLDivElement>>(new Map());
  const capturedPagesRef = useRef<Map<number, CapturedTextLayerPage>>(new Map());

  const chunksByPage = useMemo(() => groupChunksByPage(reader), [reader]);
  const annotationCounts = useMemo(() => countPriorities(reader), [reader]);
  const captureTextLayer = useCallback((page: CapturedTextLayerPage) => {
    capturedPagesRef.current.set(page.pageNumber, page);
    setCapturedPageCount(capturedPagesRef.current.size);
  }, []);

  useEffect(() => {
    let cancelled = false;
    capturedPagesRef.current.clear();
    setCapturedPageCount(0);

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

  function exportDiagnosticsCapture() {
    const pages = Array.from(capturedPagesRef.current.values()).sort(
      (left, right) => left.pageNumber - right.pageNumber
    );
    if (pages.length !== reader.page_count) return;
    const capture: AnnotationReaderCapture = {
      fixtureType: "captured_reader_state",
      mode,
      name: reader.filename,
      pages,
      reader,
      schemaVersion: 1
    };
    const url = URL.createObjectURL(
      new Blob([JSON.stringify(capture, null, 2)], { type: "application/json" })
    );
    const link = document.createElement("a");
    link.href = url;
    link.download = "reader-state.json";
    link.click();
    URL.revokeObjectURL(url);
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
          {ANNOTATION_DEBUG_ENABLED && (
            <button
              disabled={capturedPageCount !== reader.page_count}
              onClick={exportDiagnosticsCapture}
              title="Available after every PDF page has rendered"
              type="button"
            >
              Export diagnostics
            </button>
          )}
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
          Filtered uses subtle highlights and fades. Focus keeps only the strongest cues. Normal
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
                  onTextLayerCapture={captureTextLayer}
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

function LazyPdfPage({ pageNumber, pdf, chunks, mode, onTextLayerCapture, scale }: PageRenderState) {
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
        <PdfPage
          chunks={chunks}
          mode={mode}
          onTextLayerCapture={onTextLayerCapture}
          pageNumber={pageNumber}
          pdf={pdf}
          scale={scale}
        />
      ) : (
        <div className="pdf-page-placeholder">Page {pageNumber}</div>
      )}
    </div>
  );
}

function PdfPage({ pageNumber, pdf, chunks, mode, onTextLayerCapture, scale }: PageRenderState) {
  const pageRef = useRef<HTMLDivElement | null>(null);
  const canvasSlotRef = useRef<HTMLDivElement | null>(null);
  const textLayerRef = useRef<HTMLDivElement | null>(null);
  const annotationLayerRef = useRef<HTMLDivElement | null>(null);
  const textItemsRef = useRef<unknown[]>([]);
  const styleStateRef = useRef({ chunks, mode });
  const [error, setError] = useState<string | null>(null);
  const [debug, setDebug] = useState<PageAnnotationDebug | null>(null);

  useEffect(() => {
    styleStateRef.current = { chunks, mode };
  }, [chunks, mode]);

  useEffect(() => {
    let cancelled = false;
    let textLayer: { cancel: () => void; render: () => Promise<unknown> } | null = null;
    let renderTask: { cancel: () => void; promise: Promise<unknown> } | null = null;

    async function renderPage() {
      if (
        !pageRef.current ||
        !canvasSlotRef.current ||
        !textLayerRef.current ||
        !annotationLayerRef.current
      ) return;

      setError(null);
      textLayerRef.current.innerHTML = "";
      annotationLayerRef.current.innerHTML = "";

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
        if (
          cancelled ||
          !canvasSlotRef.current ||
          !textLayerRef.current ||
          !annotationLayerRef.current
        ) return;

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
          onTextLayerCapture?.({
            items: pdfTextItemsToStrings(textItemsRef.current),
            lineTops: Array.from(textLayerRef.current.querySelectorAll<HTMLElement>("span")).map(
              (span) => span.offsetTop
            ),
            pageNumber
          });
          const nextDebug = syncDeltaStyles(
            textLayerRef.current,
            annotationLayerRef.current,
            textItemsRef.current,
            styleStateRef.current.chunks,
            styleStateRef.current.mode,
            pageNumber
          );
          if (ANNOTATION_DEBUG_ENABLED) setDebug(nextDebug);
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
  }, [onTextLayerCapture, pageNumber, pdf, scale]);

  useEffect(() => {
    if (
      !textLayerRef.current ||
      !annotationLayerRef.current ||
      textItemsRef.current.length === 0
    ) return;
    const nextDebug = syncDeltaStyles(
      textLayerRef.current,
      annotationLayerRef.current,
      textItemsRef.current,
      chunks,
      mode,
      pageNumber
    );
    if (ANNOTATION_DEBUG_ENABLED) setDebug(nextDebug);
  }, [chunks, mode, pageNumber]);

  return (
    <>
      {error && <div className="error">{error}</div>}
      <div className="pdf-page-view" ref={pageRef}>
        <div className="pdf-canvas-slot" ref={canvasSlotRef} />
        <div className="canonical-annotation-layer" ref={annotationLayerRef} />
        <div className="textLayer delta-text-layer" ref={textLayerRef} />
        {mode !== "normal" && <PageGuidance chunks={chunks} mode={mode} />}
      </div>
      {ANNOTATION_DEBUG_ENABLED && debug && <AnnotationDebugPanel debug={debug} />}
    </>
  );
}

function AnnotationDebugPanel({ debug }: { debug: PageAnnotationDebug }) {
  const { counts } = debug;
  return (
    <details>
      <summary>
        Annotation debug, page {debug.pageNumber}: generated {counts.generated}, eligible{" "}
        {counts.eligibleAfterFilters}, intended {counts.intendedInline}, attempted{" "}
        {counts.matchAttempted}, candidates {counts.candidateMatches}, usable {counts.usableMatches},
        inline {counts.renderedInline}, fallback {counts.inlineFallbackWithoutUsableMatch}, note{" "}
        {counts.renderedAsPageNote}, normal {counts.leftNormal}, dropped {counts.dropped}
      </summary>
      <table>
        <thead>
          <tr>
            <th>Chunk</th>
            <th>Kind</th>
            <th>Directive</th>
            <th>Source</th>
            <th>Confidence</th>
            <th>Validation</th>
            <th>Intended inline</th>
            <th>Eligibility</th>
            <th>Match attempted</th>
            <th>Match status</th>
            <th>Usable</th>
            <th>Matched chars</th>
            <th>Matched spans</th>
            <th>Rendered spans</th>
            <th>Final status</th>
            <th>Drop reason</th>
          </tr>
        </thead>
        <tbody>
          {debug.annotations.map((annotation) => (
            <tr key={annotation.chunkId}>
              <td>{annotation.chunkId}</td>
              <td>{annotation.annotationKind}</td>
              <td>{annotation.directive}</td>
              <td>{annotation.source}</td>
              <td>{annotation.confidence.toFixed(2)}</td>
              <td>{annotation.validationStatus}</td>
              <td>{annotation.intendedInline ? "yes" : "no"}</td>
              <td>{annotation.eligibilityStatus}</td>
              <td>{annotation.matchAttempted ? "yes" : "no"}</td>
              <td>{annotation.matchStatus}</td>
              <td>{annotation.usableMatch ? "yes" : "no"}</td>
              <td>{(annotation.matchedCharRatio * 100).toFixed(1)}%</td>
              <td>{annotation.matchedSpanCount}</td>
              <td>{annotation.renderedSpanCount}</td>
              <td>{annotation.finalStatus}</td>
              <td>
                {annotation.finalStatus === "dropped" ? annotation.primaryDropReason : "-"}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </details>
  );
}

function PageGuidance({ chunks, mode }: { chunks: PageChunk[]; mode: StyleMode }) {
  const visibleChunks = chunks.filter((chunk) => {
    if (chunk.directive === "leave_normal") return false;
    if (mode === "focus") {
      return (
        (chunk.directive === "highlight" && chunk.confidence >= 0.7) ||
        (chunk.directive === "soft_fade" && chunk.confidence >= 0.8)
      );
    }
    return (
      chunk.directive === "highlight" ||
      chunk.directive === "callout" ||
      chunk.directive === "soft_fade"
    );
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
    .find(
      (chunk) =>
        chunk.directive === "highlight" ||
        chunk.directive === "callout" ||
        chunk.directive === "soft_fade"
    );

  if (visibleChunks.length === 0) return null;

  return (
    <aside className="page-guidance">
      <div className="page-guidance-counts">
        {counts.highlight > 0 && <span className="delta">Highlight {counts.highlight}</span>}
        {counts.soft_fade > 0 && <span className="familiar">Fade {counts.soft_fade}</span>}
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
  if (reader.canonical_sentence_annotations?.length) {
    const byPage = new Map<number, PageChunk[]>();
    for (const annotation of reader.canonical_sentence_annotations) {
      const pageChunks = byPage.get(annotation.page) ?? [];
      pageChunks.push({
        annotationGenerated: true,
        annotationKind: "canonical_sentence_annotation",
        confidence: normalizeConfidence(annotation.confidence),
        directive: annotation.directive,
        id: annotation.annotation_id,
        itemRanges: annotation.item_ranges,
        page: annotation.page,
        priority: annotation.priority,
        rationale: annotation.rationale,
        readerLabel: annotation.reader_label,
        source: annotation.source,
        text: annotation.source_text
      });
      byPage.set(annotation.page, pageChunks);
    }
    return byPage;
  }
  if (reader.reading_anchors && reader.reading_anchors.length > 0) {
    const byPage = new Map<number, PageChunk[]>();
    for (const anchor of reader.reading_anchors) {
      const pageChunks = byPage.get(anchor.page) ?? [];
      pageChunks.push({
        annotationGenerated: true,
        annotationKind: "sentence_anchor",
        confidence: normalizeConfidence(anchor.confidence),
        directive: anchor.directive,
        id: anchor.anchor_id,
        page: anchor.page,
        priority: anchor.priority,
        rationale: anchor.rationale,
        readerLabel: anchor.reader_label,
        source: anchor.source,
        text: anchor.text
      });
      byPage.set(anchor.page, pageChunks);
    }
    return byPage;
  }
  const annotations = new Map<string, ChunkAnnotation>(
    reader.chunk_annotations.map((annotation) => [annotation.chunk_id, annotation])
  );
  const byPage = new Map<number, PageChunk[]>();

  for (const chunk of reader.chunks) {
    const annotation = annotations.get(chunk.id);
    const pageChunks = byPage.get(chunk.page) ?? [];
    pageChunks.push({
      ...chunk,
      annotationKind: "legacy_chunk_annotation",
      annotationGenerated: annotation !== undefined,
      confidence: normalizeConfidence(annotation?.confidence),
      directive: annotation?.directive ?? directiveForPriority(annotation?.priority ?? "bridge"),
      priority: annotation?.priority ?? "bridge",
      rationale: annotation?.rationale ?? "",
      readerLabel: annotation?.reader_label ?? labelForPriority(annotation?.priority ?? "bridge"),
      source: annotation?.source ?? "fallback"
    });
    byPage.set(chunk.page, pageChunks);
  }

  return byPage;
}

function countPriorities(reader: ReaderResponse): Record<Priority, number> {
  const annotations =
    reader.canonical_sentence_annotations?.length
      ? reader.canonical_sentence_annotations
      : reader.reading_anchors && reader.reading_anchors.length > 0
      ? reader.reading_anchors
      : reader.chunk_annotations;
  return annotations.reduce(
    (counts, annotation) => {
      counts[annotation.priority] += 1;
      return counts;
    },
    { bridge: 0, delta: 0, familiar: 0 }
  );
}

function syncDeltaStyles(
  layer: HTMLElement,
  annotationLayer: HTMLElement,
  rawItems: unknown[],
  chunks: PageChunk[],
  mode: StyleMode,
  pageNumber: number
): PageAnnotationDebug {
  const spans = Array.from(layer.querySelectorAll<HTMLElement>("span"));
  clearDeltaStyles(spans);
  annotationLayer.replaceChildren();
  const items = pdfTextItemsToStrings(rawItems);
  const { debug, rectOverlays, spanStyles } = diagnoseAnnotationPage({
    chunks,
    items,
    mode,
    pageNumber,
    spans
  });
  spans.forEach((span, index) => {
    const style = spanStyles.get(index);
    if (style) {
      span.classList.add(style === "delta" ? "delta-token" : "familiar-token");
    }
  });
  renderRectOverlays(annotationLayer, spans, rectOverlays);
  return debug;
}

function renderRectOverlays(
  layer: HTMLElement,
  spans: HTMLElement[],
  overlays: Array<{
    end: number;
    index: number;
    start: number;
    style: "delta" | "familiar";
  }>
) {
  const layerRect = layer.getBoundingClientRect();
  for (const overlay of overlays) {
    const span = spans[overlay.index];
    const textNode = span?.firstChild;
    if (!span || !(textNode instanceof Text)) continue;
    const [rawStart, rawEnd] = normalizedOffsetsToRaw(
      textNode.data,
      overlay.start,
      overlay.end
    );
    if (rawEnd <= rawStart) continue;
    const range = document.createRange();
    range.setStart(textNode, rawStart);
    range.setEnd(textNode, rawEnd);
    for (const rect of range.getClientRects()) {
      if (rect.width <= 0 || rect.height <= 0) continue;
      const element = document.createElement("div");
      element.className = `canonical-annotation-rect ${overlay.style}`;
      element.style.left = `${rect.left - layerRect.left}px`;
      element.style.top = `${rect.top - layerRect.top}px`;
      element.style.width = `${rect.width}px`;
      element.style.height = `${rect.height}px`;
      layer.appendChild(element);
    }
    range.detach();
  }
}

function normalizedOffsetsToRaw(text: string, start: number, end: number): [number, number] {
  const boundaries = Array.from({ length: text.length + 1 }, (_, offset) => offset).filter(
    (offset) => offset === 0 || offset === text.length || !isLowSurrogate(text.charCodeAt(offset))
  );
  const rawOffsetFor = (target: number) =>
    boundaries.find(
      (offset) => canonicalItemText(text.slice(0, offset)).length >= target
    ) ?? text.length;
  return [rawOffsetFor(start), rawOffsetFor(end)];
}

function isLowSurrogate(value: number): boolean {
  return value >= 0xdc00 && value <= 0xdfff;
}

function canonicalItemText(text: string): string {
  return text
    .normalize("NFKC")
    .replace(/\u00ad/g, "")
    .replace(/\s+/gu, " ")
    .trim();
}

function pdfTextItemsToStrings(rawItems: unknown[]): string[] {
  return rawItems.map((item) => {
    if (item && typeof item === "object" && "str" in item) {
      return String((item as { str: unknown }).str ?? "");
    }
    return "";
  });
}

function clearDeltaStyles(spans: HTMLElement[]) {
  spans.forEach((span) => {
    span.classList.remove("delta-token", "bridge-token", "familiar-token", "focus-hidden-token");
  });
}

function normalizeConfidence(value: number | undefined): number {
  if (typeof value !== "number" || Number.isNaN(value)) return 0.5;
  return Math.max(0, Math.min(1, value));
}

function directiveForPriority(priority: Priority): ReaderDirective {
  if (priority === "delta") return "highlight";
  if (priority === "bridge") return "callout";
  return "soft_fade";
}

function labelForPriority(priority: Priority): string {
  if (priority === "delta") return "New insight";
  if (priority === "bridge") return "Bridge context";
  return "Likely familiar";
}
