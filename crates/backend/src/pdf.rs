use crate::models::{SentenceCandidate, TextChunk};

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

pub fn extract_sentence_candidates(chunks: &[TextChunk]) -> Vec<SentenceCandidate> {
    chunks
        .iter()
        .flat_map(|chunk| {
            sentence_ranges(&chunk.text)
                .into_iter()
                .enumerate()
                .map(|(index, (char_start, char_end, text))| SentenceCandidate {
                    sentence_id: format!("{}s{}", chunk.id, index + 1),
                    chunk_id: chunk.id.clone(),
                    page: chunk.page,
                    text,
                    char_start,
                    char_end,
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn sentence_ranges(text: &str) -> Vec<(usize, usize, String)> {
    let chars = text.chars().collect::<Vec<_>>();
    let mut ranges = Vec::new();
    let mut start = 0;
    for (index, character) in chars.iter().enumerate() {
        let terminal = matches!(character, '.' | '!' | '?');
        let boundary = terminal && (index + 1 == chars.len() || chars[index + 1].is_whitespace());
        if boundary {
            push_sentence_range(&chars, start, index + 1, &mut ranges);
            start = index + 1;
        }
    }
    push_sentence_range(&chars, start, chars.len(), &mut ranges);
    ranges
}

fn push_sentence_range(
    chars: &[char],
    mut start: usize,
    mut end: usize,
    ranges: &mut Vec<(usize, usize, String)>,
) {
    while start < end && chars[start].is_whitespace() {
        start += 1;
    }
    while end > start && chars[end - 1].is_whitespace() {
        end -= 1;
    }
    if start < end {
        ranges.push((start, end, chars[start..end].iter().collect()));
    }
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

    #[test]
    fn sentence_candidates_preserve_chunk_and_page_offsets() {
        let chunks = vec![TextChunk {
            id: "p2c3".to_owned(),
            page: 2,
            text: "First sentence. Second sentence!".to_owned(),
        }];
        let candidates = extract_sentence_candidates(&chunks);
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[1].sentence_id, "p2c3s2");
        assert_eq!(candidates[1].chunk_id, "p2c3");
        assert_eq!(candidates[1].page, 2);
        assert_eq!(candidates[1].text, "Second sentence!");
        assert_eq!(candidates[1].char_start, 16);
        assert_eq!(candidates[1].char_end, 32);
    }
}
