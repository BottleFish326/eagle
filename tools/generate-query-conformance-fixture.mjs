import { randomUUID } from "node:crypto";
import { mkdir, open, rename, unlink } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { evaluateOracleCase } from "./query-conformance.mjs";

const repository = path.resolve(import.meta.dirname, "..");
const outputPath = path.join(
  repository,
  "fixtures",
  "queries",
  "manifest.json",
);
const ROOT_A = "019b76c0-0000-7000-8000-000000000001";
const ROOT_B = "019b76c0-0000-7000-8000-000000000002";
const ROOT_REMOVED = "019b76c0-0000-7000-8000-000000000003";
const BASE_TIME = Date.parse("2026-01-01T00:00:00Z");

export function buildQueryConformanceFixture() {
  const records = Array.from({ length: 64 }, (_, index) => buildRecord(index));
  const validCases = buildValidCases();
  for (const entry of validCases) {
    entry.expectedKeys = evaluateOracleCase(records, entry);
  }
  return {
    schema: 1,
    suite: { id: "advanced-query-p3-a03", version: 1 },
    records,
    validCases,
    invalidCases: buildInvalidCases(),
  };
}

function buildRecord(index) {
  const kinds = ["image", "video", "audio", "pdf", "other"];
  const extensions = ["png", "mp4", "wav", "pdf", "bin"];
  const pathGroups = [
    "Brand Assets/Hero",
    "品牌/图像",
    "日本語/素材",
    "Emoji/🦅",
    "Case/Logo",
    "case/logo",
  ];
  const kind = kinds[index % kinds.length];
  const dimensions =
    index % 7 === 0
      ? [null, null]
      : [
          [1920, 1080],
          [1080, 1920],
          [1000, 1000],
          [1600, 1200],
        ][index % 4];
  const tags = ["project/eagle", index % 2 === 0 ? "group/even" : "group/odd"];
  if (index % 3 === 0) tags.push("state/draft");
  if (index % 5 === 0) tags.push("usage/hero");
  if (index % 10 === 0) tags.push("主题/鹰");
  const hasMediaDuration = kind === "video" || kind === "audio";
  return {
    key: `asset-${String(index).padStart(3, "0")}`,
    rootId: index % 4 === 0 ? null : index % 2 === 0 ? ROOT_A : ROOT_B,
    relativePath: `${pathGroups[index % pathGroups.length]} ${String(index).padStart(3, "0")}.${extensions[index % extensions.length]}`,
    kind,
    extension: extensions[index % extensions.length],
    size: index % 11 === 0 ? null : index === 1 ? 0 : index * 1024 * 1024,
    createdUnixMs: index % 13 === 0 ? null : BASE_TIME + index * 86_400_000,
    modifiedUnixMs: index % 17 === 0 ? null : BASE_TIME + index * 3_600_000,
    width: dimensions[0],
    height: dimensions[1],
    displayQuarterTurns: index % 4,
    durationMs: hasMediaDuration
      ? index % 9 === 0
        ? null
        : index * 1000
      : null,
    pageCount: kind === "pdf" && index % 8 !== 3 ? (index % 5) + 1 : null,
    colorSpace:
      index % 4 === 0 ? null : index % 2 === 0 ? "display-p3" : "srgb",
    hasAlpha: index % 3 === 0 ? null : index % 2 === 0,
    rating: index % 6,
    favorite: index % 2 === 0,
    note:
      index % 4 === 0 ? "" : index % 4 === 1 ? "　" : `候选 ${String(index)}`,
    tags: tags.toSorted(compareCodePoints),
  };
}

