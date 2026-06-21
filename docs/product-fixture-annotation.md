# Product fixture annotation workflow

Product-fit labels are judgments about the knowledge delta for one specific
reader baseline. The same sentence can correctly be `delta` for a beginner and
`familiar` for an expert, so labels must never be written without first fixing
the reader profile and known claims.

## 1. Initialize a real-PDF fixture

```bash
make init-product-fixture FIXTURE=my-real-paper PDF=/absolute/path/paper.pdf
```

This copies the PDF into `resource/product-fixtures/my-real-paper/` and creates
`baseline.template.json`, `fixture.json`, and a fixture README. Edit
`fixture.json` to set the domain, fixture type, notes, source attribution, and
eventual quality thresholds.

## 2. Write the baseline

Copy `baseline.template.json` to `baseline.json`. Describe a concrete reader in
`reader_profile`, then list:

- `known_concepts`: terminology and mechanisms the reader understands;
- `familiar_claims`: propositions or results already in their baseline;
- `interests`: relevant topics, which do not imply novelty;
- `explicit_not_interested_topics`: material that should stay unobtrusive.

Keep claims specific enough to distinguish relevance from prior knowledge.

## 3. Export canonical sentence candidates

```bash
make export-product-candidates FIXTURE=my-real-paper
```

The command uses the pinned PDF.js canonical extractor and writes:

- `candidates.json` for tooling and validation;
- `candidates.md`, grouped by page with a manual checklist;
- `gold_labels.template.json`, containing body sentences only.

Sentence IDs and item-range availability come from the same canonical model
used by analysis and rendering. Re-export after changing the PDF.

## 4. Label the document

Copy `gold_labels.template.json` to `gold_labels.json`. For each sentence, fill
`expected_priority`, `expected_directive`, and a baseline-relative `reason`.
Optional review metadata supports:

- `label_type`: `standard`, `trap`, or `control`;
- `difficulty`: `easy`, `medium`, `hard`, or `adversarial`;
- `review_status`: `draft`, `reviewed`, `approved`, or `rejected`;
- `tags` and `annotator` for local review conventions.

Non-body candidates are excluded by default. If one is deliberately labeled,
add `allow_non_body: true` and explain why.

## 5. Validate and review

```bash
make validate-product-labels FIXTURE=my-real-paper
make review-product-labels FIXTURE=my-real-paper
```

Validation checks IDs, duplicates, enums, reasons, review metadata, and unsafe
candidate kinds. Review writes
`artifacts/product-eval/my-real-paper/label_review.md` with coverage,
distributions, page density, review status, trap counts, and suspicious labels.

Resolve missing candidates, empty reasons, accidental metadata/reference
deltas, very short deltas, and pages with excessive highlights before treating
the fixture as a quality gate.

## 6. Evaluate

```bash
make eval-product-fixture FIXTURE=my-real-paper
```

Evaluation is deterministic and does not call a real provider. Interpret delta
precision as the reliability of highlighted novelty, delta recall as coverage
of human-labeled novelty, familiar suppression as restraint on known material,
and visible guidance per page as annotation density. `N/A` means a metric had no
applicable denominator; JSON reports preserve it as `null` with numerator and
denominator counts.

Only add thresholds after labels are reviewed. Thresholds belong in
`fixture.json` and should reflect fixture type and sample size rather than force
perfect scores on realistic documents.
