import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { promisify } from "node:util";
import { fileURLToPath } from "node:url";

import Ajv2020 from "ajv/dist/2020.js";
import addFormats from "ajv-formats";

import { writeJsonAtomic } from "./resource-stability-checkpoint.mjs";

const runFile = promisify(execFile);
const repository = path.resolve(import.meta.dirname, "..");
const schemaPath = path.join(
  repository,
  "schemas",
  "query-conformance-manifest.schema.json",
);
const UUID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/u;
const COLOR_SPACE_PATTERN = /^[a-z0-9][a-z0-9._-]{0,63}$/u;
const EXTENSION_PATTERN = /^[a-z0-9]{1,32}$/u;
const UNKNOWN_FIELDS = new Set([
  "size",
  "width",
  "height",
  "aspect",
  "created",
  "modified",
  "duration",
  "pages",
  "orientation",
  "root",
  "color-space",
  "has-alpha",
]);
const NUMERIC_FIELDS = new Set([
  "rating",
  "size",
  "width",
  "height",
  "created",
  "modified",
  "duration",
  "pages",
]);
const COMPARISON_OPERATORS = new Set(["eq", "lt", "lte", "gt", "gte"]);

export async function loadQueryManifest(manifestPath) {
  const [bytes, schemaBytes] = await Promise.all([
    readFile(manifestPath),
    readFile(schemaPath),
  ]);
  const manifest = JSON.parse(bytes);
  const schema = JSON.parse(schemaBytes);
  const ajv = new Ajv2020({ allErrors: true, strict: true });
  addFormats(ajv);
  const validate = ajv.compile(schema);
  if (!validate(manifest)) {
    const details = (validate.errors ?? [])
      .map((error) => `${error.instancePath || "/"} ${error.message}`)
      .join("; ");
    throw new Error(`query manifest schema rejected: ${details}`);
  }
  return {
    manifest,
    sha256: createHash("sha256").update(bytes).digest("hex"),
  };
}

export function inspectQueryManifest(manifest, { formal = false } = {}) {
  const failures = [];
  const recordKeys = uniqueValues(
    manifest.records.map((record) => record.key),
    "record key",
    failures,
  );
  const cases = [...manifest.validCases, ...manifest.invalidCases];
  uniqueValues(
    cases.map((entry) => entry.id),
    "case id",
    failures,
  );
  for (const record of manifest.records) {
    if ((record.width === null) !== (record.height === null)) {
      failures.push(
        `${record.key}: width and height must both be known or unknown`,
      );
    }
    if (record.relativePath !== record.relativePath.normalize("NFC")) {
      failures.push(`${record.key}: relativePath is not NFC`);
    }
    if (!isCodePointSorted(record.tags)) {
      failures.push(`${record.key}: tags are not in stable code-point order`);
    }
  }
  for (const entry of manifest.validCases) {
    if (!isCodePointSorted(entry.expectedKeys)) {
      failures.push(
        `${entry.id}: expectedKeys are not in stable code-point order`,
      );
    }
    for (const key of entry.expectedKeys) {
      if (!recordKeys.has(key))
        failures.push(`${entry.id}: expected key is undeclared: ${key}`);
    }
    for (const [index, predicate] of entry.oracle.entries()) {
      validatePredicate(
        predicate,
        `${entry.id}.oracle[${String(index)}]`,
        failures,
      );
    }
  }
  if (formal) {
    if (manifest.records.length < 40)
      failures.push("formal manifest has fewer than 40 records");
    if (manifest.validCases.length < 60)
      failures.push("formal manifest has fewer than 60 valid cases");
    if (manifest.invalidCases.length < 24)
      failures.push("formal manifest has fewer than 24 invalid cases");
  }
  return { accepted: failures.length === 0, failures };
}

export function evaluateOracleCase(records, entry) {
  return records
    .filter((record) =>
      entry.oracle.every((predicate) => matchesPredicate(record, predicate)),
    )
    .map((record) => record.key)
    .toSorted(compareCodePoints);
}

