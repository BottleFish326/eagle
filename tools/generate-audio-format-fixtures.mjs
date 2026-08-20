import assert from "node:assert/strict";
import { createHash, randomUUID } from "node:crypto";
import { lstat, mkdir, open, readFile, rename, unlink } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repository = path.resolve(import.meta.dirname, "..");
const sourceRoot = path.join(repository, "fixtures", "formats", "sources");

export const GENERATED_AUDIO_FIXTURES = Object.freeze([
  "audio/minimal.mp3",
  "audio/cover.mp3",
  "audio/minimal.wav",
  "audio/minimal.flac",
  "audio/cover.flac",
  "audio/truncated.mp3",
  "audio/png-disguised-as-mp3.mp3",
  "audio/mp3-disguised-as-wav.wav",
  "audio/unknown-codec.wav",
  "audio/oversized-cover.mp3",
]);

const concat = (...parts) => Buffer.concat(parts.flat());

function unsigned(value, bytes, littleEndian = false) {
  const result = Buffer.alloc(bytes);
  let remaining = BigInt(value);
  for (let offset = 0; offset < bytes; offset += 1) {
    const index = littleEndian ? offset : bytes - offset - 1;
    result[index] = Number(remaining & 0xffn);
    remaining >>= 8n;
  }
  assert.equal(remaining, 0n, `integer does not fit in ${bytes} bytes`);
  return result;
}

const be32 = (value) => unsigned(value, 4);
const le16 = (value) => unsigned(value, 2, true);
const le32 = (value) => unsigned(value, 4, true);

function synchsafe(value) {
  assert.ok(value >= 0 && value <= 0x0fff_ffff);
  return Buffer.from([
    (value >>> 21) & 0x7f,
    (value >>> 14) & 0x7f,
    (value >>> 7) & 0x7f,
    value & 0x7f,
  ]);
}

function mp3Frames(count = 20) {
  const frames = [];
  for (let index = 0; index < count; index += 1) {
    const size = index % 2 === 0 ? 417 : 418;
    frames.push(
      concat(Buffer.from([0xff, 0xfb, 0x90, 0x00]), Buffer.alloc(size - 4)),
    );
  }
  return concat(frames);
}

function id3Cover(png) {
  const payload = concat(
    Buffer.from([0]),
    Buffer.from("image/png\0", "ascii"),
    Buffer.from([3, 0]),
    png,
  );
  const frame = concat(
    Buffer.from("APIC", "ascii"),
    be32(payload.length),
    Buffer.alloc(2),
    payload,
  );
  return concat(
    Buffer.from("ID3\x03\x00\x00", "binary"),
    synchsafe(frame.length),
    frame,
  );
}

function wav({
  format = 1,
  sampleRate = 8_000,
  channels = 1,
  bitDepth = 16,
} = {}) {
  const sampleBytes = bitDepth / 8;
  const data = Buffer.alloc(sampleRate * channels * sampleBytes);
  const fmt = concat(
    le16(format),
    le16(channels),
    le32(sampleRate),
    le32(sampleRate * channels * sampleBytes),
    le16(channels * sampleBytes),
    le16(bitDepth),
  );
  const body = concat(
    Buffer.from("WAVEfmt ", "ascii"),
    le32(fmt.length),
    fmt,
    Buffer.from("data", "ascii"),
    le32(data.length),
    data,
  );
  return concat(Buffer.from("RIFF", "ascii"), le32(body.length), body);
}

function flacStreamInfo() {
  const info = Buffer.alloc(34);
  unsigned(4_096, 2).copy(info, 0);
  unsigned(4_096, 2).copy(info, 2);
  const packed = (8_000n << 44n) | (0n << 41n) | (15n << 36n) | 8_000n;
  unsigned(packed, 8).copy(info, 10);
  return info;
}

function flacBlock(type, last, bytes) {
  assert.ok(bytes.length <= 0xff_ffff);
  return concat(
    Buffer.from([(last ? 0x80 : 0) | type]),
    unsigned(bytes.length, 3),
    bytes,
  );
}

