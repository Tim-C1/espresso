use crate::{
    ai::ChatProvider,
    models::{ChunkAnnotation, ConceptTag, Quest, TextChunk, UserBaseline},
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
    concepts: &[ConceptTag],
    baseline: &UserBaseline,
) -> anyhow::Result<(Vec<Quest>, Vec<ChunkAnnotation>)> {
    let analysis = ai.analyze_delta(chunks, concepts, baseline).await?;
    Ok((analysis.quests, analysis.annotations))
}
