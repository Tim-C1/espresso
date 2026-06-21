# Experienced Retrieval Novelty Override

This synthetic adversarial fixture protects the positive side of the delta
eligibility policy. Each highlighted finding overlaps a claim the experienced
reader already knows, but also supplies a new failure, regression, quantified
tradeoff, or changed condition.

The fixture must report familiar-claim overlap and retain delta highlights due
to novelty cues. Its familiar control sentences ensure the same baseline still
vetoes claims that contain no new evidence. The PDF is synthetic public test
data and contains no private or provider output.

Regenerate the deterministic PDF with
`node tools/generate-novelty-override-fixture.mjs` from the repository root.
