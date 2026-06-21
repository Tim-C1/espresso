# AGENTS.md

## Project overview

AI Delta Reader is a PDF-first reading application.

The core product promise is not summarization. The reader should still read the original PDF. AI guidance should help the reader notice the knowledge delta between the document and their existing baseline.

The original PDF canvas remains the primary reading surface. AI output may guide attention through highlights, soft fades, page notes, callouts, and pre-reading questions, but it must not replace the source document.

## Architecture summary

Backend:

* Rust Axum API under `crates/backend`.
* Uploads PDFs.
* Extracts selectable text.
* Splits extracted text into page-scoped chunks.
* Generates concept tags.
* Stores temporary in-memory document sessions.
* Runs AI delta analysis through the provider abstraction.

Frontend:

* Vite React TypeScript app under `frontend`.
* Uses PDF.js to render the original PDF.
* Applies AI annotation overlays to the PDF.js text layer.
* Contains annotation matching, density caps, reader modes, and diagnostics.

Important files:

* `crates/backend/src/models.rs`
* `crates/backend/src/ai.rs`
* `crates/backend/src/pdf.rs`
* `frontend/src/PdfDeltaReader.tsx`
* `frontend/src/types.ts`
* frontend annotation matcher / diagnostics modules
* `Makefile`

## Product invariants

* PDF-first always: never turn the app into a summary-first reader.
* Source text is primary; AI guidance is secondary.
* Inline styling must be conservative and readable.
* Highlight, fade, callout, and leave-normal must remain reversible UI guidance.
* Do not hide or aggressively suppress source text unless the match is reliable and the behavior is explicitly intended.
* Unmatched inline annotations must not be reported as successfully rendered inline.
* If an inline annotation cannot be matched confidently, degrade it to a page note or report it clearly in diagnostics.
* Do not let AI-generated text replace the author’s source text.

## Current known diagnosis

Real backend-output diagnostics on `bitcoin.pdf` showed the main problem:

* AI generated chunk-level annotations, not sentence-level reading anchors.
* Average source text length was about 1000+ characters.
* Most annotation sources were not complete sentences.
* Inline matching was poor because the model was effectively asking the frontend to highlight huge chunks.
* Deterministic dense fixtures show the matcher works well when annotations are short, precise, and source-grounded.

Interpretation:

The current bottleneck is mostly AI annotation granularity, not basic PDF.js matching.

The next major architectural direction is sentence-level anchors:

* Keep `TextChunk` for model context.
* Add sentence or reading-unit candidates.
* Give each candidate a stable `sentence_id`.
* Make the AI select sentence IDs or validated short anchors.
* Avoid using entire chunks as inline highlight/fade targets.

## Annotation quality rules

Inline annotations should be complete sentences or coherent 1–3 sentence passages.

Avoid:

* isolated noun phrases;
* tiny fragments;
* entire 1000-character chunks;
* arbitrary model-generated snippets that are not exact source text;
* high-confidence inline annotations without a reliable PDF text match.

Prefer:

* sentence-level anchors;
* validated source quotes;
* stable IDs such as `sentence_id`;
* page-scoped diagnostics;
* fallback page notes for uncertain matches.

Reasonable target ranges:

* Inline anchor minimum: usually at least one complete sentence.
* Inline anchor maximum: normally below 300–500 characters unless there is a strong reason.
* Callouts may summarize or explain, but inline overlays must stay grounded in source text.

## Reader directive semantics

Valid directives:

* `highlight`: inline emphasis for high-value delta content.
* `soft_fade`: gentle fade for familiar content when the baseline clearly covers it.
* `callout`: page note or side guidance; does not require inline matching.
* `leave_normal`: valid no-op; should be reported as intentionally left normal, not as unsupported.

Diagnostics should reserve `unsupported_directive` for malformed or unknown values only.

## Annotation diagnostics

The project has a deterministic diagnostics harness.

Common commands:

```bash
make debug-annotations
make debug-annotations FIXTURE=deterministic
make debug-annotations FIXTURE=multiline
make debug-annotations FIXTURE=hyphenation
make debug-annotations FIXTURE=real-pdf
make debug-annotations FIXTURE=real-pdf-dense
make debug-annotations FILE=artifacts/annotation-debug/reader-state.json
```

JSON mode:

```bash
cd frontend && npm run --silent debug:annotations -- --json
```

Real backend-output workflow:

```bash
VITE_ANNOTATION_DEBUG=true make run-frontend
```

Then:

1. Upload and analyze a PDF.
2. Render every page.
3. Click `Export diagnostics`.
4. Move the downloaded file:

```bash
mkdir -p artifacts/annotation-debug
mv ~/Downloads/reader-state.json artifacts/annotation-debug/reader-state.json
```

5. Run:

