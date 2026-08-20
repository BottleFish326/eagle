import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

import {
  GENERATED_VIDEO_FIXTURES,
  buildVideoFormatFixtures,
} from "./generate-video-format-fixtures.mjs";

const repository = path.resolve(import.meta.dirname, "..");

async function fixtures() {
  const referencePng = await readFile(
    path.join(repository, "fixtures/formats/references/svg/minimal.png"),
  );
  return buildVideoFormatFixtures(referencePng);
}

test("builds bounded normal and adversarial video containers deterministically", async () => {
  const generated = await fixtures();
  assert.deepEqual([...generated.keys()], GENERATED_VIDEO_FIXTURES);
  assert.ok(generated.get("video/minimal.mp4").includes("moov"));
  assert.ok(generated.get("video/minimal.mov").includes("qt  "));
  assert.ok(generated.get("video/minimal.webm").includes("webm"));
  assert.equal(generated.get("video/truncated.mp4").length, 24);
  assert.ok(generated.get("video/unknown-codec.mp4").includes("zzzz"));
  assert.deepEqual(
    generated.get("video/mp4-disguised-as-webm.webm"),
    generated.get("video/minimal.mp4"),
  );
});

test("tracked video fixtures exactly match deterministic output", async () => {
  const generated = await fixtures();
  for (const [relativePath, expected] of generated) {
    const tracked = await readFile(
      path.join(repository, "fixtures/formats/sources", relativePath),
    );
    assert.deepEqual(tracked, expected, relativePath);
  }
});
