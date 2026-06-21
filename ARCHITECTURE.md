# AI Delta Reader Architecture

## Project Definition

AI Delta Reader is a PDF-first reading application that helps a reader focus on
the incremental knowledge delta between a document and their existing baseline.
The application preserves the original PDF as the primary reading surface and
adds conservative AI-generated guidance through highlights, fades, page notes,
and pre-reading quests.

The core product promise is not summarization. The reader still reads the
source document. The AI layer classifies document chunks by likely knowledge
value and gives the UI enough structured metadata to guide attention without
rewriting the author's text.

## Product Characteristics

- PDF-first: the rendered PDF remains the main artifact.
- Baseline-aware: analysis depends on a user-provided free-text baseline and
  selected known concepts.
- Delta-oriented: annotations distinguish new material from bridge context and
  likely familiar material.
- Readability-preserving: inline text styling is capped and confidence-gated.
- Provider-adaptable: AI analysis is behind a backend trait and provider config
  adapter.
- Stateless beyond memory: sessions are temporary and stored in process memory.
- V1-local: no auth, persistence, OCR, saved library, billing, or packaged app.

## System Overview

```text
Browser / React
  |
  | HTTP JSON + multipart PDF upload
  v
Rust Axum API
  |
  | PDF bytes -> selectable text chunks
  v
PDF extraction pipeline
  |
  | chunks + baseline + concepts
  v
ChatProvider abstraction
  |
  | mock provider OR chat-completions-compatible provider
  v
AI annotations + quests
  |
  | reader state JSON + original PDF bytes
  v
PDF.js reader with subtle text overlays and page guidance
```

## Workspace Layout

- `crates/backend`: Rust Axum backend for API routes, PDF extraction, session
  state, AI provider orchestration, and response models.
- `frontend`: Vite React TypeScript frontend for upload, baseline capture, PDF
  rendering, annotation overlays, and reader controls.
- `Makefile`: developer commands for install, build, test, lint, and local
  server startup.
- `.env.example`: provider configuration examples.

## Backend Architecture

The backend is a single Rust workspace crate named `delta-reader-backend`.
It starts an Axum server on `127.0.0.1:8080`, nests all API routes under
`/api`, and enables permissive CORS for local frontend development.

Main backend responsibilities:

- Accept PDF uploads as multipart form data.
- Extract selectable text from uploaded PDF bytes.
- Split extracted text into page-scoped chunks.
- Generate initial concept tags from document text.
- Store temporary document sessions in memory.
- Accept and persist the reader baseline.
- Run AI delta analysis after baseline capture.
- Return original PDF bytes and reader state to the frontend.

Primary modules:

- `main.rs`: server boot, tracing, CORS, route registration, provider startup.
- `routes.rs`: HTTP route handlers and API error mapping.
- `models.rs`: shared API/domain data structures.
- `store.rs`: in-memory document session store with TTL cleanup.
- `pdf.rs`: selectable-text PDF extraction and paragraph chunking.
- `analysis.rs`: thin orchestration wrapper around the AI provider trait.
- `ai.rs`: mock provider, provider adapter config, chat-completions transport,
  model prompts, AI response normalization, and tests.

## API Surface

