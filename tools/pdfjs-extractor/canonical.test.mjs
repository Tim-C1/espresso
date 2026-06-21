import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { extractFixture, normalizeItemText } from "./canonical.mjs";

const fixtureUrl = new URL("./fixtures/text-content.json", import.meta.url);

test("normalization is stable for whitespace and compatibility characters", () => {
  assert.equal(normalizeItemText("  A\u00a0  B  "), "A B");
  assert.equal(normalizeItemText("café"), "café");
  assert.equal(normalizeItemText("soft\u00adhyphen"), "softhyphen");
});

test("fixture produces stable canonical shape and offsets", async () => {
  const bytes = await readFile(fixtureUrl);
  const first = await extractFixture(bytes, "00000000-0000-0000-0000-000000000000");
  const second = await extractFixture(bytes, "00000000-0000-0000-0000-000000000000");

  assert.deepEqual(first, second);
  assert.equal(first.schema_version, "1.0");
  assert.equal(first.pdf_hash.length, 64);
  assert.equal(first.pages[0].normalized_text, "A transformer spans lines. Repeated phrase. Repeated phrase.");
  assert.equal(first.pages[0].text_items[1].raw_start, 2);
  assert.equal(first.pages[0].text_items[1].raw_end, 8);
  assert.equal(first.pages[0].text_items[1].normalized_start, 2);
  assert.equal(first.pages[0].text_items[1].normalized_end, 7);
  assert.equal(first.pages[0].text_items[4].bbox.x, 110);
  assert.equal(first.pages[1].normalized_text, "Unicode café text.");
});
