import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

import {
  GENERATED_AVIF_FIXTURES,
  buildAdversarialAvifFixtures,
} from "./generate-adversarial-format-fixtures.mjs";

const repository = path.resolve(import.meta.dirname, "..");

test("derives the tracked adversarial AVIF fixtures without changing the pinned source", async () => {
  const sourceAvif = await readFile(
    path.join(repository, "fixtures/formats/sources/avif/libheif-example.avif"),
  );
  const referencePng = await readFile(
    path.join(repository, "fixtures/formats/references/svg/minimal.png"),
  );
  const sourceSnapshot = Buffer.from(sourceAvif);
  const fixtures = buildAdversarialAvifFixtures({
    sourceAvif,
    referencePng,
  });

  assert.deepEqual([...fixtures.keys()], GENERATED_AVIF_FIXTURES);
  assert.deepEqual(sourceAvif, sourceSnapshot);
  assert.ok(
    fixtures
      .get("avif/corrupted-bitstream.avif")
      .subarray(280)
      .every((byte) => byte === 0),
  );
  assert.equal(fixtures.get("avif/truncated-ftyp.avif").length, 24);
  assert.deepEqual(
    fixtures.get("avif/png-disguised-as-avif.avif"),
    referencePng,
  );
  assert.equal(
    fixtures.get("avif/unknown-codec.avif").toString("ascii", 178, 182),
    "jpeg",
  );
  assert.equal(
    fixtures.get("avif/oversized-ispe.avif").readUInt32BE(242),
    65_536,
  );
  assert.deepEqual(fixtures.get("avif/avif-disguised-as-jpeg.jpg"), sourceAvif);
  assert.deepEqual(
    fixtures.get("avif/resource-limited-output.avif"),
    sourceAvif,
  );
});

test("tracked generated fixtures exactly match deterministic output", async () => {
  const sourceAvif = await readFile(
    path.join(repository, "fixtures/formats/sources/avif/libheif-example.avif"),
  );
  const referencePng = await readFile(
    path.join(repository, "fixtures/formats/references/svg/minimal.png"),
  );
  const fixtures = buildAdversarialAvifFixtures({
    sourceAvif,
    referencePng,
  });

  for (const [relativePath, expected] of fixtures) {
    const tracked = await readFile(
      path.join(repository, "fixtures/formats/sources", relativePath),
    );
    assert.deepEqual(tracked, expected, relativePath);
  }
});