```bash
make debug-annotations FILE=artifacts/annotation-debug/reader-state.json
```

Captured diagnostic files under `artifacts/annotation-debug` should remain Git-ignored.

## Diagnostic lifecycle rules

Every annotation should have a clear lifecycle.

Track:

* generated
* validated or invalid
* eligible or ineligible
* intended inline
* match attempted
* candidate / partial match
* usable match
* rendered inline
* inline fallback without match
* rendered as page note
* intentionally left normal
* dropped

Important invariants:

* `rendered_inline` must mean actually rendered against matched PDF text-layer spans.
* `inline_fallback_without_match` must be separate from `rendered_inline`.
* An annotation with `match = unmatched` must not be reported as `rendered_inline`.
* `rendered_inline <= usable_matches`.
* `usable_matches <= candidate_matches`.
* `candidate_matches <= match_attempted`.
* Final disposition should be mathematically clear:

  * rendered inline
  * inline fallback without match
  * rendered as page note
  * intentionally left normal
  * dropped

Dropped annotations must have exactly one primary drop reason.

Known drop reasons:

* `below_confidence`
* `density_cap`
* `overlap_conflict`
* `unmatched_text`
* `unsupported_directive`

## PDF text matching rules

The matcher must handle PDF.js text-layer quirks.

Required supported cases:

* sentence split across multiple spans;
* sentence split across lines;
* irregular whitespace;
* hyphenated line breaks such as `trans- former` matching `transformer`;
* ordinary hyphenated terms should not be destroyed accidentally;
* repeated text should be diagnosed carefully.

Do not regress these fixtures:

```bash
make debug-annotations FIXTURE=multiline
make debug-annotations FIXTURE=hyphenation
make debug-annotations FIXTURE=real-pdf-dense
```

If changing matcher behavior, update tests and diagnostics together.

## AI provider and prompt rules

Do not call real AI providers in deterministic tests or fixtures.

Do not change AI prompt behavior casually. Prompt changes must be tied to a diagnostic finding.

Before changing prompts, inspect diagnostics:

* generated count;
* annotations per page;
* directive distribution;
* priority distribution;
* average source text length;
* anchors shorter than 30 characters;
* anchors longer than 500 characters;
* incomplete sentence count;
* match attempted;
* usable matches;
* rendered inline;
* page notes;
* drop reasons.

Only change prompts after identifying whether the bottleneck is:

* too few annotations;
* chunk-level annotations;
* incomplete sentence anchors;
* low confidence;
* too many callouts;
* too many leave-normal directives;
* poor familiar/delta classification.

## Sentence-level anchor direction

The next major improvement should move from chunk-level annotations to sentence-level anchors.

Desired backend model:

* `TextChunk` remains context.
* Add `SentenceCandidate` or `ReadingUnit`.
* Each candidate should include:

  * `sentence_id`
  * `chunk_id`
  * `page`
  * `text`
  * `char_start`
  * `char_end`

Desired AI output:

* AI selects existing `sentence_id` values.
* AI should not invent arbitrary source snippets.
* AI should not request inline rendering for entire chunks.
* Unknown sentence IDs must be rejected.
* Long anchors should be rejected or demoted.
* Invalid model output should degrade conservatively.

Temporary backward compatibility with legacy `ChunkAnnotation` is acceptable, but diagnostics should clearly mark legacy chunk-level annotations.

## Testing and verification

Primary verification command:

```bash
make check
```

`make check` should run:

* Rust formatting check
* Rust clippy with warnings denied
* Rust tests
* frontend TypeScript check
* Vite production build

When changing annotation, matching, PDF rendering, diagnostics, prompts, or AI schema, also run the relevant diagnostics:

```bash
make debug-annotations
make debug-annotations FIXTURE=real-pdf-dense
make debug-annotations FIXTURE=hyphenation
```

For real captured output changes, run:

```bash
make debug-annotations FILE=artifacts/annotation-debug/reader-state.json
```

Do not consider annotation-related work complete unless diagnostics still explain generated, eligible, attempted, matched, rendered, fallback, normal, and dropped annotations.

## Development workflow for Codex

Before editing:

* Inspect the relevant files.
* Explain the likely source of the issue.
* Propose a small implementation plan.
* Prefer narrow, testable changes over broad rewrites.

While editing:

* Do not change unrelated files.
* Do not change visual styling unless the task asks for UI styling.
* Do not change AI prompts unless the task asks for prompt behavior.
* Do not introduce new production dependencies without a clear reason.
* Preserve deterministic mock and fixture paths.

After editing:

* Run the smallest relevant diagnostics first.
* Run `make check`.
* Summarize changed files.
* Summarize any remaining risks or limitations.
* Include the exact commands run and their results.

## Safety and privacy

Do not commit uploaded PDFs, captured reader states, API keys, provider outputs, or user private documents.

