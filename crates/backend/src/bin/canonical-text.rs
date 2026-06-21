#[path = "../canonical.rs"]
mod canonical;
#[allow(dead_code)]
#[path = "../delta_policy.rs"]
mod delta_policy;
#[allow(dead_code)]
#[path = "../models.rs"]
mod models;

use std::{collections::BTreeMap, env, fs, path::PathBuf};

use anyhow::{bail, Context};
use uuid::Uuid;

fn main() -> anyhow::Result<()> {
    let mut args = env::args().skip(1);
    let mut fixture: Option<PathBuf> = None;
    let mut pdf: Option<PathBuf> = None;
    let mut document_id = Uuid::nil();
    let mut compact = false;
    let mut sentences = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--fixture" => {
                fixture = Some(PathBuf::from(required_value(&mut args, "--fixture")?));
            }
            "--document-id" => {
                document_id = required_value(&mut args, "--document-id")?
                    .parse()
                    .context("--document-id must be a UUID")?;
            }
            "--compact" => compact = true,
            "--sentences" => sentences = true,
            "--help" | "-h" => {
                print_usage();
                return Ok(());
            }
            value if value.starts_with('-') => bail!("unknown option {value}"),
            value if pdf.is_none() => pdf = Some(PathBuf::from(value)),
            value => bail!("unexpected argument {value}"),
        }
    }

    let model = match (pdf, fixture) {
        (Some(path), None) => {
            let bytes = fs::read(&path)
                .with_context(|| format!("failed to read PDF {}", path.display()))?;
            canonical::extract_canonical_text_model(&bytes, document_id)?
        }
        (None, Some(path)) => canonical::extract_canonical_text_fixture(&path, document_id)?,
        (None, None) => {
            print_usage();
            bail!("provide a PDF path or --fixture path")
        }
        (Some(_), Some(_)) => bail!("provide either a PDF path or --fixture, not both"),
    };

    if sentences {
        let units = canonical::generate_canonical_reading_units(&model)?;
        print_sentence_diagnostics(&model, &units)?;
    } else if compact {
        println!("{}", serde_json::to_string(&model)?);
    } else {
        println!("{}", serde_json::to_string_pretty(&model)?);
    }
    Ok(())
}

fn print_sentence_diagnostics(
    model: &models::CanonicalTextModel,
    units: &[models::CanonicalReadingUnit],
) -> anyhow::Result<()> {
    let mut per_page = model
        .pages
        .iter()
        .map(|page| (page.page, 0_usize))
        .collect::<BTreeMap<_, _>>();
    let lengths = units
        .iter()
        .map(|unit| unit.normalized_text.encode_utf16().count())
        .collect::<Vec<_>>();
    for unit in units {
        *per_page.entry(unit.page).or_default() += 1;
    }
    let average = if lengths.is_empty() {
        0.0
    } else {
        lengths.iter().sum::<usize>() as f64 / lengths.len() as f64
    };
    let kinds = [
        models::CandidateKind::BodySentence,
        models::CandidateKind::Heading,
        models::CandidateKind::Metadata,
        models::CandidateKind::Reference,
        models::CandidateKind::FormulaOrTable,
        models::CandidateKind::ShortFragment,
        models::CandidateKind::Unknown,
    ];
    let kind_counts = units.iter().fold(BTreeMap::new(), |mut counts, unit| {
        *counts.entry(unit.candidate_kind).or_insert(0_usize) += 1;
        counts
    });

    println!("Canonical sentence diagnostics");
    println!("Pages: {}", model.pages.len());
    println!("Total sentence candidates: {}", units.len());
    println!(
        "Sentences per page: {}",
        per_page
            .iter()
            .map(|(page, count)| format!("{page}={count}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!("Average sentence length: {average:.1}");
    println!(
        "Candidates longer than 500 chars: {}",
        lengths.iter().filter(|length| **length > 500).count()
    );
    println!(
        "Candidates shorter than 30 chars: {}",
        lengths.iter().filter(|length| **length < 30).count()
    );
    println!(
        "Candidate kinds: {}",
        kinds
            .iter()
            .map(|kind| format!(
                "{}={}",
                candidate_kind_name(*kind),
                kind_counts.get(kind).copied().unwrap_or_default()
            ))
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!("Short candidate examples:");
    for unit in units
        .iter()
        .filter(|unit| unit.candidate_kind == models::CandidateKind::ShortFragment)
        .take(5)
    {
        println!(
            "- {} page={} text={:?}",
            unit.sentence_id, unit.page, unit.text
        );
    }
    println!("Metadata/heading/reference examples:");
    for kind in [
        models::CandidateKind::Metadata,
        models::CandidateKind::Heading,
        models::CandidateKind::Reference,
    ] {
        for unit in units
            .iter()
            .filter(|unit| unit.candidate_kind == kind)
            .take(3)
        {
            println!(
                "- {} kind={} page={} text={:?}",
                unit.sentence_id,
                candidate_kind_name(unit.candidate_kind),
                unit.page,
                unit.text
            );
        }
    }
    println!("Body sentence examples:");
    for unit in units
        .iter()
        .filter(|unit| unit.candidate_kind == models::CandidateKind::BodySentence)
        .take(5)
    {
        println!(
            "- {} page={} text={:?} item_ranges={}",
            unit.sentence_id,
            unit.page,
            unit.text,
            serde_json::to_string(&unit.item_ranges)?
        );
    }
    Ok(())
}

fn candidate_kind_name(kind: models::CandidateKind) -> &'static str {
    match kind {
        models::CandidateKind::BodySentence => "body_sentence",
        models::CandidateKind::Heading => "heading",
        models::CandidateKind::Metadata => "metadata",
        models::CandidateKind::Reference => "reference",
        models::CandidateKind::FormulaOrTable => "formula_or_table",
        models::CandidateKind::ShortFragment => "short_fragment",
        models::CandidateKind::Unknown => "unknown",
    }
}

fn required_value(args: &mut impl Iterator<Item = String>, option: &str) -> anyhow::Result<String> {
    args.next()
        .with_context(|| format!("{option} requires a value"))
}

fn print_usage() {
    eprintln!(
        "Usage: canonical-text <document.pdf> [--document-id UUID] [--compact|--sentences]\n       canonical-text --fixture <text-content.json> [--document-id UUID] [--compact|--sentences]"
    );
}
