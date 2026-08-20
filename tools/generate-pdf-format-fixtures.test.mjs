import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

import {
  GENERATED_PDF_FIXTURES,
  buildPdfFormatFixtures,
} from "./generate-pdf-format-fixtures.mjs";

const repository = path.resolve(import.meta.dirname, "..");

async function fixtures() {
  const referencePng = await readFile(
    path.join(repository, "fixtures/formats/references/svg/minimal.png"),
  );
  return buildPdfFormatFixtures(referencePng);
}

test("builds classic normal and adversarial PDF structures deterministically", async () => {
  const generated = await fixtures();
  assert.deepEqual([...generated.keys()], GENERATED_PDF_FIXTURES);
  assert.ok(generated.get("pdf/minimal.pdf").includes("xref"));
  assert.ok(generated.get("pdf/active-javascript.pdf").includes("/JavaScript"));
  assert.ok(generated.get("pdf/encrypted.pdf").includes("/Encrypt"));
  assert.ok(generated.get("pdf/object-stream.pdf").includes("/ObjStm"));
  assert.ok(!generated.get("pdf/truncated.pdf").includes("%%EOF"));
});

test("tracked PDF fixtures exactly match deterministic output", async () => {
  const generated = await fixtures();
  for (const [relativePath, expected] of generated) {
    const tracked = await readFile(
      path.join(repository, "fixtures/formats/sources", relativePath),
    );
    assert.deepEqual(tracked, expected, relativePath);
  }
});