export function buildQueryConformanceReport({
  manifest,
  manifestSha256,
  productReport,
  formal = false,
  gitCommit = null,
}) {
  const failures = [];
  const inspection = inspectQueryManifest(manifest, { formal });
  failures.push(...inspection.failures);
  if (productReport.schema !== 1)
    failures.push("product report schema is not 1");
  if (productReport.recordCount !== manifest.records.length) {
    failures.push("product report record count does not match the manifest");
  }
  const productValid = indexedResults(
    productReport.validCases,
    "product valid",
    failures,
  );
  const productInvalid = indexedResults(
    productReport.invalidCases,
    "product invalid",
    failures,
  );
  const validCases = manifest.validCases.map((entry) => {
    const oracleKeys = evaluateOracleCase(manifest.records, entry);
    const product = productValid.get(entry.id);
    const oracleMatchesExpected = arraysEqual(oracleKeys, entry.expectedKeys);
    const productMatchesExpected =
      product !== undefined &&
      product.error === null &&
      Array.isArray(product.keys) &&
      arraysEqual(product.keys, entry.expectedKeys);
    if (!oracleMatchesExpected)
      failures.push(`${entry.id}: oracle differs from expectedKeys`);
    if (!productMatchesExpected)
      failures.push(`${entry.id}: product differs from expectedKeys`);
    return {
      id: entry.id,
      oracleMatchesExpected,
      productMatchesExpected,
      matchCount: entry.expectedKeys.length,
      elapsedNanoseconds: product?.elapsedNanoseconds ?? null,
    };
  });
  const invalidCases = manifest.invalidCases.map((entry) => {
    const product = productInvalid.get(entry.id);
    const productMatchesExpected =
      product !== undefined &&
      product.keys === null &&
      product.error?.kind === entry.errorKind &&
      product.error?.offset === entry.offset;
    if (!productMatchesExpected) {
      failures.push(
        `${entry.id}: product parse error differs from the fixed error`,
      );
    }
    return { id: entry.id, productMatchesExpected };
  });
  rejectExtraResults(productValid, manifest.validCases, "valid", failures);
  rejectExtraResults(
    productInvalid,
    manifest.invalidCases,
    "invalid",
    failures,
  );
  return {
    schema: 1,
    accepted: failures.length === 0,
    failures,
    manifest: {
      suiteId: manifest.suite.id,
      suiteVersion: manifest.suite.version,
      sha256: manifestSha256,
      recordCount: manifest.records.length,
      validCaseCount: manifest.validCases.length,
      invalidCaseCount: manifest.invalidCases.length,
    },
    product: {
      gitCommit,
      nodeVersion: process.version,
    },
    validCases,
    invalidCases,
  };
}

export async function executeProductQueryGate(manifestPath) {
  const { stdout } = await runFile(
    "cargo",
    ["run", "--quiet", "-p", "query-gate", "--", "--manifest", manifestPath],
    { cwd: repository, maxBuffer: 16 * 1024 * 1024 },
  );
  return JSON.parse(stdout);
}

