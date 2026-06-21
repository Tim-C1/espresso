use crate::{
    ai::{apply_delta_eligibility_gate, rebuild_canonical_quests, ChatProvider},
    delta_policy::DeltaEligibilityDiagnostics,
    models::{
        CanonicalReadingUnit, CanonicalSentenceAnnotation, ChunkAnnotation, ConceptTag, Quest,
        ReadingAnchor, SentenceCandidate, TextChunk, UserBaseline,
    },
};

pub async fn generate_concepts(
    ai: &dyn ChatProvider,
    chunks: &[TextChunk],
) -> anyhow::Result<Vec<ConceptTag>> {
    ai.generate_concepts(chunks).await
}

pub async fn analyze_document(
    ai: &dyn ChatProvider,
    chunks: &[TextChunk],
    canonical_candidates: &[CanonicalReadingUnit],
    legacy_sentence_candidates: &[SentenceCandidate],
    concepts: &[ConceptTag],
    baseline: &UserBaseline,
) -> anyhow::Result<(
    Vec<Quest>,
    Vec<ChunkAnnotation>,
    Vec<ReadingAnchor>,
    Vec<CanonicalSentenceAnnotation>,
    DeltaEligibilityDiagnostics,
)> {
    let mut analysis = ai
        .analyze_delta(
            chunks,
            canonical_candidates,
            legacy_sentence_candidates,
            concepts,
            baseline,
        )
        .await?;
    let diagnostics =
        apply_delta_eligibility_gate(&mut analysis.canonical_annotations, concepts, baseline);
    if !analysis.canonical_annotations.is_empty() {
        analysis.quests = rebuild_canonical_quests(&analysis.canonical_annotations);
    }
    Ok((
        analysis.quests,
        analysis.annotations,
        analysis.reading_anchors,
        analysis.canonical_annotations,
        diagnostics,
    ))
}
