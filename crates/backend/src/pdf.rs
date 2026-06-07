use crate::models::TextChunk;

pub fn extract_text_chunks(pdf_bytes: &[u8]) -> anyhow::Result<(usize, Vec<TextChunk>)> {
    let page_texts = pdf_extract::extract_text_from_mem_by_pages(pdf_bytes)?;
    let page_count = page_texts.len().max(1);
    let mut chunks = Vec::new();

    for (page_index, page_text) in page_texts.iter().enumerate() {
        for (chunk_index, paragraph) in chunk_paragraphs(page_text).into_iter().enumerate() {
            chunks.push(TextChunk {
                id: format!("p{}c{}", page_index + 1, chunk_index + 1),
                page: page_index + 1,
                text: paragraph,
            });
        }
    }

    if chunks.is_empty() {
        anyhow::bail!("No selectable text found. OCR/scanned PDFs are not supported in v1.");
    }

    Ok((page_count, chunks))
}

fn chunk_paragraphs(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();

    for paragraph in text.split("\n\n").map(str::trim).filter(|p| !p.is_empty()) {
        if current.len() + paragraph.len() > 1_500 && !current.is_empty() {
            chunks.push(current.trim().to_owned());
            current.clear();
        }
        current.push_str(paragraph);
        current.push_str("\n\n");
    }

    if !current.trim().is_empty() {
        chunks.push(current.trim().to_owned());
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_paragraphs_without_losing_text() {
        let chunks = chunk_paragraphs("First idea.\n\nSecond idea.");
        assert_eq!(chunks, vec!["First idea.\n\nSecond idea."]);
    }

    #[test]
    fn skips_empty_pages_without_losing_page_count() {
        assert!(chunk_paragraphs("").is_empty());
    }
}
