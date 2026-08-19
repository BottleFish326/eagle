export const FORMAL_SOAK_BASELINE_COMMIT =
  "c18e1cae6a2ca40805dfd39fdc8406f1f95ffd21";

export const FORMAL_SOAK_LOADED_PATHS = Object.freeze([
  "tools/resource-stability-analysis.mjs",
  "tools/resource-stability-checkpoint.mjs",
  "tools/verify-resource-stability.mjs",
]);

export const FORMAL_SOAK_PRODUCT_SCOPES = Object.freeze([
  "Cargo.toml",
  "Cargo.lock",
  ".nvmrc",
  "crates",
  "apps",
  "integrations",
  "tools/fixture-generator",
  "tools/resource-soak",
]);

export function buildSoakBaselineAudit({
  baselineCommit,
  currentCommit,
  descendantOfBaseline,
  loadedChangedPaths,
  productChangedPaths,
}) {
  const failures = [];
  if (!isCommit(baselineCommit)) failures.push("baseline commit is invalid");
  if (!isCommit(currentCommit)) failures.push("current commit is invalid");
  if (descendantOfBaseline !== true)
    failures.push("current commit does not descend from the soak baseline");

  const loadedChanges = normalizePaths(loadedChangedPaths, failures, "loaded");
  const productChanges = normalizePaths(
    productChangedPaths,
    failures,
    "product",
  );
  if (loadedChanges.length > 0)
    failures.push(
      `formal soak-loaded inputs differ from the baseline: ${loadedChanges.join(", ")}`,
    );
  if (productChanges.length > 0)
    failures.push(
      `formal soak product inputs differ from the baseline: ${productChanges.join(", ")}`,
    );

  return {
    schema: 1,
    accepted: failures.length === 0,
    failures,
    baselineCommit,
    currentCommit,
    descendantOfBaseline: descendantOfBaseline === true,
    loadedInputs: {
      scopes: [...FORMAL_SOAK_LOADED_PATHS],
      changedPaths: loadedChanges,
    },
    productInputs: {
      scopes: [...FORMAL_SOAK_PRODUCT_SCOPES],
      changedPaths: productChanges,
    },
  };
}

function normalizePaths(values, failures, label) {
  if (!Array.isArray(values)) {
    failures.push(`${label} changed paths are not an array`);
    return [];
  }
  const normalized = [];
  for (const value of values) {
    if (
      typeof value !== "string" ||
      value.length === 0 ||
      value.startsWith("/") ||
      value.split("/").includes("..")
    ) {
      failures.push(
        `${label} changed paths contain an invalid repository path`,
      );
      continue;
    }
    normalized.push(value);
  }
  return [...new Set(normalized)].toSorted();
}

function isCommit(value) {
  return typeof value === "string" && /^[0-9a-f]{40,64}$/u.test(value);
}