Keep these ignored:

* `artifacts/annotation-debug/`
* downloaded reader-state JSON files
* local `.env` files
* provider keys
* generated private PDF fixtures unless explicitly intended as public test data

AI provider requests may include extracted document text and user baseline. Treat these as sensitive.

## Do not do

* Do not replace PDF reading with generated summaries.
* Do not treat chunk-level annotations as acceptable inline anchors.
* Do not report unmatched annotations as rendered inline.
* Do not remove diagnostics to simplify code.
* Do not silently drop annotations without a reason.
* Do not weaken tests just to make a command pass.
* Do not make deterministic fixtures call real AI providers.
* Do not assume PDF text extracted by the backend exactly equals PDF.js rendered text.
* Do not aggressively fade or hide content unless matching is reliable.

## long-term Architecture direction
We have confirmed that sentence-level anchors improve AI output quality, but real reader-state diagnostics still fail because backend-extracted sentence text and PDF.js textContent are different text sources.

Current diagnosis:
- Sentence anchors are complete and reasonable length.
- Pages missing PDF.js textContent: 0.
- Intended inline found in backend text: 19.
- Intended inline found in PDF.js page text: 0.
- Many fuzzy nearest candidates are 0.98–1.00, showing the content is visually similar but exact text-source alignment is broken.

Goal:
Refactor the architecture so AI analysis and PDF overlay rendering use the same canonical PDF.js-derived text model.

High-level design:
Introduce a CanonicalTextModel generated from PDF.js textContent. Sentence candidates, AI prompts, AI annotation validation, and frontend rendering should all use this canonical model.

Requirements:
1. Add a CanonicalTextModel domain model.
   It should include:
   - document_id
   - pdf_hash
   - extractor name/version/options
   - pages
   - per-page raw text and normalized text
   - PDF.js text items with item_id, page, str, normalized str, raw offsets, normalized offsets, transform, width, height, and optional bbox/rect data.

2. Add SentenceCandidate / ReadingUnit generation from CanonicalTextModel.
   Each candidate should include:
   - sentence_id
   - page
   - text
   - normalized_text
   - norm_start
   - norm_end
   - start_item_id
   - end_item_id
   - item_ranges
   - quote selector: exact, prefix, suffix.

3. Update backend analysis so the AI sees SentenceCandidate IDs and returns sentence_id selections, not arbitrary quotes or legacy chunk-level inline annotations.

4. Update backend validation:
   - reject unknown sentence_id;
   - reject anchors that do not map to item ranges;
   - reject or demote overly long anchors;
   - preserve callout behavior;
   - preserve conservative fallback behavior.

5. Update reader state:
   - include canonical text model or enough text item/range data to render annotation overlays;
   - include annotation targets as item_ranges and/or rects;
   - keep quote/prefix/suffix for diagnostics and repair, not as the primary render path.

6. Update frontend rendering:
   - stop using source_text string search as the primary inline annotation path;
   - render inline highlights/fades from item_ranges or rects;
   - keep fuzzy matching only as a diagnostic/repair fallback;
   - clearly report legacy fallback if used.

7. Add a PDF.js extraction worker.
   Preferred:
   - backend invokes a Node/pdfjs-dist worker to produce CanonicalTextModel from uploaded PDF bytes.
   Acceptable first step:
   - frontend extracts PDF.js textContent and posts CanonicalTextModel to backend during upload/analyze.

8. Preserve existing deterministic fixtures.
   Add new fixtures for:
   - canonical text model extraction;
   - sentence candidate generation;
   - sentence_id annotation rendering without string matching;
   - multi-span sentence;
   - hyphenated line break;
   - repeated phrase;
   - old reader-state fallback.

9. Diagnostics:
   - add canonical_text_model_available;
   - add annotations rendered by direct_range;
   - add annotations rendered by rect_overlay;
   - add annotations requiring fuzzy repair;
   - add legacy_chunk_annotation count;
   - add text_source_mismatch count.

10. Do not remove old diagnostics until the new path is verified.
11. Do not change visual styling unless necessary for the new overlay layer.
12. Do not call real AI providers in deterministic tests.
13. Run:
   - make debug-annotations FIXTURE=real-pdf-dense
   - make debug-annotations FIXTURE=hyphenation
   - make debug-annotations FILE=artifacts/annotation-debug/reader-state.json
   - make check

Done when:
For new analyses, inline annotations are rendered from canonical sentence_id item ranges or rects, not by searching annotation source text inside PDF.js page text.


## design rules
For experienced readers, relevance is not novelty.
A sentence matching user interests should not be highlighted as delta unless it adds new evidence, a new claim, a new result, a new limitation, or a changed tradeoff beyond the user's familiar baseline.
Familiar-claim overlap should override interest overlap unless explicit novelty evidence is present.
