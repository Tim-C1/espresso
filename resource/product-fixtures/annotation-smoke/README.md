# annotation-smoke

Real-PDF product-fit annotation fixture.

1. Copy `baseline.template.json` to `baseline.json` and describe a specific reader.
2. Run `make export-product-candidates FIXTURE=annotation-smoke`.
3. Copy `gold_labels.template.json` to `gold_labels.json` and label every intended candidate.
4. Validate and review the labels before evaluation.
