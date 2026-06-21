use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{bail, Context};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::models::{
    CandidateKind, CanonicalItemRange, CanonicalReadingUnit, CanonicalTextModel, TextQuoteSelector,
    CANONICAL_TEXT_SCHEMA_VERSION,
};

const QUOTE_CONTEXT_LENGTH: usize = 32;

pub fn extract_canonical_text_model(
    pdf_bytes: &[u8],
    document_id: Uuid,
) -> anyhow::Result<CanonicalTextModel> {
    let mut child = worker_command(document_id)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to start the PDF.js canonical text worker")?;

    child
        .stdin
        .take()
        .context("canonical text worker stdin was unavailable")?
        .write_all(pdf_bytes)
        .context("failed to send PDF bytes to the canonical text worker")?;

    let model = parse_worker_output(child.wait_with_output()?, document_id)?;
    let expected_hash = format!("{:x}", Sha256::digest(pdf_bytes));
    if model.pdf_hash != expected_hash {
        bail!(
            "canonical text worker returned PDF hash {}, expected {}",
            model.pdf_hash,
            expected_hash
        );
    }
    Ok(model)
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn extract_canonical_text_fixture(
    fixture: &Path,
    document_id: Uuid,
) -> anyhow::Result<CanonicalTextModel> {
    let output = worker_command(document_id)
        .arg("--fixture")
        .arg(fixture)
        .output()
        .context("failed to run the PDF.js canonical text fixture worker")?;
    parse_worker_output(output, document_id)
}

pub fn generate_canonical_reading_units(
    model: &CanonicalTextModel,
) -> anyhow::Result<Vec<CanonicalReadingUnit>> {
    let mut units = Vec::new();

    for page in &model.pages {
        for (sentence_index, span) in sentence_spans(&page.normalized_text)
            .into_iter()
            .enumerate()
        {
            let item_ranges = page
                .text_items
                .iter()
                .filter_map(|item| {
                    let start = span.norm_start.max(item.normalized_start);
                    let end = span.norm_end.min(item.normalized_end);
                    (start < end).then(|| CanonicalItemRange {
                        item_id: item.item_id.clone(),
                        normalized_start: start - item.normalized_start,
                        normalized_end: end - item.normalized_start,
                    })
                })
                .collect::<Vec<_>>();

            let Some(first_range) = item_ranges.first() else {
                bail!(
                    "canonical sentence {} on page {} does not map to a PDF.js text item",
                    sentence_index + 1,
                    page.page
                );
            };
            let last_range = item_ranges.last().expect("a first item range exists");
            let normalized_text = page.normalized_text[span.byte_start..span.byte_end].to_owned();
            let prefix_start = span.norm_start.saturating_sub(QUOTE_CONTEXT_LENGTH);
            let suffix_end = (span.norm_end + QUOTE_CONTEXT_LENGTH)
                .min(page.normalized_text.encode_utf16().count());

            units.push(CanonicalReadingUnit {
                sentence_id: format!("cp{}s{}", page.page, sentence_index + 1),
                page: page.page,
                text: normalized_text.clone(),
                normalized_text: normalized_text.clone(),
                norm_start: span.norm_start,
                norm_end: span.norm_end,
                start_item_id: first_range.item_id.clone(),
                end_item_id: last_range.item_id.clone(),
                item_ranges,
                quote_selector: TextQuoteSelector {
                    exact: normalized_text,
                    prefix: slice_utf16(&page.normalized_text, prefix_start, span.norm_start)
                        .trim()
                        .to_owned(),
                    suffix: slice_utf16(&page.normalized_text, span.norm_end, suffix_end)
                        .trim()
                        .to_owned(),
                },
                candidate_kind: CandidateKind::Unknown,
            });
        }
    }

    classify_canonical_reading_units(&mut units);
    Ok(units)
}

pub fn classify_canonical_reading_units(units: &mut [CanonicalReadingUnit]) {
    let mut in_references = false;

    for unit in units {
        let text = unit.normalized_text.trim();
        if starts_reference_section(text) {
            in_references = true;
            unit.candidate_kind = if is_reference_heading(text) {
                CandidateKind::Heading
            } else {
                CandidateKind::Reference
            };
        } else if in_references || is_reference_entry(text) {
            unit.candidate_kind = CandidateKind::Reference;
        } else if is_heading(text) {
            unit.candidate_kind = CandidateKind::Heading;
        } else if is_page_one_metadata(unit) {
            unit.candidate_kind = CandidateKind::Metadata;
        } else if text.is_empty() {
            unit.candidate_kind = CandidateKind::Unknown;
        } else if is_formula_or_table(text) {
            unit.candidate_kind = CandidateKind::FormulaOrTable;
        } else if text.encode_utf16().count() < 30 {
            unit.candidate_kind = CandidateKind::ShortFragment;
        } else {
            unit.candidate_kind = CandidateKind::BodySentence;
        }
    }
}

fn starts_reference_section(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower == "references"
        || lower == "references."
        || lower.starts_with("references [")
        || lower.starts_with("references 1.")
}

fn is_reference_heading(text: &str) -> bool {
    matches!(text.to_lowercase().as_str(), "references" | "references.")
}

fn is_reference_entry(text: &str) -> bool {
    let text = text.trim_start();
    if let Some(rest) = text.strip_prefix('[') {
        let digits = rest
            .chars()
            .take_while(|character| character.is_ascii_digit());
        let digit_count = digits.count();
        return digit_count > 0 && rest[digit_count..].starts_with(']');
    }

    let Some((number, remainder)) = text.split_once('.') else {
        return false;
    };
    !number.is_empty()
        && number.chars().all(|character| character.is_ascii_digit())
        && remainder.starts_with(char::is_whitespace)
        && text.contains(['"', '“', '”'])
}

fn is_heading(text: &str) -> bool {
    let text = text.trim();
    let label = text.trim_end_matches(['.', ':']);
    let lower = label.to_lowercase();
    if matches!(
        lower.as_str(),
        "abstract"
            | "introduction"
            | "transactions"
            | "timestamp server"
            | "proof-of-work"
            | "network"
            | "incentive"
            | "reclaiming disk space"
            | "simplified payment verification"
            | "combining and splitting value"
            | "privacy"
            | "calculations"
            | "conclusion"
    ) {
        return true;
    }

    let Some(first_word) = text.split_whitespace().next() else {
        return false;
    };
    let numeric_marker = first_word.ends_with('.')
        && first_word.trim_end_matches('.').split('.').all(|part| {
            !part.is_empty() && part.chars().all(|character| character.is_ascii_digit())
        });
    numeric_marker && text.encode_utf16().count() <= 100
}

fn is_page_one_metadata(unit: &CanonicalReadingUnit) -> bool {
    if unit.page != 1 || unit.norm_start > 500 {
        return false;
    }
    let text = unit.normalized_text.trim();
    let lower = text.to_lowercase();
    if text.contains('@')
        || lower.contains("http://")
        || lower.contains("https://")
        || lower.contains("www.")
    {
        return true;
    }

    let words = text
        .split_whitespace()
        .filter(|word| word.chars().any(char::is_alphabetic))
        .collect::<Vec<_>>();
    if words.len() < 2 || words.len() > 14 || text.ends_with(['.', '!', '?']) {
        return false;
    }
    let title_case_words = words
        .iter()
        .filter(|word| {
            word.trim_matches(|character: char| !character.is_alphabetic())
                .chars()
                .next()
                .is_some_and(char::is_uppercase)
        })
        .count();
    title_case_words * 5 >= words.len() * 3
}

fn is_formula_or_table(text: &str) -> bool {
    let lower = text.to_lowercase();
    if lower.starts_with("table ") || lower.starts_with("table:") {
        return true;
    }

    let characters = text.chars().filter(|character| !character.is_whitespace());
    let total = characters.clone().count();
    if total == 0 {
        return false;
    }
    let math_symbols = characters
        .clone()
        .filter(|character| {
            matches!(
                character,
                '=' | '+' | '−' | '×' | '÷' | '∑' | '√' | '<' | '>' | '|' | '{' | '}'
            )
        })
        .count();
    let digits = characters
        .filter(|character| character.is_ascii_digit())
        .count();
    math_symbols >= 2 && (math_symbols + digits) * 4 >= total
}

#[derive(Clone, Copy, Debug)]
struct SentenceSpan {
    byte_start: usize,
    byte_end: usize,
    norm_start: usize,
    norm_end: usize,
}

fn sentence_spans(text: &str) -> Vec<SentenceSpan> {
    let chars = text.char_indices().collect::<Vec<_>>();
    let mut spans = Vec::new();
    let mut start_index = 0;
    let mut index = 0;

    while index < chars.len() {
        if matches!(chars[index].1, '.' | '!' | '?') {
            let mut end_index = index + 1;
            while end_index < chars.len()
                && matches!(chars[end_index].1, '\'' | '"' | '’' | '”' | ')' | ']')
            {
                end_index += 1;
            }
            if end_index == chars.len() || chars[end_index].1.is_whitespace() {
                push_sentence_span(text, &chars, start_index, end_index, &mut spans);
                start_index = end_index;
                index = end_index;
                continue;
            }
        }
        index += 1;
    }

    push_sentence_span(text, &chars, start_index, chars.len(), &mut spans);
    spans
}

fn push_sentence_span(
    text: &str,
    chars: &[(usize, char)],
    mut start_index: usize,
    mut end_index: usize,
    spans: &mut Vec<SentenceSpan>,
) {
    while start_index < end_index && chars[start_index].1.is_whitespace() {
        start_index += 1;
    }
    while end_index > start_index && chars[end_index - 1].1.is_whitespace() {
        end_index -= 1;
    }
    if start_index == end_index {
        return;
    }

    let byte_start = chars[start_index].0;
    let byte_end = chars
        .get(end_index)
        .map_or(text.len(), |(offset, _)| *offset);
    spans.push(SentenceSpan {
        byte_start,
        byte_end,
        norm_start: text[..byte_start].encode_utf16().count(),
        norm_end: text[..byte_end].encode_utf16().count(),
    });
}

fn slice_utf16(text: &str, start: usize, end: usize) -> String {
    let mut offset = 0;
    text.chars()
        .filter(|character| {
            let char_start = offset;
            offset += character.len_utf16();
            char_start >= start && offset <= end
        })
        .collect()
}

fn worker_command(document_id: Uuid) -> Command {
    let mut command = Command::new("node");
    command
        .arg(worker_path())
        .arg("--document-id")
        .arg(document_id.to_string());
    command
}

fn worker_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tools/pdfjs-extractor/extract.mjs")
}

