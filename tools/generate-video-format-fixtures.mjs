import assert from "node:assert/strict";
import { createHash, randomUUID } from "node:crypto";
import { lstat, mkdir, open, readFile, rename, unlink } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repository = path.resolve(import.meta.dirname, "..");
const sourceRoot = path.join(repository, "fixtures", "formats", "sources");

export const GENERATED_VIDEO_FIXTURES = Object.freeze([
  "video/minimal.mp4",
  "video/minimal.mov",
  "video/minimal.webm",
  "video/truncated.mp4",
  "video/png-disguised-as-mp4.mp4",
  "video/mp4-disguised-as-webm.webm",
  "video/unknown-codec.mp4",
  "video/oversized-duration.mp4",
  "video/oversized-dimensions.webm",
]);

const concat = (...parts) => Buffer.concat(parts.flat());

function unsigned(value, bytes) {
  assert.ok(Number.isSafeInteger(value) && value >= 0);
  const result = Buffer.alloc(bytes);
  let remaining = BigInt(value);
  for (let index = bytes - 1; index >= 0; index -= 1) {
    result[index] = Number(remaining & 0xffn);
    remaining >>= 8n;
  }
  assert.equal(remaining, 0n, `integer does not fit in ${bytes} bytes`);
  return result;
}

const u8 = (value) => unsigned(value, 1);
const u16 = (value) => unsigned(value, 2);
const u32 = (value) => unsigned(value, 4);

function box(type, ...payload) {
  assert.match(type, /^[\x20-\x7e]{4}$/u);
  const body = concat(...payload);
  return concat(u32(body.length + 8), Buffer.from(type, "ascii"), body);
}

function fullBox(type, version, flags, ...payload) {
  return box(
    type,
    u8(version),
    Buffer.from([(flags >>> 16) & 0xff, (flags >>> 8) & 0xff, flags & 0xff]),
    ...payload,
  );
}

function identityMatrix() {
  return concat(
    u32(0x0001_0000),
    u32(0),
    u32(0),
    u32(0),
    u32(0x0001_0000),
    u32(0),
    u32(0),
    u32(0),
    u32(0x4000_0000),
  );
}

function movieHeader({ timescale, duration, nextTrackId }) {
  return fullBox(
    "mvhd",
    0,
    0,
    u32(0),
    u32(0),
    u32(timescale),
    u32(duration),
    u32(0x0001_0000),
    u16(0x0100),
    Buffer.alloc(10),
    identityMatrix(),
    Buffer.alloc(24),
    u32(nextTrackId),
  );
}

function trackHeader({ id, duration, width, height, audio }) {
  return fullBox(
    "tkhd",
    0,
    7,
    u32(0),
    u32(0),
    u32(id),
    u32(0),
    u32(duration),
    Buffer.alloc(8),
    u16(0),
    u16(0),
    u16(audio ? 0x0100 : 0),
    u16(0),
    identityMatrix(),
    u32(width << 16),
    u32(height << 16),
  );
}

function mediaHeader({ timescale, duration }) {
  return fullBox(
    "mdhd",
    0,
    0,
    u32(0),
    u32(0),
    u32(timescale),
    u32(duration),
    u16(0x55c4),
    u16(0),
  );
}

function handler(type, name) {
  return fullBox(
    "hdlr",
    0,
    0,
    u32(0),
    Buffer.from(type, "ascii"),
    Buffer.alloc(12),
    Buffer.from(`${name}\0`, "utf8"),
  );
}

function dataInformation() {
  const url = fullBox("url ", 0, 1);
  return box("dinf", fullBox("dref", 0, 0, u32(1), url));
}

function emptySampleTables(sampleEntry, sampleDuration) {
  return box(
    "stbl",
    fullBox("stsd", 0, 0, u32(1), sampleEntry),
    fullBox("stts", 0, 0, u32(1), u32(1), u32(sampleDuration)),
    fullBox("stsc", 0, 0, u32(0)),
    fullBox("stsz", 0, 0, u32(0), u32(0)),
    fullBox("stco", 0, 0, u32(0)),
  );
}