function validatePredicate(predicate, label, failures) {
  const { field, operator, value } = predicate;
  if (field === "tag") {
    if (
      !["all", "any", "none"].includes(operator) ||
      !isStableStringSet(value)
    ) {
      failures.push(
        `${label}: tag requires all/any/none and a sorted string set`,
      );
    }
  } else if (
    ["type", "extension", "orientation", "root", "color-space"].includes(field)
  ) {
    if (operator === "is-unknown") {
      if (!UNKNOWN_FIELDS.has(field) || value !== "unknown") {
        failures.push(`${label}: invalid unknown enum predicate`);
      }
    } else if (operator !== "any" || !isStableStringSet(value)) {
      failures.push(`${label}: enum requires any and a sorted string set`);
    } else if (
      field === "type" &&
      value.some(
        (item) => !["image", "video", "audio", "pdf", "other"].includes(item),
      )
    ) {
      failures.push(`${label}: invalid type value`);
    } else if (
      field === "extension" &&
      value.some((item) => !EXTENSION_PATTERN.test(item))
    ) {
      failures.push(`${label}: invalid extension value`);
    } else if (
      field === "orientation" &&
      value.some((item) => !["landscape", "portrait", "square"].includes(item))
    ) {
      failures.push(`${label}: invalid orientation value`);
    } else if (
      field === "root" &&
      value.some((item) => !UUID_PATTERN.test(item))
    ) {
      failures.push(`${label}: invalid root value`);
    } else if (
      field === "color-space" &&
      value.some((item) => !COLOR_SPACE_PATTERN.test(item))
    ) {
      failures.push(`${label}: invalid color-space value`);
    }
  } else if (["favorite", "has-note", "has-alpha"].includes(field)) {
    const validUnknown =
      field === "has-alpha" && operator === "is-unknown" && value === "unknown";
    if (!validUnknown && (operator !== "eq" || typeof value !== "boolean")) {
      failures.push(
        `${label}: boolean predicate has an invalid operator or value`,
      );
    }
  } else if (NUMERIC_FIELDS.has(field)) {
    const validUnknown =
      UNKNOWN_FIELDS.has(field) &&
      operator === "is-unknown" &&
      value === "unknown";
    if (
      !validUnknown &&
      (!COMPARISON_OPERATORS.has(operator) || !Number.isSafeInteger(value))
    ) {
      failures.push(
        `${label}: numeric predicate has an invalid operator or value`,
      );
    }
  } else if (field === "aspect") {
    const validUnknown = operator === "is-unknown" && value === "unknown";
    const validRatio =
      COMPARISON_OPERATORS.has(operator) &&
      value !== null &&
      typeof value === "object" &&
      Number.isSafeInteger(value.numerator) &&
      Number.isSafeInteger(value.denominator) &&
      greatestCommonDivisor(value.numerator, value.denominator) === 1;
    if (!validUnknown && !validRatio)
      failures.push(`${label}: aspect ratio is not reduced`);
  } else if (field === "path") {
    if (
      operator !== "contains" ||
      typeof value !== "string" ||
      value.length === 0 ||
      value !== value.normalize("NFC")
    ) {
      failures.push(`${label}: path requires a non-empty NFC contains value`);
    }
  } else {
    failures.push(`${label}: unsupported oracle field`);
  }
}

function matchesPredicate(record, predicate) {
  const { field, operator, value } = predicate;
  if (field === "tag") {
    const matches = value.map((tag) =>
      record.tags.some((recordTag) => tagMatches(recordTag, tag)),
    );
    if (operator === "all") return matches.every(Boolean);
    if (operator === "any") return matches.some(Boolean);
    return matches.every((matched) => !matched);
  }
  const actual = oracleFieldValue(record, field);
  if (operator === "is-unknown") return actual === null;
  if (operator === "any") return actual !== null && value.includes(actual);
  if (operator === "contains") return actual !== null && actual.includes(value);
  if (actual === null) return false;
  if (field === "aspect") return compareRatios(actual, value, operator);
  if (operator === "eq") return actual === value;
  if (operator === "lt") return actual < value;
  if (operator === "lte") return actual <= value;
  if (operator === "gt") return actual > value;
  if (operator === "gte") return actual >= value;
  return false;
}

function oracleFieldValue(record, field) {
  const dimensions = effectiveDimensions(record);
  if (field === "type") return record.kind;
  if (field === "extension") return record.extension;
  if (field === "favorite") return record.favorite;
  if (field === "rating") return record.rating;
  if (field === "size") return record.size;
  if (field === "width") return dimensions?.width ?? null;
  if (field === "height") return dimensions?.height ?? null;
  if (field === "aspect") {
    return dimensions === null
      ? null
      : { numerator: dimensions.width, denominator: dimensions.height };
  }
  if (field === "created") return record.createdUnixMs;
  if (field === "modified") return record.modifiedUnixMs;
  if (field === "duration") return record.durationMs;
  if (field === "pages") return record.pageCount;
  if (field === "orientation") {
    if (dimensions === null) return null;
    if (dimensions.width > dimensions.height) return "landscape";
    if (dimensions.width < dimensions.height) return "portrait";
    return "square";
  }
  if (field === "root") return record.rootId;
  if (field === "path") return record.relativePath.normalize("NFC");
  if (field === "color-space") return record.colorSpace;
  if (field === "has-note") return record.note.trim().length > 0;
  if (field === "has-alpha") return record.hasAlpha;
  return null;
}

