# Evaluation Fixture Types

AI Delta Reader uses three fixture classes because matching correctness and
product usefulness are different questions.

## Infrastructure regression fixtures

`pdf-samples/bitcoin.pdf` and captured bitcoin reader-state diagnostics verify
the end-to-end text and rendering contract: canonical sentence annotations,
non-empty item ranges, direct-range or rect-overlay rendering, and zero legacy
search, fuzzy repair, or missing-range regressions. Bitcoin is concise and
dense, so it is not treated as evidence that the product finds useful deltas for
an experienced reader.

## Anchoring stress fixtures

The deterministic fixtures under `frontend/fixtures/` isolate text-layer edge
cases such as multiline spans, hyphenated line breaks, repeated phrases,
density caps, overlap conflicts, and legacy capture fallback. They test anchor
mechanics, not familiar-versus-novel judgment.

## Product-fit fixtures

Directories under `product-fixtures/` contain a PDF, an experienced-reader
baseline, complete sentence-level gold labels, and fixture-specific rationale.
`make eval-product-fixture FIXTURE=<name>` performs canonical extraction and a
deterministic baseline-aware analysis without contacting a provider, then
reports delta, bridge, familiar-suppression, known-as-delta, and guidance-density
metrics. Gold labels score predictions but are never inputs to the analyzer.
Each evaluation also writes detailed machine-readable and Markdown error reports
to `artifacts/product-eval/<name>/`, grouped by false deltas, missed deltas,
familiar highlights, bridge confusions, over-annotation, and likely cause.

Each directory has this stable shape:

```text
product-fixtures/<name>/
  document.pdf
  baseline.json
  gold_labels.json
  fixture.json
  README.md
```

Baselines contain `reader_profile`, `known_concepts`, `familiar_claims`,
`interests`, and `explicit_not_interested_topics`. Gold labels cover every
canonical body sentence with `sentence_id`, `expected_priority`,
`expected_directive`, and a human-readable `reason`.

`fixture.json` classifies the benchmark as `smoke`, `realistic`, or
`adversarial`, names the baseline, and defines per-fixture minimum/maximum
thresholds. Smoke fixtures may require perfect scores; realistic and adversarial
fixtures use bounded quality thresholds and may intentionally retain diagnostic
mismatches.

Current product benchmarks:

- `experienced-retrieval`: small product-fit smoke test;
- `beginner-retrieval`: the same smoke document with a novice baseline and more
  explanatory callouts;
- `experienced-retrieval-hard`: five-page adversarial product-quality benchmark
- `experienced-retrieval-novelty-override`: adversarial regression benchmark
  proving that strong new evidence survives familiar-claim overlap
  with 40 body sentences and known-but-important-sounding near misses.
