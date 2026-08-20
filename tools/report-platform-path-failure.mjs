import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const DEFAULT_MAX_DETAIL_CHARACTERS = 12_000;

export function formatPlatformPathFailureAnnotation(
  report,
  { maxDetailCharacters = DEFAULT_MAX_DETAIL_CHARACTERS } = {},
) {
  const failures = Array.isArray(report?.failures)
    ? report.failures.filter((value) => typeof value === "string")
    : [];
  const test = report?.processResults?.test;
  const output = [test?.stdout, test?.stderr]
    .filter((value) => typeof value === "string" && value.trim() !== "")
    .map((value) => value.trim())
    .join("\n\n");
  const heading =
    failures.length > 0
      ? failures.join("; ")
      : "P2-A12 platform path evidence was rejected without a recorded failure";
  const details = output === "" ? heading : `${heading}\n\n${output}`;
  const bounded = tailByCodePoints(details, maxDetailCharacters);
  return `::error title=${escapeProperty("P2-A12 platform path rejection")}::${escapeData(bounded)}`;
}

export function formatMissingPlatformPathReportAnnotation(error) {
  const message =
    error instanceof Error && error.message !== ""
      ? error.message
      : "unknown report read error";
  return `::error title=${escapeProperty("P2-A12 report unavailable")}::${escapeData(message)}`;
}

function tailByCodePoints(value, maximum) {
  if (!Number.isSafeInteger(maximum) || maximum <= 0) {
    throw new Error("maxDetailCharacters must be a positive safe integer");
  }
  const points = [...value];
  if (points.length <= maximum) return value;
  return `[output truncated to final ${String(maximum)} characters]\n${points.slice(-maximum).join("")}`;
}

function escapeData(value) {
  return value
    .replaceAll("%", "%25")
    .replaceAll("\r", "%0D")
    .replaceAll("\n", "%0A");
}

function escapeProperty(value) {
  return escapeData(value).replaceAll(":", "%3A").replaceAll(",", "%2C");
}

async function main(args) {
  if (args.length !== 2 || args[0] !== "--input") {
    throw new Error(
      "usage: node tools/report-platform-path-failure.mjs --input <json>",
    );
  }
  try {
    const report = JSON.parse(await readFile(path.resolve(args[1]), "utf8"));
    console.log(formatPlatformPathFailureAnnotation(report));
  } catch (error) {
    console.log(formatMissingPlatformPathReportAnnotation(error));
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  await main(process.argv.slice(2));
}