All API routes are mounted under `/api`.

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/health` | Returns `ok` for health checks. |
| `POST` | `/documents` | Upload a PDF, extract text, generate concepts, create a session. |
| `GET` | `/documents/:id/pdf` | Return original uploaded PDF bytes. |
| `GET` | `/documents/:id/concepts` | Return generated concept tags. |
| `POST` | `/documents/:id/baseline` | Store user baseline text and mastered concept IDs. |
| `POST` | `/documents/:id/analyze` | Run delta analysis and persist quests/annotations. |
| `GET` | `/documents/:id/reader` | Return complete reader state for rendering. |

The upload body limit is 50 MB. API errors are returned as JSON:

```json
{ "error": "message" }
```

## Core Data Model

`DocumentSession` is the central backend state object. It contains:

- document identity and filename
- upload timestamp
- page count
- original PDF bytes
- extracted text chunks
- generated concept tags
- optional user baseline
- generated quests
- generated chunk annotations

Important domain types:

- `TextChunk`: page-scoped text block with an ID like `p1c2`.
- `ConceptTag`: model-generated concept option for baseline selection.
- `UserBaseline`: free-text baseline plus mastered concept IDs.
- `Priority`: `delta`, `bridge`, or `familiar`.
- `ReaderDirective`: `highlight`, `soft_fade`, `callout`, or `leave_normal`.
- `ChunkAnnotation`: AI guidance for one chunk.
- `Quest`: pre-reading question anchored to one or more chunks.

## Document Processing Flow

1. User uploads a PDF through the frontend.
2. Backend reads multipart field `file`.
3. `pdf-extract` extracts selectable text by page.
4. Backend chunks paragraphs, preserving page numbers.
5. Empty/selectable-text-free documents fail with a v1 OCR unsupported error.
6. Backend creates a UUID document session.
7. AI provider generates concept tags from an early document sample.
8. Session is stored in memory and the frontend moves to baseline setup.

Chunking uses blank-line paragraph boundaries and combines paragraphs up to
roughly 1,500 characters before creating a new chunk. Chunk IDs encode page and
chunk order, not semantic identity.

## AI Provider Architecture

The app-facing AI boundary is the `ChatProvider` trait:

```rust
generate_concepts(chunks) -> Vec<ConceptTag>
analyze_delta(chunks, concepts, baseline) -> (Vec<Quest>, Vec<ChunkAnnotation>)
```

Provider selection happens once at backend startup through `provider_from_env`.
The reading pipeline depends only on `ChatProvider`, not on provider-specific
keys, base URLs, model names, or HTTP details.

### Provider Adapter Layer

`AiProviderConfig` resolves environment configuration into one of:

- `Mock`: deterministic local provider.
- `ChatCompletions`: a chat-completions-compatible provider config.

`ChatCompletionsConfig` contains:

- provider kind: OpenAI, Gemini, or custom
- base URL
- API key
- model

`ChatCompletionsProvider` is the shared HTTP adapter. It calls
`{base_url}/chat/completions`, sends `response_format: { "type": "json_object" }`,
and parses the assistant message content as JSON.

### Provider Configuration

Provider selection rules:

- `AI_PROVIDER=mock` forces deterministic mock mode.
- `AI_PROVIDER=gemini` uses Gemini defaults unless overridden.
- `AI_PROVIDER=openai` or `AI_PROVIDER=openai-compatible` uses OpenAI defaults.
- `AI_PROVIDER=custom` uses explicit `AI_API_KEY`, `AI_BASE_URL`, and `AI_MODEL`.
- If no provider is specified, key source selects the provider.
- `AI_API_KEY` is the generic override and takes precedence over
  provider-specific keys.
- `GEMINI_API_KEY` selects Gemini defaults.
- `OPENAI_API_KEY` selects OpenAI defaults.
- No key falls back to mock mode.

Gemini defaults:

```bash
AI_PROVIDER=gemini
AI_BASE_URL=https://generativelanguage.googleapis.com/v1beta/openai
GEMINI_API_KEY=...
AI_MODEL=gemini-2.5-flash
```

OpenAI-compatible defaults:

```bash
AI_PROVIDER=openai
AI_BASE_URL=https://api.openai.com/v1
OPENAI_API_KEY=...
AI_MODEL=gpt-4.1-mini
```

### AI Response Normalization

External model responses are not trusted as direct domain objects. The backend
first deserializes chunk annotations into a tolerant raw DTO, then normalizes
string fields into strict app enums.

This protects the API from model mistakes such as:

- putting `leave_normal` in the `priority` field
- returning missing labels
- returning out-of-range confidence values
- omitting annotations for some chunks

Malformed or missing annotations degrade conservatively into fallback
annotations instead of causing the whole analysis request to fail.

## Frontend Architecture

The frontend is a Vite React TypeScript app. It is intentionally thin around
business logic and relies on backend API contracts for document and AI state.

Main frontend responsibilities:

- Upload a PDF.
- Fetch concept tags.
- Capture user baseline text and selected known concepts.
- Trigger analysis.
- Fetch complete reader state.
- Render the original PDF with PDF.js.
- Apply subtle inline overlays and page guidance.

Primary files:

- `App.tsx`: top-level upload, baseline, and reader workflow.
- `api.ts`: typed fetch helpers and API URL construction.
- `types.ts`: TypeScript mirror of backend API models.
- `PdfDeltaReader.tsx`: PDF.js rendering, lazy page rendering, page navigation,
  zoom, overlay matching, density caps, and guidance notes.
- `styles.css`: application and PDF overlay styling.

## Frontend User Flow

1. Upload screen accepts a PDF file.
2. Frontend posts the file to `/api/documents`.
3. Frontend fetches `/api/documents/:id/concepts`.
4. User writes baseline text and selects known concept tags.
5. Frontend posts baseline to `/api/documents/:id/baseline`.
6. Frontend posts `/api/documents/:id/analyze`.
7. Frontend fetches `/api/documents/:id/reader`.
8. Reader opens with the original PDF plus AI-guided overlays.

## PDF Reader Behavior

The PDF reader uses `pdfjs-dist` to render each page into a canvas and a text
layer. The original PDF canvas remains visible. AI overlays are applied to the
PDF.js text layer spans.

Reader modes:

- `Filtered`: default balanced mode with subtle highlights and fades.
- `Focus`: stricter mode that keeps only strongest cues.
- `Normal PDF`: clears AI overlay styling.

Guidance types:

- `highlight`: high-confidence delta content eligible for subtle yellow bands.
- `soft_fade`: high-confidence familiar content eligible for gentle fade.
- `callout`: shown in page guidance, not strongly styled inline.
- `leave_normal`: no visual manipulation.

Readability protections:

- confidence thresholds differ by mode
- per-page density caps limit highlighted and faded spans
- highlight wins over fade on overlap
- fuzzy fallback matching is only used for delta highlights, not fades
- callouts mostly remain outside inline PDF text styling

## State And Persistence

State is process-local and temporary.

- Sessions live in an in-memory `HashMap<DocumentId, DocumentSession>`.
- The store is protected by a Tokio `RwLock`.
- Sessions expire after six hours.
- Expired sessions are purged opportunistically on insert, get, and update.
- Uploaded PDF bytes are stored in memory for later PDF retrieval.

There is no database, durable object store, account model, or saved library in
the current version.

## Security And Privacy Characteristics

Current v1 behavior:

- No authentication.
- No authorization boundaries between users.
- Permissive CORS for local development.
- Uploaded PDF bytes and extracted text live in backend memory.
- AI provider requests send extracted document text, user baseline, concept
  tags, and chunk metadata to the configured provider.
- API keys are read from environment variables.

This architecture is appropriate for local development and early prototyping.
It is not yet production multi-user infrastructure.

## Error Handling Characteristics

- Missing upload file returns `400`.
- Unsupported scanned/OCR-only PDFs return `400`.
- Missing baseline before analysis returns `400`.
- Unknown document IDs return `404`.
- AI provider, network, or parsing failures return `500`.
- Model annotation shape errors are normalized when possible; missing or invalid
  chunk annotations fall back to conservative defaults.

## Verification

The project verification command is:

```bash
make check
```

It runs:

- Rust formatting check
- Rust clippy with warnings denied
- Rust tests
- Frontend TypeScript and Vite production build

Current backend tests cover:

- PDF paragraph chunking
- empty-page behavior
- mock provider familiar-content behavior
- annotation fallback behavior
- confidence and label normalization
- misplaced directive/priority normalization
- provider config selection and key precedence

## Current Limitations

- Selectable-text PDFs only; scanned PDFs and OCR are unsupported.
- No durable storage.
- No user accounts or access control.
- No concurrent multi-user isolation.
- No streaming AI responses.
- No background jobs or retry queue.
- No provider-specific rate-limit/backoff layer.
- No observability beyond tracing logs.
- No frontend test suite.
- PDF text matching can still be imperfect because PDF text layers split words
  and lines inconsistently.

## Extension Points

Near-term extension points:

- Add provider-specific adapters if an API diverges from chat completions.
- Add a persistence layer for documents, sessions, and reader history.
- Add OCR preprocessing for scanned PDFs.
- Add auth and per-user document ownership.
- Add background analysis jobs for large documents.
- Add frontend tests for overlay matching and density caps.
- Add telemetry for provider latency, model failures, and annotation density.
- Add provider retry, timeout, and quota handling policies.

## Design Principles

- Keep source text primary; AI guidance should not replace reading.
- Keep provider-specific configuration outside the reading pipeline.
- Prefer conservative visual treatment over aggressive annotation density.
- Treat model output as untrusted and normalize before storing.
- Keep core analysis logic separate from HTTP route glue.
- Preserve a working deterministic mock path for local development.
