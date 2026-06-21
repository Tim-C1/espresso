# AI Delta Reader

AI Delta Reader is a PDF-first reading interface that preserves the original text while highlighting the user's incremental knowledge delta.

## Workspace

- `crates/backend`: Rust Axum API for uploads, PDF text extraction, baseline handling, AI orchestration, and reader state.
- `frontend`: Vite React TypeScript app for upload, baseline setup, guided concept selection, and annotated reading.

## Local Setup

Prerequisites:

- Rust stable toolchain
- Node.js 20+

Install frontend dependencies:

```bash
make install
```

Run the full verification suite:

```bash
make check
```

Run the deterministic experienced-reader product-fit benchmark:

```bash
make eval-product-fixture FIXTURE=experienced-retrieval
make eval-product-fixture FIXTURE=beginner-retrieval
make eval-product-fixture FIXTURE=experienced-retrieval-hard
```

Infrastructure, anchoring-stress, and product-fit fixture roles are documented
in [`resource/README.md`](resource/README.md).

Backend:

```bash
make run-backend
```

Frontend:

```bash
make run-frontend
```

The frontend expects the backend at `http://localhost:8080`. Override with `VITE_API_BASE_URL`.

To inspect per-page annotation matching and rendering decisions during local
development, start Vite with `VITE_ANNOTATION_DEBUG=true`. The debug panel is
disabled by default and is unavailable in production builds.

Run the same matching and density-cap pipeline against deterministic text-layer
fixtures from the terminal:

```bash
make debug-annotations
make debug-annotations FIXTURE=multiline
make debug-annotations FIXTURE=hyphenation
make debug-annotations FIXTURE=real-pdf
make debug-annotations FIXTURE=real-pdf-dense
```

For machine-readable output without npm's command banner, run
`cd frontend && npm run --silent debug:annotations -- --fixture real-pdf --json`.

Inspect the canonical PDF.js text model for a PDF:

```bash
make debug-canonical PDF=/path/to/document.pdf
make debug-canonical PDF=/path/to/document.pdf SENTENCES=1
```

With no `PDF`, the command inspects the deterministic textContent fixture. The
output includes the PDF SHA-256, exact PDF.js extractor version and options,
page text, stable item IDs, UTF-16 raw/normalized offsets, transforms,
dimensions, and optional bounding boxes. New uploads use this same extraction
path to build the sentence IDs supplied to AI analysis. Extraction failures
retain the legacy analysis path as a conservative compatibility fallback.

`SENTENCES=1` prints canonical reading-unit metrics and examples, including
stable page-scoped sentence IDs and item-local normalized ranges. Each unit is
additively classified as body text, heading, metadata,
reference, formula/table content, short fragment, or unknown; no candidates are
removed by classification. AI inline targets are selected by canonical sentence
ID and resolved by the backend to source text, quote selectors, and item ranges.
The frontend renders canonical targets from those item ranges: full-item targets
map directly to PDF.js text-layer spans and partial targets use a separate rect
overlay. Source-text matching remains only for legacy reader states.

`real-pdf` currently uses captured PDF.js `textContent.items` and line positions,
not live extraction from a binary PDF. This keeps the harness deterministic while
exercising the same pure matching and allocation path as the reader.

### Real analysis capture

Start the frontend with `VITE_ANNOTATION_DEBUG=true`, upload and analyze a PDF,
then scroll through the document until every page has rendered. Use **Export
diagnostics** in the reader toolbar to download `reader-state.json`. The capture
contains the backend reader response plus PDF.js text items and line positions.

Store and inspect the capture from the repository root:

```bash
mkdir -p artifacts/annotation-debug
mv ~/Downloads/reader-state.json artifacts/annotation-debug/reader-state.json
make debug-annotations FILE=artifacts/annotation-debug/reader-state.json
```

Machine-readable output uses the same capture:

```bash
cd frontend
npm run --silent debug:annotations -- \
  --reader-state ../artifacts/annotation-debug/reader-state.json --json
```

Captured reader-state JSON files are ignored by Git because they may contain
document text and baseline data.

New analyses return validated `reading_anchors` selected from backend-generated
sentence candidates. `chunk_annotations` remains in the reader response for old
captures and is reported as `legacy_chunk_annotation`; restart the backend and
reanalyze a document before evaluating sentence-anchor quality metrics.

## AI Provider

The backend keeps product analysis behind a `ChatProvider` trait and resolves
credentials through a small provider adapter. This keeps provider-specific API
keys and defaults out of the reading pipeline while still using a shared
chat-completions transport for compatible providers.

The backend defaults to a deterministic mock provider when no API key is
present, so the product flow works without external credentials. If
`AI_API_KEY`, `GEMINI_API_KEY`, or `OPENAI_API_KEY` is set, the backend uses a
real chat-completions provider unless `AI_PROVIDER=mock` is set explicitly.

With `GEMINI_API_KEY`, the backend defaults to Google's OpenAI-compatible Gemini
endpoint and `gemini-2.5-flash`:

```bash
AI_PROVIDER=gemini
AI_BASE_URL=https://generativelanguage.googleapis.com/v1beta/openai
GEMINI_API_KEY=...
AI_MODEL=gemini-2.5-flash
```

Provider precedence:

- `AI_PROVIDER=mock` always uses the deterministic local provider.
- `AI_PROVIDER=gemini` uses Gemini defaults unless `AI_BASE_URL` or `AI_MODEL`
  overrides them.
- `AI_PROVIDER=openai` or `AI_PROVIDER=openai-compatible` uses OpenAI defaults.
- `AI_PROVIDER=custom` expects `AI_API_KEY`, `AI_BASE_URL`, and `AI_MODEL` for
  another OpenAI-compatible endpoint.
- `AI_API_KEY` is the generic override key and takes precedence over
  provider-specific keys.

The v2 reader contract asks the model for reader directives, not just broad
summaries. Each analyzed chunk returns a priority, directive, confidence,
reader-facing label, and rationale. The frontend applies subtle PDF text
highlights and fades only when confidence and per-page readability caps allow it;
lower-confidence guidance remains in page notes.

## V1 Scope

- Text/selectable PDFs only.
- Temporary in-memory sessions only.
- No auth, saved library, OCR, billing, or mobile/desktop packaging yet.
- Core service code is kept separate from HTTP route glue so future local desktop/mobile shells can reuse the analysis pipeline.