fn parse_worker_output(
    output: std::process::Output,
    document_id: Uuid,
) -> anyhow::Result<CanonicalTextModel> {
    if !output.status.success() {
        bail!(
            "canonical text worker failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let json = output
        .stdout
        .split(|byte| *byte == b'\n')
        .rev()
        .find(|line| line.first() == Some(&b'{'))
        .context("canonical text worker returned no JSON object")?;
    let model: CanonicalTextModel =
        serde_json::from_slice(json).context("canonical text worker returned invalid JSON")?;
    if model.schema_version != CANONICAL_TEXT_SCHEMA_VERSION {
        bail!("unsupported canonical text schema {}", model.schema_version);
    }
    if model.document_id != document_id {
        bail!("canonical text worker returned a different document ID");
    }
    if model.pdf_hash.len() != 64 || !model.pdf_hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("canonical text worker returned an invalid SHA-256 hash");
    }
    Ok(model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_extraction_is_stable_and_preserves_offsets() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/pdfjs-extractor/fixtures/text-content.json");
        let document_id = Uuid::nil();
        let first = extract_canonical_text_fixture(&fixture, document_id).unwrap();
        let second = extract_canonical_text_fixture(&fixture, document_id).unwrap();

        assert_eq!(
            serde_json::to_vec(&first).unwrap(),
            serde_json::to_vec(&second).unwrap()
        );
        assert_eq!(first.pages.len(), 2);
        assert_eq!(
            first.pages[0].normalized_text,
            "A transformer spans lines. Repeated phrase. Repeated phrase."
        );
        assert_eq!(first.pages[0].text_items[1].item_id, "p1i2");
        assert_eq!(first.pages[0].text_items[1].raw_start, 2);
        assert_eq!(first.pages[0].text_items[1].normalized_start, 2);
        assert_eq!(first.pages[1].normalized_text, "Unicode café text.");
    }

    #[test]
    fn canonical_reading_units_are_stable_and_page_scoped() {
        let model = fixture_model();
        let first = generate_canonical_reading_units(&model).unwrap();
        let second = generate_canonical_reading_units(&model).unwrap();

        assert_eq!(
            serde_json::to_vec(&first).unwrap(),
            serde_json::to_vec(&second).unwrap()
        );
        assert_eq!(
            first
                .iter()
                .map(|unit| unit.sentence_id.as_str())
                .collect::<Vec<_>>(),
            vec!["cp1s1", "cp1s2", "cp1s3", "cp2s1"]
        );
        assert!(first.iter().all(|unit| !unit.item_ranges.is_empty()));
        assert!(first
            .iter()
            .all(|unit| unit.quote_selector.exact == unit.normalized_text));
    }

    #[test]
    fn multi_span_hyphenated_sentence_preserves_item_ranges() {
        let units = generate_canonical_reading_units(&fixture_model()).unwrap();
        let unit = &units[0];

        assert_eq!(unit.normalized_text, "A transformer spans lines.");
        assert_eq!(unit.start_item_id, "p1i1");
        assert_eq!(unit.end_item_id, "p1i3");
        assert_eq!(
            unit.item_ranges
                .iter()
                .map(|range| range.item_id.as_str())
                .collect::<Vec<_>>(),
            vec!["p1i1", "p1i2", "p1i3"]
        );
        assert_eq!(unit.item_ranges[1].normalized_start, 0);
        assert_eq!(unit.item_ranges[1].normalized_end, 5);
    }

    #[test]
    fn repeated_sentences_have_distinct_ids_ranges_and_quote_context() {
        let units = generate_canonical_reading_units(&fixture_model()).unwrap();
        let first = &units[1];
        let second = &units[2];

        assert_eq!(first.text, second.text);
        assert_ne!(first.sentence_id, second.sentence_id);
        assert_ne!(first.norm_start, second.norm_start);
        assert_ne!(first.start_item_id, second.start_item_id);
        assert!(!first.quote_selector.prefix.is_empty());
        assert!(!first.quote_selector.suffix.is_empty());
        assert!(!second.quote_selector.prefix.is_empty());
    }

    #[test]
    fn classifies_candidate_kinds_deterministically() {
        let mut units = vec![
            test_unit("Bitcoin: A Peer-to-Peer Electronic Cash System", 1, 0),
            test_unit("Satoshi Nakamoto", 1, 50),
            test_unit("satoshin@gmx.com", 1, 70),
            test_unit("https://www.bitcoin.org", 1, 90),
            test_unit("Abstract", 1, 120),
            test_unit("1. Introduction", 1, 130),
            test_unit(
                "A purely peer-to-peer payment can be sent directly between two parties.",
                1,
                150,
            ),
            test_unit("Brief note.", 2, 0),
            test_unit("Table 1: x = 2 + 3", 2, 20),
            test_unit("References", 3, 0),
            test_unit("[1] W. Dai, b-money, 1998.", 3, 20),
        ];

        classify_canonical_reading_units(&mut units);

        assert_eq!(units[0].candidate_kind, CandidateKind::Metadata);
        assert_eq!(units[1].candidate_kind, CandidateKind::Metadata);
        assert_eq!(units[2].candidate_kind, CandidateKind::Metadata);
        assert_eq!(units[3].candidate_kind, CandidateKind::Metadata);
        assert_eq!(units[4].candidate_kind, CandidateKind::Heading);
        assert_eq!(units[5].candidate_kind, CandidateKind::Heading);
        assert_eq!(units[6].candidate_kind, CandidateKind::BodySentence);
        assert_eq!(units[7].candidate_kind, CandidateKind::ShortFragment);
        assert_eq!(units[8].candidate_kind, CandidateKind::FormulaOrTable);
        assert_eq!(units[9].candidate_kind, CandidateKind::Heading);
        assert_eq!(units[10].candidate_kind, CandidateKind::Reference);
        assert!(is_reference_entry("[7] A. Author, A cited work."));
    }

    #[test]
    fn classification_preserves_item_ranges() {
        let mut units = generate_canonical_reading_units(&fixture_model()).unwrap();
        let ranges_before = units
            .iter()
            .map(|unit| serde_json::to_vec(&unit.item_ranges).unwrap())
            .collect::<Vec<_>>();

        classify_canonical_reading_units(&mut units);

        let ranges_after = units
            .iter()
            .map(|unit| serde_json::to_vec(&unit.item_ranges).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(ranges_before, ranges_after);
        assert!(units.iter().all(|unit| !unit.item_ranges.is_empty()));
    }

    fn test_unit(text: &str, page: usize, norm_start: usize) -> CanonicalReadingUnit {
        let length = text.encode_utf16().count();
        CanonicalReadingUnit {
            sentence_id: format!("cp{page}s{norm_start}"),
            page,
            text: text.to_owned(),
            normalized_text: text.to_owned(),
            norm_start,
            norm_end: norm_start + length,
            start_item_id: format!("p{page}i1"),
            end_item_id: format!("p{page}i1"),
            item_ranges: vec![CanonicalItemRange {
                item_id: format!("p{page}i1"),
                normalized_start: 0,
                normalized_end: length,
            }],
            quote_selector: TextQuoteSelector {
                exact: text.to_owned(),
                prefix: String::new(),
                suffix: String::new(),
            },
            candidate_kind: CandidateKind::Unknown,
        }
    }

    fn fixture_model() -> CanonicalTextModel {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/pdfjs-extractor/fixtures/text-content.json");
        extract_canonical_text_fixture(&fixture, Uuid::nil()).unwrap()
    }
}