function effectiveDimensions(record) {
  if (record.width === null || record.height === null) return null;
  return record.displayQuarterTurns % 2 === 1
    ? { width: record.height, height: record.width }
    : { width: record.width, height: record.height };
}

function compareRatios(left, right, operator) {
  const comparison =
    BigInt(left.numerator) * BigInt(right.denominator) -
    BigInt(right.numerator) * BigInt(left.denominator);
  if (operator === "eq") return comparison === 0n;
  if (operator === "lt") return comparison < 0n;
  if (operator === "lte") return comparison <= 0n;
  if (operator === "gt") return comparison > 0n;
  return comparison >= 0n;
}

function tagMatches(recordTag, expression) {
  return expression.endsWith("/*")
    ? recordTag.startsWith(`${expression.slice(0, -2)}/`)
    : recordTag === expression;
}

function uniqueValues(values, label, failures) {
  const unique = new Set(values);
  if (unique.size !== values.length) failures.push(`duplicate ${label}`);
  return unique;
}

function indexedResults(results, label, failures) {
  if (!Array.isArray(results)) {
    failures.push(`${label} results are missing`);
    return new Map();
  }
  const map = new Map();
  for (const result of results) {
    if (map.has(result.id))
      failures.push(`duplicate ${label} result: ${String(result.id)}`);
    map.set(result.id, result);
  }
  return map;
}

function rejectExtraResults(results, cases, label, failures) {
  const expected = new Set(cases.map((entry) => entry.id));
  for (const id of results.keys()) {
    if (!expected.has(id))
      failures.push(`undeclared product ${label} result: ${String(id)}`);
  }
}

function isStableStringSet(value) {
  return (
    Array.isArray(value) &&
    value.length > 0 &&
    value.every((item) => typeof item === "string" && item.length > 0) &&
    new Set(value).size === value.length &&
    isCodePointSorted(value)
  );
}

function isCodePointSorted(values) {
  return values.every(
    (value, index) =>
      index === 0 || compareCodePoints(values[index - 1], value) < 0,
  );
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

function greatestCommonDivisor(left, right) {
  let a = Math.abs(left);
  let b = Math.abs(right);
  while (b !== 0) [a, b] = [b, a % b];
  return a;
}

function arraysEqual(left, right) {
  return (
    left.length === right.length &&
    left.every((value, index) => value === right[index])
  );
}

async function main() {
  const options = parseArguments(process.argv.slice(2));
  const { manifest, sha256 } = await loadQueryManifest(options.manifest);
  const productReport = await executeProductQueryGate(options.manifest);
  const { stdout: revision } = await runFile("git", ["rev-parse", "HEAD"], {
    cwd: repository,
  });
  const report = buildQueryConformanceReport({
    manifest,
    manifestSha256: sha256,
    productReport,
    formal: options.formal,
    gitCommit: revision.trim(),
  });
  if (options.output !== undefined)
    await writeJsonAtomic(options.output, report);
  process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
  if (!report.accepted) process.exitCode = 1;
}

function parseArguments(arguments_) {
  const options = { formal: false };
  for (let index = 0; index < arguments_.length; index += 1) {
    const argument = arguments_[index];
    if (argument === "--manifest")
      options.manifest = path.resolve(arguments_[++index]);
    else if (argument === "--output")
      options.output = path.resolve(arguments_[++index]);
    else if (argument === "--formal") options.formal = true;
    else throw new Error(`unknown argument: ${String(argument)}`);
  }
  if (options.manifest === undefined) throw new Error("--manifest is required");
  return options;
}

if (path.resolve(process.argv[1] ?? "") === fileURLToPath(import.meta.url)) {
  await main();
}
