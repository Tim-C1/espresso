import { createHash } from "node:crypto";
import { createRequire } from "node:module";
import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

export const SCHEMA_VERSION = "1.0";
export const EXTRACTOR_OPTIONS = Object.freeze({
  disable_font_face: true,
  include_marked_content: false,
  use_system_fonts: false,
});

const here = dirname(fileURLToPath(import.meta.url));
const frontendDirectory = resolve(here, "../../frontend");

function utf16Length(value) {
  return value.length;
}

export function normalizeItemText(value) {
  return value
    .normalize("NFKC")
    .replaceAll("\u00ad", "")
    .replace(/\s+/gu, " ")
    .trim();
}

export function buildCanonicalTextModel({
  documentId,
  pdfHash,
  extractorVersion,
  pages,
}) {
  return {
    schema_version: SCHEMA_VERSION,
    document_id: documentId,
    pdf_hash: pdfHash,
    extractor: {
      name: "pdfjs-dist",
      version: extractorVersion,
      options: EXTRACTOR_OPTIONS,
    },
    pages: pages.map((pageItems, pageIndex) => buildPage(pageIndex + 1, pageItems)),
  };
}

function buildPage(pageNumber, sourceItems) {
  let rawText = "";
  let normalizedText = "";
  const textItems = [];

  for (const source of sourceItems) {
    if (typeof source.str !== "string") continue;

    const rawStart = utf16Length(rawText);
    rawText += source.str;
    const rawEnd = utf16Length(rawText);
    const hasEol = Boolean(source.hasEOL ?? source.has_eol);
    let normalized = normalizeItemText(source.str);

    const previous = textItems.at(-1);
    const joinsHyphenatedLine =
      previous?.has_eol &&
      previous.normalized_str.endsWith("-") &&
      normalized.length > 0;

    if (joinsHyphenatedLine) {
      previous.normalized_str = previous.normalized_str.slice(0, -1);
      previous.normalized_end -= 1;
      normalizedText = normalizedText.slice(0, -1);
    } else if (normalized && normalizedText && !normalizedText.endsWith(" ")) {
      normalizedText += " ";
    }

    const normalizedStart = utf16Length(normalizedText);
    normalizedText += normalized;
    const normalizedEnd = utf16Length(normalizedText);
    const transform = Array.from(source.transform ?? [1, 0, 0, 1, 0, 0], Number);
    if (transform.length !== 6 || transform.some((value) => !Number.isFinite(value))) {
      throw new Error(`page ${pageNumber} contains an invalid PDF.js transform`);
    }

    textItems.push({
      item_id: `p${pageNumber}i${textItems.length + 1}`,
      page: pageNumber,
      str: source.str,
      normalized_str: normalized,
      raw_start: rawStart,
      raw_end: rawEnd,
      normalized_start: normalizedStart,
      normalized_end: normalizedEnd,
      transform,
      width: finiteNumber(source.width, "width", pageNumber),
      height: finiteNumber(source.height, "height", pageNumber),
      has_eol: hasEol,
      bbox: normalizeBbox(source.bbox, pageNumber),
    });

    if (hasEol) rawText += "\n";
  }

  return {
    page: pageNumber,
    raw_text: rawText,
    normalized_text: normalizedText,
    text_items: textItems,
  };
}

function finiteNumber(value, field, pageNumber) {
  const number = Number(value ?? 0);
  if (!Number.isFinite(number)) {
    throw new Error(`page ${pageNumber} contains an invalid ${field}`);
  }
  return number;
}

function normalizeBbox(value, pageNumber) {
  if (value == null) return null;
  return {
    x: finiteNumber(value.x, "bbox.x", pageNumber),
    y: finiteNumber(value.y, "bbox.y", pageNumber),
    width: finiteNumber(value.width, "bbox.width", pageNumber),
    height: finiteNumber(value.height, "bbox.height", pageNumber),
  };
}

export async function extractPdf(pdfBytes, documentId) {
  const { pdfjs, version } = await loadPdfJs();
  const loadingTask = pdfjs.getDocument({
    data: new Uint8Array(pdfBytes),
    disableFontFace: EXTRACTOR_OPTIONS.disable_font_face,
    useSystemFonts: EXTRACTOR_OPTIONS.use_system_fonts,
  });
  const document = await loadingTask.promise;
  try {
    const pages = [];
    for (let pageNumber = 1; pageNumber <= document.numPages; pageNumber += 1) {
      const page = await document.getPage(pageNumber);
      const content = await page.getTextContent({
        includeMarkedContent: EXTRACTOR_OPTIONS.include_marked_content,
      });
      pages.push(content.items.filter((item) => typeof item.str === "string"));
      page.cleanup();
    }
    return buildCanonicalTextModel({
      documentId,
      pdfHash: sha256(pdfBytes),
      extractorVersion: version,
      pages,
    });
  } finally {
    await document.destroy();
  }
}

export async function extractFixture(fixtureBytes, documentId) {
  const fixture = JSON.parse(fixtureBytes.toString("utf8"));
  return buildCanonicalTextModel({
    documentId,
    pdfHash: sha256(fixtureBytes),
    extractorVersion: fixture.extractor_version,
    pages: fixture.pages,
  });
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

async function loadPdfJs() {
  const require = createRequire(import.meta.url);
  const packagePath = require.resolve("pdfjs-dist/package.json", {
    paths: [frontendDirectory],
  });
  const packageJson = JSON.parse(await readFile(packagePath, "utf8"));
  const modulePath = resolve(dirname(packagePath), "legacy/build/pdf.mjs");
  return { pdfjs: await import(pathToFileURL(modulePath).href), version: packageJson.version };
}