function videoSampleEntry({ codec = "avc1", width, height }) {
  const avcConfiguration = box("avcC", Buffer.from([1, 66, 0, 30]));
  return box(
    codec,
    Buffer.alloc(6),
    u16(1),
    Buffer.alloc(16),
    u16(width),
    u16(height),
    u32(0x0048_0000),
    u32(0x0048_0000),
    u32(0),
    u16(1),
    Buffer.alloc(32),
    u16(0x0018),
    u16(0xffff),
    avcConfiguration,
  );
}

function audioSampleEntry() {
  return box(
    "sowt",
    Buffer.alloc(6),
    u16(1),
    u16(0),
    Buffer.alloc(6),
    u16(2),
    u16(16),
    Buffer.alloc(4),
    u32(48_000 * 65_536),
  );
}

function mediaInformation({ audio, sampleEntry, sampleDuration }) {
  const header = audio
    ? fullBox("smhd", 0, 0, u16(0), u16(0))
    : fullBox("vmhd", 0, 1, u16(0), Buffer.alloc(6));
  return box(
    "minf",
    header,
    dataInformation(),
    emptySampleTables(sampleEntry, sampleDuration),
  );
}

function track({ id, timescale, duration, width, height, audio, codec }) {
  const sampleEntry = audio
    ? audioSampleEntry()
    : videoSampleEntry({ codec, width, height });
  const mdia = box(
    "mdia",
    mediaHeader({ timescale, duration }),
    handler(audio ? "soun" : "vide", audio ? "SoundHandler" : "VideoHandler"),
    mediaInformation({ audio, sampleEntry, sampleDuration: duration }),
  );
  return box("trak", trackHeader({ id, duration, width, height, audio }), mdia);
}

function buildIsoVideo({ brand, durationMs, codec = "avc1" }) {
  const timescale = durationMs > 0xffff_ffff ? 1 : 1_000;
  const duration =
    timescale === 1 ? Math.floor(durationMs / 1_000) : durationMs;
  assert.ok(duration <= 0xffff_fffe);
  const ftyp = box(
    "ftyp",
    Buffer.from(brand, "ascii"),
    u32(0),
    Buffer.from(brand === "qt  " ? "qt  " : "isommp42", "ascii"),
  );
  return concat(
    ftyp,
    box(
      "moov",
      movieHeader({ timescale, duration, nextTrackId: 3 }),
      track({
        id: 1,
        timescale,
        duration,
        width: 320,
        height: 180,
        audio: false,
        codec,
      }),
      track({
        id: 2,
        timescale,
        duration,
        width: 0,
        height: 0,
        audio: true,
      }),
    ),
    box("mdat"),
  );
}

function ebmlSize(size) {
  assert.ok(Number.isSafeInteger(size) && size >= 0);
  for (let width = 1; width <= 8; width += 1) {
    const maximum = 2 ** (7 * width) - 2;
    if (size <= maximum) {
      const result = Buffer.alloc(width);
      let value = BigInt(size) | (1n << BigInt(7 * width));
      for (let index = width - 1; index >= 0; index -= 1) {
        result[index] = Number(value & 0xffn);
        value >>= 8n;
      }
      return result;
    }
  }
  throw new Error("EBML fixture element is too large");
}

function ebmlElement(id, ...payload) {
  const body = concat(...payload);
  return concat(Buffer.from(id, "hex"), ebmlSize(body.length), body);
}

function ebmlUnsigned(id, value) {
  assert.ok(Number.isSafeInteger(value) && value >= 0);
  let width = 1;
  while (value >= 2 ** (8 * width) && width < 8) width += 1;
  return ebmlElement(id, unsigned(value, width));
}

function ebmlFloat(id, value) {
  const bytes = Buffer.alloc(8);
  bytes.writeDoubleBE(value);
  return ebmlElement(id, bytes);
}

function ebmlString(id, value) {
  return ebmlElement(id, Buffer.from(value, "utf8"));
}

