import { readFileSync } from "node:fs";
import type { AnnotationReaderCapture } from "../annotationDebug.js";
import { buildCapturedReaderReport, buildFixtureReport } from "./annotationFixture.js";

declare const process: {
  argv: string[];
  stdout: { write(value: string): void };
};

const fixtureArgument = process.argv.find((argument) => argument.startsWith("--fixture="));
const fixtureIndex = process.argv.indexOf("--fixture");
const readerStateArgument = process.argv.find((argument) => argument.startsWith("--reader-state="));
const readerStateIndex = process.argv.indexOf("--reader-state");
const readerStatePath =
  readerStateArgument?.slice("--reader-state=".length) ??
  (readerStateIndex >= 0 ? process.argv[readerStateIndex + 1] : undefined);
const fixtureKey =
  fixtureArgument?.slice("--fixture=".length) ??
  (fixtureIndex >= 0 ? process.argv[fixtureIndex + 1] : undefined) ??
  "deterministic";
const report = readerStatePath
  ? buildCapturedReaderReport(
      JSON.parse(readFileSync(readerStatePath, "utf8")) as AnnotationReaderCapture,
      readerStatePath
    )
  : buildFixtureReport(fixtureKey);

if (process.argv.includes("--json")) {
  process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
} else {
  const summary = report.summary;
  console.log(`Annotation diagnostics: ${report.fixture} (${report.mode})`);
  console.log(`Fixture: ${report.fixtureKey} | Type: ${report.fixtureType} | Pages: ${report.pageCount}`);
  console.log(`Generated: ${summary.generated}`);
  console.log(`Validated: ${summary.validated} (invalid: ${summary.invalid})`);
  console.log(`Eligible after filters: ${summary.eligibleAfterFilters}`);
  console.log(`Intended inline: ${summary.intendedInline}`);
  console.log(`Match attempted: ${summary.matchAttempted}`);
  console.log(`Candidate matches: ${summary.candidateMatches}`);
  console.log(`canonical_direct_range_available: ${summary.canonicalDirectRangeAvailable}`);
  console.log(`rendered_by_direct_range: ${summary.renderedByDirectRange}`);
  console.log(`rendered_by_rect_overlay: ${summary.renderedByRectOverlay}`);
  console.log(`direct_range_missing_span: ${summary.directRangeMissingSpan}`);
  console.log(`direct_range_invalid_item: ${summary.directRangeInvalidItem}`);
  console.log(`legacy_text_search_used: ${summary.legacyTextSearchUsed}`);
  console.log(`fuzzy_repair_used: ${summary.fuzzyRepairUsed}`);
  console.log(
    `Usable matches (ratio >= ${(report.minimumUsableMatchRatio * 100).toFixed(0)}%): ${summary.usableMatches}`
  );
  console.log(`Rendered inline: ${summary.renderedInline}`);
  console.log(
    `Inline fallback without usable match: ${summary.inlineFallbackWithoutUsableMatch}`
  );
  console.log(`Rendered as page note: ${summary.renderedAsPageNote}`);
  console.log(`Intentionally left normal: ${summary.leftNormal}`);
  console.log(`Dropped: ${summary.dropped}`);
  console.log("\nAI output:");
  console.log(`Average anchor length: ${report.aiOutput.averageAnchorLength}`);
  console.log(`Anchors longer than 500 characters: ${report.aiOutput.anchorsLongerThan500Characters}`);
  console.log(`Anchors shorter than 30 characters: ${report.aiOutput.anchorsShorterThan30Characters}`);
  console.log(`Complete sentence ratio: ${(report.aiOutput.completeSentenceRatio * 100).toFixed(1)}%`);
  console.log(`Annotation kinds: ${formatDistribution(report.aiOutput.annotationKindDistribution)}`);
  console.log(
    `Canonical sentence annotations: ${report.aiOutput.annotationKindDistribution.canonical_sentence_annotation ?? 0}`
  );
  console.log(
    `Legacy chunk annotations: ${report.aiOutput.annotationKindDistribution.legacy_chunk_annotation ?? 0}`
  );
  console.log(`Missing item ranges: ${report.aiOutput.missingItemRanges}`);
  console.log(
    `Selected candidate kinds: ${formatDistribution(report.aiOutput.selectedCandidateKindDistribution)}`
  );
  console.log(`Average source text length: ${report.aiOutput.averageSourceTextLength}`);
  console.log(`Shorter than 30 characters: ${report.aiOutput.shorterThan30Characters}`);
  console.log(`Not complete sentences: ${report.aiOutput.notCompleteSentences}`);
  console.log(`Directives: ${formatDistribution(report.aiOutput.directiveDistribution)}`);
  console.log(`Priorities: ${formatDistribution(report.aiOutput.priorityDistribution)}`);
  console.log(`Annotations per page: ${formatDistribution(report.aiOutput.annotationsPerPage)}`);
  if (report.textSourceIntegrity) {
    const integrity = report.textSourceIntegrity;
    console.log("\nText-source integrity:");
    console.log(`Pages missing PDF.js textContent: ${integrity.summary.pagesMissingPdfjsTextContent}`);
    console.log(`Intended inline found in backend text: ${integrity.summary.intendedInlineFoundInBackendText}`);
    console.log(`Intended inline found in PDF.js page text: ${integrity.summary.intendedInlineFoundInPdfjsPageText}`);
    console.log(`Intended inline not found anywhere: ${integrity.summary.intendedInlineNotFoundAnywhere}`);
    console.log(`Anchors with possible page mismatch: ${integrity.summary.anchorsWithPossiblePageMismatch}`);
    console.log("\nPDF.js pages:");
    for (const page of integrity.pages) {
      console.log(
        `- page ${page.pageNumber}: available=${page.hasTextContent} items=${page.textItemCount} raw=${page.rawTextLength} normalized=${page.normalizedTextLength} preview="${page.first120NormalizedCharacters}"`
      );
    }
    console.log("\nUnmatched intended-inline anchors:");
    for (const anchor of integrity.unmatchedAnchors) {
      console.log(
        `- ${anchor.anchorId} page=${anchor.page} root_cause=${anchor.rootCause} final=${anchor.finalStatus} final_drop_reason=${anchor.finalDropReason ?? "-"}`
      );
      console.log(
        `  lengths: raw=${anchor.anchorLength} normalized=${anchor.anchorNormalizedLength} backend=${anchor.existsInBackendChunk} pdfjs_page=${anchor.existsInPdfjsPageText} other_page=${anchor.possiblePageMatch ?? "-"}`
      );
      console.log(
        `  fuzzy=${anchor.bestSimilarityScore.toFixed(3)} anchor="${anchor.anchorNormalizedPreview}" nearest="${anchor.bestPageCandidatePreview}"`
      );
    }
  }
  console.log("\nDrop reasons:");
  for (const [reason, count] of Object.entries(summary.dropReasons).sort(([left], [right]) =>
    left.localeCompare(right)
  )) {
    console.log(`- ${reason}: ${count}`);
  }

  for (const page of report.pages) {
    console.log(
      `\nPage ${page.pageNumber}: generated=${page.counts.generated} intended=${page.counts.intendedInline} eligible=${page.counts.eligibleAfterFilters} attempted=${page.counts.matchAttempted} candidates=${page.counts.candidateMatches} usable=${page.counts.usableMatches} inline=${page.counts.renderedInline} fallback=${page.counts.inlineFallbackWithoutUsableMatch} note=${page.counts.renderedAsPageNote} normal=${page.counts.leftNormal} dropped=${page.counts.dropped}`
    );
    console.log(
      "chunk id                 kind                    directive   priority conf source chars match ratio valid     intended eligible   attempted match           final status                  drop reason"
    );
    for (const annotation of page.annotations) {
      console.log(
        [
          annotation.chunkId.padEnd(24),
          annotation.annotationKind.padEnd(23),
          annotation.directive.padEnd(11),
          annotation.priority.padEnd(8),
          annotation.confidence.toFixed(2).padStart(4),
          annotation.source.padEnd(6),
          String(annotation.sourceTextLength).padStart(5),
          String(annotation.matchedTextLength).padStart(5),
          `${(annotation.matchedCharRatio * 100).toFixed(1)}%`.padStart(6),
          annotation.validationStatus.padEnd(9),
          (annotation.intendedInline ? "yes" : "no").padEnd(8),
          annotation.eligibilityStatus.padEnd(10),
          (annotation.matchAttempted ? "yes" : "no").padEnd(9),
          annotation.matchStatus.padEnd(13),
          annotation.finalStatus.padEnd(29),
          annotation.finalStatus === "dropped" ? annotation.primaryDropReason : "-"
        ].join(" ")
      );
    }
  }
}

function formatDistribution(values: Record<string, number>): string {
  const entries = Object.entries(values);
  if (entries.length === 0) return "(none)";
  return entries
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([key, value]) => `${key}=${value}`)
    .join(", ");
}