function buildValidCases() {
  const cases = [];
  const add = (id, expression, oracle) =>
    cases.push({ id, expression, oracle, expectedKeys: [] });
  add("all-assets", "", []);
  add("tag-all", "project/eagle", [predicate("tag", "all", ["project/eagle"])]);
  add("tag-all-two", "project/eagle usage/hero", [
    predicate("tag", "all", ["project/eagle", "usage/hero"]),
  ]);
  add("tag-any", "any:(group/even|group/odd)", [
    predicate("tag", "any", ["group/even", "group/odd"]),
  ]);
  add("tag-none", "-state/draft", [predicate("tag", "none", ["state/draft"])]);
  add("tag-namespace", "group/*", [predicate("tag", "all", ["group/*"])]);
  add("tag-explicit", "tag:project/eagle", [
    predicate("tag", "all", ["project/eagle"]),
  ]);
  add("tag-unicode", "主题/鹰", [predicate("tag", "all", ["主题/鹰"])]);
  add("type-image", "type:image", [predicate("type", "any", ["image"])]);
  add("type-image-video", "type:image\\|video", [
    predicate("type", "any", ["image", "video"]),
  ]);
  add("extension-png", "ext:png", [predicate("extension", "any", ["png"])]);
  add("extension-pdf-wav", "ext:pdf\\|wav", [
    predicate("extension", "any", ["pdf", "wav"]),
  ]);
  add("favorite-true", "favorite:true", [predicate("favorite", "eq", true)]);
  add("favorite-false", "favorite:false", [predicate("favorite", "eq", false)]);
  add("rating-zero", "rating:0", [predicate("rating", "eq", 0)]);
  add("rating-high", "rating:>=4", [predicate("rating", "gte", 4)]);
  add("rating-range", "rating:>=2 rating:<5", [
    predicate("rating", "gte", 2),
    predicate("rating", "lt", 5),
  ]);
  add("size-unknown", "size:unknown", [
    predicate("size", "is-unknown", "unknown"),
  ]);
  add("size-zero", "size:0", [predicate("size", "eq", 0)]);
  add("size-at-least-10-mib", "size:>=10MiB", [
    predicate("size", "gte", 10 * 1024 * 1024),
  ]);
  add("size-window", "size:>=16MiB size:<32MiB", [
    predicate("size", "gte", 16 * 1024 * 1024),
    predicate("size", "lt", 32 * 1024 * 1024),
  ]);
  add("width-unknown", "width:unknown", [
    predicate("width", "is-unknown", "unknown"),
  ]);
  add("width-hd", "width:>=1920", [predicate("width", "gte", 1920)]);
  add("height-small", "height:<=1080", [predicate("height", "lte", 1080)]);
  add("aspect-unknown", "aspect:unknown", [
    predicate("aspect", "is-unknown", "unknown"),
  ]);
  add("aspect-square", "aspect:1/1", [predicate("aspect", "eq", ratio(1, 1))]);
  add("aspect-wide", "aspect:>=16/9", [
    predicate("aspect", "gte", ratio(16, 9)),
  ]);
  add("aspect-four-three", "aspect:4/3", [
    predicate("aspect", "eq", ratio(4, 3)),
  ]);
  add("orientation-unknown", "orientation:unknown", [
    predicate("orientation", "is-unknown", "unknown"),
  ]);
  add("orientation-landscape", "orientation:landscape", [
    predicate("orientation", "any", ["landscape"]),
  ]);
  add("orientation-portrait-square", "orientation:portrait\\|square", [
    predicate("orientation", "any", ["portrait", "square"]),
  ]);
  add("created-unknown", "created:unknown", [
    predicate("created", "is-unknown", "unknown"),
  ]);
  add("created-after", "created:>=2026-01-20T00:00:00Z", [
    predicate("created", "gte", Date.parse("2026-01-20T00:00:00Z")),
  ]);
  add("created-offset", "created:>=2026-01-20T08:00:00+08:00", [
    predicate("created", "gte", Date.parse("2026-01-20T00:00:00Z")),
  ]);
  add("modified-unknown", "modified:unknown", [
    predicate("modified", "is-unknown", "unknown"),
  ]);
  add("modified-before", "modified:<2026-01-02T00:00:00Z", [
    predicate("modified", "lt", Date.parse("2026-01-02T00:00:00Z")),
  ]);
  add("duration-unknown", "duration:unknown", [
    predicate("duration", "is-unknown", "unknown"),
  ]);
  add("duration-zero", "duration:0", [predicate("duration", "eq", 0)]);
  add("duration-thirty-seconds", "duration:>=30s", [
    predicate("duration", "gte", 30_000),
  ]);
  add("duration-one-minute", "duration:>=1min", [
    predicate("duration", "gte", 60_000),
  ]);
  add("pages-unknown", "pages:unknown", [
    predicate("pages", "is-unknown", "unknown"),
  ]);
  add("pages-two", "pages:>=2", [predicate("pages", "gte", 2)]);
  add("root-unknown", "root:unknown", [
    predicate("root", "is-unknown", "unknown"),
  ]);
  add("root-a", `root:${ROOT_A}`, [predicate("root", "any", [ROOT_A])]);
  add("root-a-b", `root:${ROOT_A}\\|${ROOT_B}`, [
    predicate("root", "any", [ROOT_A, ROOT_B]),
  ]);
  add("root-removed", `root:${ROOT_REMOVED}`, [
    predicate("root", "any", [ROOT_REMOVED]),
  ]);
  add("path-brand", 'path:"Brand Assets"', [
    predicate("path", "contains", "Brand Assets"),
  ]);
  add("path-chinese", "path:品牌", [predicate("path", "contains", "品牌")]);
  add("path-japanese", "path:日本語", [
    predicate("path", "contains", "日本語"),
  ]);
  add("path-emoji", "path:🦅", [predicate("path", "contains", "🦅")]);
  add("path-case-sensitive", "path:Case", [
    predicate("path", "contains", "Case"),
  ]);
  add("path-two-fragments", 'path:"Brand Assets" path:Hero', [
    predicate("path", "contains", "Brand Assets"),
    predicate("path", "contains", "Hero"),
  ]);
  add("color-unknown", "color-space:unknown", [
    predicate("color-space", "is-unknown", "unknown"),
  ]);
  add("color-srgb", "color-space:srgb", [
    predicate("color-space", "any", ["srgb"]),
  ]);
  add("color-two", "color-space:display-p3\\|srgb", [
    predicate("color-space", "any", ["display-p3", "srgb"]),
  ]);
  add("note-true", "has-note:true", [predicate("has-note", "eq", true)]);
  add("note-false", "has-note:false", [predicate("has-note", "eq", false)]);
  add("alpha-unknown", "has-alpha:unknown", [
    predicate("has-alpha", "is-unknown", "unknown"),
  ]);
  add("alpha-true", "has-alpha:true", [predicate("has-alpha", "eq", true)]);
  add("alpha-false", "has-alpha:false", [predicate("has-alpha", "eq", false)]);
  add(
    "combined-basic",
    "project/eagle group/* -state/draft type:image\\|video favorite:true",
    [
      predicate("tag", "all", ["group/*", "project/eagle"]),
      predicate("tag", "none", ["state/draft"]),
      predicate("type", "any", ["image", "video"]),
      predicate("favorite", "eq", true),
    ],
  );
  add(
    "combined-advanced",
    "rating:>=2 size:>=8MiB width:>=1000 orientation:landscape has-note:false",
    [
      predicate("rating", "gte", 2),
      predicate("size", "gte", 8 * 1024 * 1024),
      predicate("width", "gte", 1000),
      predicate("orientation", "any", ["landscape"]),
      predicate("has-note", "eq", false),
    ],
  );
  add("empty-result", "root:019b76c0-0000-7000-8000-000000000099", [
    predicate("root", "any", ["019b76c0-0000-7000-8000-000000000099"]),
  ]);
  add("large-candidate-set", "project/eagle", [
    predicate("tag", "all", ["project/eagle"]),
  ]);
  return cases;
}

