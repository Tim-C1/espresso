#!/usr/bin/env node
import { readFile } from "node:fs/promises";
import { stdin, stdout } from "node:process";

import { extractFixture, extractPdf } from "./canonical.mjs";

async function main() {
  const args = process.argv.slice(2);
  let documentId = "00000000-0000-0000-0000-000000000000";
  let fixturePath = null;

  for (let index = 0; index < args.length; index += 1) {
    if (args[index] === "--document-id") {
      documentId = args[++index];
    } else if (args[index] === "--fixture") {
      fixturePath = args[++index];
    } else {
      throw new Error(`unknown option ${args[index]}`);
    }
  }

  if (!documentId) throw new Error("--document-id requires a value");
  const bytes = fixturePath ? await readFile(fixturePath) : await readStdin();
  if (bytes.length === 0) throw new Error("no PDF bytes received on stdin");
  const model = fixturePath
    ? await extractFixture(bytes, documentId)
    : await extractPdf(bytes, documentId);
  stdout.write(`${JSON.stringify(model)}\n`);
}

async function readStdin() {
  const chunks = [];
  for await (const chunk of stdin) chunks.push(chunk);
  return Buffer.concat(chunks);
}

main().catch((error) => {
  console.error(error instanceof Error ? error.stack : String(error));
  process.exitCode = 1;
});
