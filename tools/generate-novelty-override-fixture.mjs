import { writeFileSync } from "node:fs";

const output = new URL(
  "../resource/product-fixtures/experienced-retrieval-novelty-override/document.pdf",
  import.meta.url
);

const pages = [
  [
    "Novelty Override Failures.",
    "The previously validated configuration remains the production baseline for normal request traffic.",
    "The previously validated configuration failed when burst traffic introduced a new skewed tenant mix.",
    "The adaptive freshness budget regressed for the new archival workload and increased stale answers by 18 percent.",
    "The familiar reranking setup produced a new latency quality tradeoff, increasing recall by 6 percent but doubling tail latency."
  ],
  [
    "Novelty Override Conditions.",
    "Standard hybrid retrieval combines lexical and semantic candidates during routine operation.",
    "Unlike the baseline, standard hybrid retrieval under the distribution shift reduced rare-query recall by 14 percent.",
    "The adaptive freshness budget was already validated for the ordinary workload and adds no new evidence.",
    "Vendor pricing lists annual support tiers for the deployment."
  ]
];

function escapePdfText(value) {
  return value.replaceAll("\\", "\\\\").replaceAll("(", "\\(").replaceAll(")", "\\)");
}

const objects = [];
const pageObjectIds = pages.map((_, index) => 3 + index);
const fontObjectId = 3 + pages.length;
const contentObjectIds = pages.map((_, index) => fontObjectId + 1 + index);
objects[1] = "<< /Type /Catalog /Pages 2 0 R >>";
objects[2] = `<< /Type /Pages /Kids [${pageObjectIds.map((id) => `${id} 0 R`).join(" ")}] /Count ${pages.length} >>`;
for (const [index, pageObjectId] of pageObjectIds.entries()) {
  objects[pageObjectId] = `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 ${fontObjectId} 0 R >> >> /Contents ${contentObjectIds[index]} 0 R >>`;
}
objects[fontObjectId] = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>";
for (const [index, contentObjectId] of contentObjectIds.entries()) {
  const commands = pages[index]
    .map((line, lineIndex) => `${lineIndex === 0 ? "" : "0 -42 Td\n"}(${escapePdfText(line)}) Tj`)
    .join("\n");
  const stream = `/F1 8 Tf\n72 740 Td\n${commands}\n`;
  objects[contentObjectId] = `<< /Length ${Buffer.byteLength(stream)} >>\nstream\n${stream}endstream`;
}

let pdf = "%PDF-1.4\n%1234\n";
const offsets = [0];
for (let id = 1; id < objects.length; id += 1) {
  offsets[id] = Buffer.byteLength(pdf);
  pdf += `${id} 0 obj\n${objects[id]}\nendobj\n`;
}
const xrefOffset = Buffer.byteLength(pdf);
pdf += `xref\n0 ${objects.length}\n0000000000 65535 f \n`;
for (let id = 1; id < objects.length; id += 1) {
  pdf += `${String(offsets[id]).padStart(10, "0")} 00000 n \n`;
}
pdf += `trailer\n<< /Size ${objects.length} /Root 1 0 R >>\nstartxref\n${xrefOffset}\n%%EOF\n`;
writeFileSync(output, pdf);
