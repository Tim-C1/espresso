import { useState } from "react";
import {
  analyzeDocument,
  getConcepts,
  getReader,
  pdfUrl,
  setBaseline,
  uploadDocument
} from "./api";
import type { ConceptTag, ReaderResponse } from "./types";
import PdfDeltaReader from "./PdfDeltaReader";

type Step = "upload" | "baseline" | "reader";

export default function App() {
  const [step, setStep] = useState<Step>("upload");
  const [documentId, setDocumentId] = useState<string | null>(null);
  const [concepts, setConcepts] = useState<ConceptTag[]>([]);
  const [selectedConcepts, setSelectedConcepts] = useState<Set<string>>(new Set());
  const [expressText, setExpressText] = useState("");
  const [reader, setReader] = useState<ReaderResponse | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function run<T>(work: () => Promise<T>): Promise<T | undefined> {
    setBusy(true);
    setError(null);
    try {
      return await work();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Unexpected error");
    } finally {
      setBusy(false);
    }
  }

  async function onUpload(file: File) {
    const upload = await run(() => uploadDocument(file));
    if (!upload) return;
    setDocumentId(upload.document_id);
    const tags = await run(() => getConcepts(upload.document_id));
    if (!tags) return;
    setConcepts(tags);
    setStep("baseline");
  }

  async function onAnalyze() {
    if (!documentId) return;
    const baseline = await run(() =>
      setBaseline(documentId, {
        express_text: expressText,
        mastered_concept_ids: Array.from(selectedConcepts)
      })
    );
    if (!baseline) return;
    const analysis = await run(() => analyzeDocument(documentId));
    if (!analysis) return;
    const readerState = await run(() => getReader(documentId));
    if (!readerState) return;
    setReader(readerState);
    setStep("reader");
  }

  function toggleConcept(id: string) {
    setSelectedConcepts((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  return (
    <main className="app-shell">
      <header className="hero">
        <p className="eyebrow">AI Delta Reader</p>
        <h1>Read for what changed, not what you already know.</h1>
        <p>
          Upload a text PDF, define your knowledge baseline, then get quests and
          visual priority cues while preserving the author's original flow.
        </p>
      </header>

      {error && <div className="error">{error}</div>}

      {step === "upload" && <UploadPanel busy={busy} onUpload={onUpload} />}

      {step === "baseline" && (
        <section className="panel">
          <h2>Knowledge Baseline</h2>
          <label className="field">
            <span>Express Mode</span>
            <textarea
              value={expressText}
              onChange={(event) => setExpressText(event.target.value)}
              placeholder="Example: I understand basic transformers, but I want architectural optimizations and counter-intuitive training details."
            />
          </label>

          <div className="concept-grid">
            {concepts.map((concept) => (
              <button
                className={selectedConcepts.has(concept.id) ? "concept selected" : "concept"}
                key={concept.id}
                onClick={() => toggleConcept(concept.id)}
                type="button"
              >
                <strong>{concept.label}</strong>
                <span>{concept.description}</span>
              </button>
            ))}
          </div>

          <button className="primary" disabled={busy} onClick={onAnalyze}>
            {busy ? "Analyzing..." : "Generate Delta Reader"}
          </button>
        </section>
      )}

      {step === "reader" && reader && documentId && (
        <ReaderPanel reader={reader} pdfSource={pdfUrl(documentId)} />
      )}
    </main>
  );
}

function UploadPanel({
  busy,
  onUpload
}: {
  busy: boolean;
  onUpload: (file: File) => void;
}) {
  return (
    <section className="panel upload-panel">
      <h2>Upload a selectable-text PDF</h2>
      <p>Scanned PDFs and OCR are intentionally out of scope for v1.</p>
      <input
        accept="application/pdf"
        disabled={busy}
        onChange={(event) => {
          const file = event.target.files?.[0];
          if (file) onUpload(file);
        }}
        type="file"
      />
    </section>
  );
}

function ReaderPanel({
  reader,
  pdfSource
}: {
  reader: ReaderResponse;
  pdfSource: string;
}) {
  return (
    <PdfDeltaReader pdfSource={pdfSource} reader={reader} />
  );
}