function flacPicture(png) {
  const mime = Buffer.from("image/png", "ascii");
  return concat(
    be32(3),
    be32(mime.length),
    mime,
    be32(0),
    be32(16),
    be32(16),
    be32(32),
    be32(0),
    be32(png.length),
    png,
  );
}

export function buildAudioFormatFixtures(referencePng) {
  const plainMp3 = mp3Frames();
  return new Map([
    ["audio/minimal.mp3", plainMp3],
    ["audio/cover.mp3", concat(id3Cover(referencePng), plainMp3)],
    ["audio/minimal.wav", wav()],
    [
      "audio/minimal.flac",
      concat(Buffer.from("fLaC"), flacBlock(0, true, flacStreamInfo())),
    ],
    [
      "audio/cover.flac",
      concat(
        Buffer.from("fLaC"),
        flacBlock(0, false, flacStreamInfo()),
        flacBlock(6, true, flacPicture(referencePng)),
      ),
    ],
    [
      "audio/truncated.mp3",
      Buffer.from("ID3\x03\x00\x00\x00\x00\x00\x10", "binary"),
    ],
    ["audio/png-disguised-as-mp3.mp3", referencePng],
    ["audio/mp3-disguised-as-wav.wav", plainMp3],
    ["audio/unknown-codec.wav", wav({ format: 0xffff })],
    [
      "audio/oversized-cover.mp3",
      concat(
        Buffer.from("ID3\x03\x00\x00", "binary"),
        synchsafe(16 * 1024 * 1024 + 1),
      ),
    ],
  ]);
}

export async function generateAudioFormatFixtures({
  replaceExisting = false,
} = {}) {
  const referencePng = await readFile(
    path.join(
      repository,
      "fixtures",
      "formats",
      "references",
      "svg",
      "minimal.png",
    ),
  );
  const fixtures = buildAudioFormatFixtures(referencePng);
  for (const relativePath of GENERATED_AUDIO_FIXTURES) {
    await writeFixedFixture(
      relativePath,
      fixtures.get(relativePath),
      replaceExisting,
    );
  }
  return fixtures;
}

async function writeFixedFixture(relativePath, bytes, replaceExisting) {
  assert.ok(bytes, `missing generated bytes for ${relativePath}`);
  const target = path.resolve(sourceRoot, relativePath);
  if (!target.startsWith(`${sourceRoot}${path.sep}`)) {
    throw new Error(`generated fixture escapes source root: ${relativePath}`);
  }
  await mkdir(path.dirname(target), { recursive: true });
  try {
    const metadata = await lstat(target);
    if (metadata.isSymbolicLink() || !metadata.isFile()) {
      throw new Error(
        `generated fixture target is not a regular file: ${relativePath}`,
      );
    }
    const existing = await readFile(target);
    if (!existing.equals(bytes) && !replaceExisting) {
      throw new Error(
        `generated fixture differs from deterministic output: ${relativePath}`,
      );
    }
    if (existing.equals(bytes)) return;
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
  }
  const temporary = `${target}.generate-${process.pid}-${randomUUID()}.tmp`;
  let temporaryOwned = false;
  try {
    const handle = await open(temporary, "wx", 0o644);
    temporaryOwned = true;
    try {
      await handle.writeFile(bytes);
      await handle.sync();
    } finally {
      await handle.close();
    }
    await rename(temporary, target);
    temporaryOwned = false;
  } finally {
    if (temporaryOwned) await unlink(temporary).catch(() => {});
  }
}

if (path.resolve(process.argv[1] ?? "") === fileURLToPath(import.meta.url)) {
  const arguments_ = process.argv.slice(2);
  if (arguments_.some((argument) => argument !== "--replace")) {
    throw new Error(
      "usage: node tools/generate-audio-format-fixtures.mjs [--replace]",
    );
  }
  const fixtures = await generateAudioFormatFixtures({
    replaceExisting: arguments_.includes("--replace"),
  });
  console.log(
    JSON.stringify(
      {
        schema: 1,
        fixtures: [...fixtures].map(([fixturePath, bytes]) => ({
          path: `sources/${fixturePath}`,
          size: bytes.length,
          sha256: createHash("sha256").update(bytes).digest("hex"),
        })),
      },
      null,
      2,
    ),
  );
}