function buildInvalidCases() {
  const tooLongTag = `tag:${"a".repeat(129)}`;
  return [
    invalid("invalid-operator", "size:!10", "invalid-operator", 0),
    invalid("invalid-integer", "width:1.5", "invalid-integer", 0),
    invalid("invalid-unit", "size:10MB", "invalid-unit", 0),
    invalid(
      "numeric-overflow",
      "size:18446744073709551616",
      "numeric-overflow",
      0,
    ),
    invalid("invalid-ratio", "aspect:16/0", "invalid-ratio", 0),
    invalid("invalid-date", "modified:2026-08-19", "invalid-date", 0),
    invalid("invalid-enum", "orientation:wide", "invalid-enum", 0),
    invalid("invalid-root-id", "root:NOT-A-UUID", "invalid-root-id", 0),
    invalid("invalid-path-parent", 'path:"../escape"', "invalid-path", 0),
    invalid(
      "invalid-path-windows",
      'path:"Brand\\\\escape"',
      "invalid-path",
      0,
    ),
    invalid("unsupported-unknown", "rating:unknown", "unsupported-unknown", 0),
    invalid(
      "conflicting-range",
      "width:>=10 width:<10",
      "conflicting-range",
      11,
    ),
    invalid(
      "conflicting-value",
      "has-note:true has-note:false",
      "conflicting-value",
      14,
    ),
    invalid("unclosed-quote", 'path:"unclosed', "unclosed-quote", 0),
    invalid("trailing-escape", "tag\\", "trailing-escape", 3),
    invalid("empty-tag", "tag:", "empty-tag", 0),
    invalid("tag-too-long", tooLongTag, "tag-too-long", 0),
    invalid("invalid-wildcard", "ui*", "invalid-wildcard", 0),
    invalid("invalid-or-group", "any:(only)", "invalid-or-group", 0),
    invalid("unknown-filter", "kind:image", "unknown-filter", 0),
    invalid("invalid-type", "type:document", "invalid-type", 0),
    invalid("invalid-extension", "ext:png.exe", "invalid-extension", 0),
    invalid("invalid-favorite", "favorite:yes", "invalid-favorite", 0),
    invalid(
      "conflicting-favorite",
      "favorite:true favorite:false",
      "conflicting-favorite",
      14,
    ),
    invalid("second-invalid-operator", "pages:==2", "invalid-operator", 0),
  ];
}

