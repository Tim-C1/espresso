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

Backend:

```bash
make run-backend
```

Frontend:

```bash
make run-frontend
```

The frontend expects the backend at `http://localhost:8080`. Override with `VITE_API_BASE_URL`.

## AI Provider

The backend defaults to a deterministic mock provider so the product flow works without external credentials.

Set these environment variables to use an OpenAI-compatible chat endpoint:

```bash
AI_PROVIDER=openai-compatible
AI_BASE_URL=https://api.openai.com/v1
AI_API_KEY=...
AI_MODEL=gpt-4.1-mini
```

The v2 reader contract asks the model for reader directives, not just broad
summaries. Each analyzed chunk returns a priority, directive, confidence,
reader-facing label, and rationale. The frontend only applies direct PDF
highlighting for high-confidence `highlight` directives; lower-confidence
guidance is shown as margin notes so the PDF remains readable.

## V1 Scope

- Text/selectable PDFs only.
- Temporary in-memory sessions only.
- No auth, saved library, OCR, billing, or mobile/desktop packaging yet.
- Core service code is kept separate from HTTP route glue so future local desktop/mobile shells can reuse the analysis pipeline.
