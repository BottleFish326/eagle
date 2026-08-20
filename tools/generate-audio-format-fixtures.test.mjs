import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

import {
  GENERATED_AUDIO_FIXTURES,
  buildAudioFormatFixtures,
} from "./generate-audio-format-fixtures.mjs";

const repository = path.resolve(import.meta.dirname, "..");

async function fixtures() {
  const referencePng = await readFile(
    path.join(repository, "fixtures/formats/references/svg/minimal.png"),
  );
  return buildAudioFormatFixtures(referencePng);
}

test("builds bounded normal and adversarial audio containers deterministically", async () => {
  const generated = await fixtures();
  assert.deepEqual([...generated.keys()], GENERATED_AUDIO_FIXTURES);
  assert.ok(generated.get("audio/cover.mp3").includes("APIC"));
  assert.ok(generated.get("audio/minimal.wav").includes("WAVE"));
  assert.ok(generated.get("audio/cover.flac").includes("fLaC"));
  assert.equal(generated.get("audio/truncated.mp3").length, 10);
});

test("tracked audio fixtures exactly match deterministic output", async () => {
  const generated = await fixtures();
  for (const [relativePath, expected] of generated) {
    const tracked = await readFile(
      path.join(repository, "fixtures/formats/sources", relativePath),
    );
    assert.deepEqual(tracked, expected, relativePath);
  }
});