function predicate(field, operator, value) {
  return { field, operator, value };
}

function ratio(numerator, denominator) {
  return { numerator, denominator };
}

function invalid(id, expression, errorKind, offset) {
  return { id, expression, errorKind, offset };
}

function compareCodePoints(left, right) {
  const leftPoints = [...left].map((character) => character.codePointAt(0));
  const rightPoints = [...right].map((character) => character.codePointAt(0));
  const length = Math.min(leftPoints.length, rightPoints.length);
  for (let index = 0; index < length; index += 1) {
    if (leftPoints[index] !== rightPoints[index])
      return leftPoints[index] - rightPoints[index];
  }
  return leftPoints.length - rightPoints.length;
}

async function writeFixture(manifest) {
  await mkdir(path.dirname(outputPath), { recursive: true });
  const temporary = `${outputPath}.generate-${process.pid}-${randomUUID()}.tmp`;
  let temporaryOwned = false;
  try {
    const handle = await open(temporary, "wx", 0o644);
    temporaryOwned = true;
    try {
      await handle.writeFile(`${JSON.stringify(manifest, null, 2)}\n`);
      await handle.sync();
    } finally {
      await handle.close();
    }
    await rename(temporary, outputPath);
    temporaryOwned = false;
  } finally {
    if (temporaryOwned) await unlink(temporary).catch(() => {});
  }
}

if (path.resolve(process.argv[1] ?? "") === fileURLToPath(import.meta.url)) {
  if (process.argv.length !== 2)
    throw new Error("usage: node tools/generate-query-conformance-fixture.mjs");
  const manifest = buildQueryConformanceFixture();
  await writeFixture(manifest);
  process.stdout.write(
    `${JSON.stringify({ output: "fixtures/queries/manifest.json", records: manifest.records.length, validCases: manifest.validCases.length, invalidCases: manifest.invalidCases.length })}\n`,
  );
}