function webmTrack({ number, codec, width, height, audio }) {
  return ebmlElement(
    "ae",
    ebmlUnsigned("d7", number),
    ebmlUnsigned("73c5", number),
    ebmlUnsigned("83", audio ? 2 : 1),
    ebmlString("86", codec),
    audio
      ? ebmlElement(
          "e1",
          ebmlFloat("b5", 48_000),
          ebmlUnsigned("9f", 2),
          ebmlUnsigned("6264", 16),
        )
      : ebmlElement(
          "e0",
          ebmlUnsigned("b0", width),
          ebmlUnsigned("ba", height),
        ),
  );
}

function buildWebm({ width = 320, height = 180 } = {}) {
  const ebmlHeader = ebmlElement(
    "1a45dfa3",
    ebmlUnsigned("4286", 1),
    ebmlUnsigned("42f7", 1),
    ebmlUnsigned("42f2", 4),
    ebmlUnsigned("42f3", 8),
    ebmlString("4282", "webm"),
    ebmlUnsigned("4287", 4),
    ebmlUnsigned("4285", 2),
  );
  const info = ebmlElement(
    "1549a966",
    ebmlUnsigned("2ad7b1", 1_000_000),
    ebmlFloat("4489", 2_000),
    ebmlString("4d80", "Material Eagle"),
    ebmlString("5741", "Material Eagle"),
  );
  const tracks = ebmlElement(
    "1654ae6b",
    webmTrack({ number: 1, codec: "V_VP9", width, height, audio: false }),
    webmTrack({ number: 2, codec: "A_OPUS", audio: true }),
  );
  const cluster = ebmlElement("1f43b675", ebmlUnsigned("e7", 0));
  return concat(ebmlHeader, ebmlElement("18538067", info, tracks, cluster));
}

export function buildVideoFormatFixtures(referencePng) {
  assert.ok(
    referencePng.subarray(0, 8).equals(Buffer.from("89504e470d0a1a0a", "hex")),
    "reference is not PNG",
  );
  const mp4 = buildIsoVideo({ brand: "isom", durationMs: 2_000 });
  const mov = buildIsoVideo({ brand: "qt  ", durationMs: 2_000 });
  const webm = buildWebm();
  return new Map([
    ["video/minimal.mp4", mp4],
    ["video/minimal.mov", mov],
    ["video/minimal.webm", webm],
    ["video/truncated.mp4", mp4.subarray(0, 24)],
    ["video/png-disguised-as-mp4.mp4", Buffer.from(referencePng)],
    ["video/mp4-disguised-as-webm.webm", Buffer.from(mp4)],
    [
      "video/unknown-codec.mp4",
      buildIsoVideo({ brand: "isom", durationMs: 2_000, codec: "zzzz" }),
    ],
    [
      "video/oversized-duration.mp4",
      buildIsoVideo({
        brand: "isom",
        durationMs: 0xffff_fffe * 1_000,
      }),
    ],
    ["video/oversized-dimensions.webm", buildWebm({ width: 65_536 })],
  ]);
}

export async function generateVideoFormatFixtures({
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
  const fixtures = buildVideoFormatFixtures(referencePng);
  for (const relativePath of GENERATED_VIDEO_FIXTURES) {
    const bytes = fixtures.get(relativePath);
    assert.ok(bytes, `missing generated bytes for ${relativePath}`);
    await writeFixedFixture(relativePath, bytes, replaceExisting);
  }
  return fixtures;
}

async function writeFixedFixture(relativePath, bytes, replaceExisting) {
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
      "usage: node tools/generate-video-format-fixtures.mjs [--replace]",
    );
  }
  const fixtures = await generateVideoFormatFixtures({
    replaceExisting: arguments_.includes("--replace"),
  });
  const report = [...fixtures].map(([fixturePath, bytes]) => ({
    path: `sources/${fixturePath}`,
    size: bytes.length,
    sha256: createHash("sha256").update(bytes).digest("hex"),
  }));
  console.log(JSON.stringify({ schema: 1, fixtures: report }, null, 2));
}
