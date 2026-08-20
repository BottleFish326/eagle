import { createHash } from "node:crypto";
import { isDeepStrictEqual } from "node:util";

import { inspectP2LocalFaultGatesReceipt } from "./p2-local-fault-gates.mjs";
import { inspectPhase2ExternalGatesReceipt } from "./phase-2-external-gates.mjs";

export const P2_DATA_SAFETY_REPORTS = Object.freeze([
  "docs/reports/phase-1-acceptance.md",
  "docs/reports/p2-01-acceptance.md",
  "docs/reports/p2-02-acceptance.md",
  "docs/reports/p2-03-acceptance.md",
  "docs/reports/p2-04-acceptance.md",
  "docs/reports/p2-05-acceptance.md",
  "docs/reports/p2-06-acceptance.md",
  "docs/reports/p2-07-acceptance.md",
  "docs/reports/p2-08-acceptance.md",
]);

export const P2_DATA_SAFETY_CONTROLS = Object.freeze(
  [
    {
      id: "DS-01",
      title: "original asset immutability",
      evidenceFiles: [
        "docs/reports/phase-1-acceptance.md",
        "docs/reports/p2-02-acceptance.md",
        "docs/reports/p2-05-acceptance.md",
      ],
    },
    {
      id: "DS-02",
      title: "atomic sidecar persistence and conflict protection",
      evidenceFiles: [
        "docs/reports/phase-1-acceptance.md",
        "docs/reports/p2-03-acceptance.md",
        "docs/reports/p2-04-acceptance.md",
      ],
    },
    {
      id: "DS-03",
      title: "recoverable batch transactions",
      evidenceFiles: ["docs/reports/p2-03-acceptance.md"],
    },
    {
      id: "DS-04",
      title: "external and synchronization conflict preservation",
      evidenceFiles: ["docs/reports/p2-04-acceptance.md"],
    },
    {
      id: "DS-05",
      title: "derived cache ownership and crash recovery",
      evidenceFiles: ["docs/reports/p2-05-acceptance.md"],
    },
    {
      id: "DS-06",
      title: "filesystem event convergence and offline isolation",
      evidenceFiles: [
        "docs/reports/p2-01-acceptance.md",
        "docs/reports/p2-08-acceptance.md",
      ],
    },
    {
      id: "DS-07",
      title: "bounded resource and queue behavior",
      evidenceFiles: ["docs/reports/p2-06-acceptance.md"],
    },
    {
      id: "DS-08",
      title: "read-only and redacted diagnostics",
      evidenceFiles: ["docs/reports/p2-07-acceptance.md"],
    },
    {
      id: "DS-09",
      title: "authorized path and reference boundaries",
      evidenceFiles: [
        "docs/reports/phase-1-acceptance.md",
        "docs/reports/p2-08-acceptance.md",
      ],
    },
    {
      id: "DS-10",
      title: "real-process crash and hosted platform evidence",
      evidenceFiles: [
        "docs/reports/p2-03-acceptance.md",
        "docs/reports/p2-05-acceptance.md",
        "docs/reports/p2-06-acceptance.md",
        "docs/reports/p2-08-acceptance.md",
      ],
    },
  ].map((control) =>
    Object.freeze({
      ...control,
      evidenceFiles: Object.freeze([...control.evidenceFiles]),
    }),
  ),
);

export function buildP2DataSafetyAuditReport({
  candidateCommit,
  candidateCommittedAt,
  repositoryClean,
  commitOrderVerified,
  inputsCommitted,
  defectRegisterBytes,
  defectRegister,
  externalBytes,
  externalReceipt,
  localFaultBytes,
  localFaultReceipt,
  reportFiles,
}) {
  const failures = [];
  if (!isCommit(candidateCommit))
    failures.push("data safety candidate commit is invalid");
  if (!isIsoInstant(candidateCommittedAt))
    failures.push("data safety candidate commit time is invalid");
  if (repositoryClean !== true)
    failures.push("data safety candidate repository is not clean");
  if (commitOrderVerified !== true)
    failures.push(
      "data safety commits are not ordered external <= local faults <= candidate",
    );
  if (inputsCommitted !== true)
    failures.push("data safety inputs are not committed in the candidate");

  const externalInspection = inspectPhase2ExternalGatesReceipt(externalReceipt);
  for (const failure of externalInspection.failures)
    failures.push(`external gates: ${failure}`);
  const localInspection = inspectP2LocalFaultGatesReceipt(localFaultReceipt);
  for (const failure of localInspection.failures)
    failures.push(`local fault gates: ${failure}`);

  const earliestReview = laterIsoInstant(
    externalReceipt?.evidenceAt,
    localFaultReceipt?.executedAt,
  );
  const registerInspection = inspectDefectRegister(defectRegister, {
    requireReviewed: true,
    minimumReviewedAt: earliestReview,
    maximumReviewedAt: candidateCommittedAt,
  });
  for (const failure of registerInspection.failures)
    failures.push(`defect register: ${failure}`);
  const reports = normalizeReports(reportFiles, failures);

  const externalSha256 = sha256(externalBytes);
  const localFaultSha256 = sha256(localFaultBytes);
  const defectRegisterSha256 = sha256(defectRegisterBytes);
  if (!isSha256(externalSha256))
    failures.push("external gate evidence bytes are invalid");
  if (!isSha256(localFaultSha256))
    failures.push("local fault evidence bytes are invalid");
  if (!isSha256(defectRegisterSha256))
    failures.push("defect register bytes are invalid");

  return {
    schema: 1,
    accepted: failures.length === 0,
    failures,
    evidenceAt: laterIsoInstant(
      externalReceipt?.evidenceAt,
      localFaultReceipt?.executedAt,
      defectRegister?.reviewedAt,
    ),
    candidateCommit,
    candidateCommittedAt,
    repositoryClean: repositoryClean === true,
    commitOrderVerified: commitOrderVerified === true,
    inputsCommitted: inputsCommitted === true,
    defectRegister: {
      fileName: "docs/defects.json",
      sha256: defectRegisterSha256,
      status: defectRegister?.status ?? null,
      reviewedAt: defectRegister?.reviewedAt ?? null,
      counts: registerInspection.counts,
      findings: Array.isArray(defectRegister?.findings)
        ? structuredClone(defectRegister.findings)
        : [],
    },
    p2External: {
      fileName: "docs/reports/evidence/p2-external-gates.json",
      sha256: externalSha256,
      evidenceAt: externalReceipt?.evidenceAt ?? null,
      p2A11Commit: externalReceipt?.p2A11?.gitCommit ?? null,
      p2A12Commit: externalReceipt?.p2A12?.gitCommit ?? null,
    },
    p2LocalFaults: {
      fileName: "docs/reports/evidence/p2-local-fault-gates.json",
      sha256: localFaultSha256,
      gitCommit: localFaultReceipt?.gitCommit ?? null,
      executedAt: localFaultReceipt?.executedAt ?? null,
    },
    reports,
    controls: P2_DATA_SAFETY_CONTROLS.map((control) => ({
      ...control,
      evidenceFiles: [...control.evidenceFiles],
    })),
  };
}

export function inspectDefectRegister(
  value,
  {
    requireReviewed = false,
    minimumReviewedAt = null,
    maximumReviewedAt = null,
  } = {},
) {
  const failures = [];
  checkExactKeys(
    value,
    ["schema", "scope", "status", "reviewedAt", "findings"],
    failures,
    "defect register",
  );
  if (value?.schema !== 1) failures.push("schema is invalid");
  if (value?.scope !== "phase-2-exit") failures.push("scope is invalid");
  if (!["draft", "reviewed"].includes(value?.status))
    failures.push("status is invalid");
  if (value?.status === "draft" && value?.reviewedAt !== null)
    failures.push("draft review time is not null");
  if (value?.status === "reviewed" && !isIsoInstant(value?.reviewedAt))
    failures.push("review time is invalid");
  if (requireReviewed && value?.status !== "reviewed")
    failures.push("status is not reviewed");
  if (
    minimumReviewedAt !== null &&
    isIsoInstant(minimumReviewedAt) &&
    isIsoInstant(value?.reviewedAt) &&
    Date.parse(value.reviewedAt) < Date.parse(minimumReviewedAt)
  )
    failures.push("review predates the final gate evidence");
  if (
    maximumReviewedAt !== null &&
    isIsoInstant(maximumReviewedAt) &&
    isIsoInstant(value?.reviewedAt) &&
    Date.parse(value.reviewedAt) > Date.parse(maximumReviewedAt) + 999
  )
    failures.push("review postdates the candidate commit");

  const findings = Array.isArray(value?.findings) ? value.findings : [];
  if (!Array.isArray(value?.findings))
    failures.push("findings are not an array");
  if (findings.length > 1_000) failures.push("findings exceed the bound");
  const identifiers = new Set();
  const counts = emptyCounts();
  for (const [index, finding] of findings.entries()) {
    inspectFinding(finding, index, identifiers, counts, failures);
  }
  if (counts.open.P0 > 0 || counts.open.P1 > 0)
    failures.push("one or more P0/P1 defects remain open");

  return { accepted: failures.length === 0, failures, counts };
}

export function inspectP2DataSafetyAuditReceipt(value) {
  const failures = [];
  checkExactKeys(
    value,
    [
      "schema",
      "accepted",
      "failures",
      "evidenceAt",
      "candidateCommit",
      "candidateCommittedAt",
      "repositoryClean",
      "commitOrderVerified",
      "inputsCommitted",
      "defectRegister",
      "p2External",
      "p2LocalFaults",
      "reports",
      "controls",
    ],
    failures,
    "data safety audit receipt",
  );
  if (value?.schema !== 1)
    failures.push("data safety audit receipt schema is invalid");
  if (value?.accepted !== true)
    failures.push("data safety audit receipt is not accepted");
  if (!isEmptyArray(value?.failures))
    failures.push("data safety audit receipt failures are not empty");
  if (!isIsoInstant(value?.evidenceAt))
    failures.push("data safety audit receipt evidence time is invalid");
  if (!isCommit(value?.candidateCommit))
    failures.push("data safety audit receipt candidate commit is invalid");
  if (!isIsoInstant(value?.candidateCommittedAt))
    failures.push("data safety audit receipt candidate commit time is invalid");
  for (const field of [
    "repositoryClean",
    "commitOrderVerified",
    "inputsCommitted",
  ])
    if (value?.[field] !== true)
      failures.push(`data safety audit receipt ${field} is not true`);

  inspectRegisterReceipt(value?.defectRegister, failures);
  inspectExternalReceipt(value?.p2External, failures);
  inspectLocalReceipt(value?.p2LocalFaults, failures);
  inspectReportReceipts(value?.reports, failures);
  if (!isDeepStrictEqual(value?.controls, P2_DATA_SAFETY_CONTROLS))
    failures.push("data safety audit controls are invalid");
  const expectedEvidenceAt = laterIsoInstant(
    value?.p2External?.evidenceAt,
    value?.p2LocalFaults?.executedAt,
    value?.defectRegister?.reviewedAt,
  );
  if (value?.evidenceAt !== expectedEvidenceAt)
    failures.push("data safety audit evidence time is not deterministic");
  if (
    isIsoInstant(value?.defectRegister?.reviewedAt) &&
    isIsoInstant(value?.p2External?.evidenceAt) &&
    Date.parse(value.defectRegister.reviewedAt) <
      Date.parse(value.p2External.evidenceAt)
  )
    failures.push("data safety review predates external evidence");
  if (
    isIsoInstant(value?.defectRegister?.reviewedAt) &&
    isIsoInstant(value?.p2LocalFaults?.executedAt) &&
    Date.parse(value.defectRegister.reviewedAt) <
      Date.parse(value.p2LocalFaults.executedAt)
  )
    failures.push("data safety review predates local fault evidence");
  if (
    isIsoInstant(value?.defectRegister?.reviewedAt) &&
    isIsoInstant(value?.candidateCommittedAt) &&
    Date.parse(value.defectRegister.reviewedAt) >
      Date.parse(value.candidateCommittedAt) + 999
  )
    failures.push("data safety review postdates the candidate commit");

  return { accepted: failures.length === 0, failures };
}

function inspectFinding(finding, index, identifiers, counts, failures) {
  const label = `finding[${String(index)}]`;
  checkExactKeys(
    finding,
    [
      "id",
      "severity",
      "status",
      "summary",
      "evidence",
      "workaround",
      "targetVersion",
      "resolvedAt",
    ],
    failures,
    label,
  );
  if (!/^DEF-[0-9]{4,}$/u.test(finding?.id ?? "")) {
    failures.push(`${label} ID is invalid`);
  } else if (identifiers.has(finding.id)) {
    failures.push(`${label} ID is duplicated`);
  } else {
    identifiers.add(finding.id);
  }
  if (!["P0", "P1", "P2", "P3"].includes(finding?.severity))
    failures.push(`${label} severity is invalid`);
  if (!["open", "resolved"].includes(finding?.status))
    failures.push(`${label} status is invalid`);
  if (!isBoundedString(finding?.summary, 1_000))
    failures.push(`${label} summary is invalid`);
  if (!isUniqueBoundedStrings(finding?.evidence, 64, 1_000))
    failures.push(`${label} evidence is invalid`);
  for (const field of ["workaround", "targetVersion"])
    if (
      finding?.[field] !== null &&
      !isBoundedString(finding?.[field], field === "workaround" ? 2_000 : 128)
    )
      failures.push(`${label} ${field} is invalid`);
  if (finding?.status === "open" && finding?.resolvedAt !== null)
    failures.push(`${label} open finding has a resolution time`);
  if (finding?.status === "resolved" && !isIsoInstant(finding?.resolvedAt))
    failures.push(`${label} resolution time is invalid`);
  if (
    finding?.status === "open" &&
    finding?.severity === "P2" &&
    (!isBoundedString(finding?.workaround, 2_000) ||
      !isBoundedString(finding?.targetVersion, 128))
  )
    failures.push(`${label} open P2 lacks a workaround or target version`);
  if (
    ["P0", "P1", "P2", "P3"].includes(finding?.severity) &&
    ["open", "resolved"].includes(finding?.status)
  )
    counts[finding.status][finding.severity] += 1;
}

function inspectRegisterReceipt(value, failures) {
  checkExactKeys(
    value,
    ["fileName", "sha256", "status", "reviewedAt", "counts", "findings"],
    failures,
    "data safety defect register receipt",
  );
  if (value?.fileName !== "docs/defects.json")
    failures.push("data safety defect register filename is invalid");
  if (!isSha256(value?.sha256))
    failures.push("data safety defect register digest is invalid");
  const inspection = inspectDefectRegister(
    {
      schema: 1,
      scope: "phase-2-exit",
      status: value?.status,
      reviewedAt: value?.reviewedAt,
      findings: value?.findings,
    },
    { requireReviewed: true },
  );
  for (const failure of inspection.failures)
    failures.push(`data safety defect register: ${failure}`);
  if (!isDeepStrictEqual(value?.counts, inspection.counts))
    failures.push("data safety defect register counts are invalid");
}

function inspectExternalReceipt(value, failures) {
  checkExactKeys(
    value,
    ["fileName", "sha256", "evidenceAt", "p2A11Commit", "p2A12Commit"],
    failures,
    "data safety external receipt",
  );
  if (value?.fileName !== "docs/reports/evidence/p2-external-gates.json")
    failures.push("data safety external receipt filename is invalid");
  if (!isSha256(value?.sha256))
    failures.push("data safety external receipt digest is invalid");
  if (!isIsoInstant(value?.evidenceAt))
    failures.push("data safety external receipt evidence time is invalid");
  for (const field of ["p2A11Commit", "p2A12Commit"])
    if (!isCommit(value?.[field]))
      failures.push(`data safety external receipt ${field} is invalid`);
}

function inspectLocalReceipt(value, failures) {
  checkExactKeys(
    value,
    ["fileName", "sha256", "gitCommit", "executedAt"],
    failures,
    "data safety local fault receipt",
  );
  if (value?.fileName !== "docs/reports/evidence/p2-local-fault-gates.json")
    failures.push("data safety local fault receipt filename is invalid");
  if (!isSha256(value?.sha256))
    failures.push("data safety local fault receipt digest is invalid");
  if (!isCommit(value?.gitCommit))
    failures.push("data safety local fault receipt commit is invalid");
  if (!isIsoInstant(value?.executedAt))
    failures.push("data safety local fault receipt execution time is invalid");
}

function normalizeReports(values, failures) {
  if (!Array.isArray(values)) {
    failures.push("data safety report files are not an array");
    return [];
  }
  const byPath = new Map();
  for (const [index, value] of values.entries()) {
    if (
      !P2_DATA_SAFETY_REPORTS.includes(value?.fileName) ||
      !(Buffer.isBuffer(value?.bytes) || value?.bytes instanceof Uint8Array)
    ) {
      failures.push(`data safety report[${String(index)}] is invalid`);
      continue;
    }
    if (byPath.has(value.fileName)) {
      failures.push(`data safety report is duplicated: ${value.fileName}`);
      continue;
    }
    byPath.set(value.fileName, {
      fileName: value.fileName,
      sha256: sha256(value.bytes),
    });
  }
  for (const fileName of P2_DATA_SAFETY_REPORTS)
    if (!byPath.has(fileName))
      failures.push(`data safety report is missing: ${fileName}`);
  return P2_DATA_SAFETY_REPORTS.flatMap((fileName) => {
    const report = byPath.get(fileName);
    return report === undefined ? [] : [report];
  });
}

function inspectReportReceipts(values, failures) {
  if (
    !Array.isArray(values) ||
    values.length !== P2_DATA_SAFETY_REPORTS.length
  ) {
    failures.push("data safety report receipt set is incomplete");
    return;
  }
  for (const [index, fileName] of P2_DATA_SAFETY_REPORTS.entries()) {
    const value = values[index];
    checkExactKeys(
      value,
      ["fileName", "sha256"],
      failures,
      `data safety report receipt ${fileName}`,
    );
    if (value?.fileName !== fileName)
      failures.push(`data safety report receipt order is invalid: ${fileName}`);
    if (!isSha256(value?.sha256))
      failures.push(
        `data safety report receipt digest is invalid: ${fileName}`,
      );
  }
}

function emptyCounts() {
  return {
    open: { P0: 0, P1: 0, P2: 0, P3: 0 },
    resolved: { P0: 0, P1: 0, P2: 0, P3: 0 },
  };
}

function checkExactKeys(value, expected, failures, label) {
  if (!isRecord(value)) {
    failures.push(`${label} is not an object`);
    return;
  }
  if (
    !isDeepStrictEqual(Object.keys(value).toSorted(), [...expected].toSorted())
  )
    failures.push(`${label} fields are invalid`);
}

function sha256(bytes) {
  return Buffer.isBuffer(bytes) || bytes instanceof Uint8Array
    ? createHash("sha256").update(bytes).digest("hex")
    : null;
}

function laterIsoInstant(...values) {
  const candidates = values.filter(isIsoInstant);
  return candidates.length === 0
    ? null
    : candidates.toSorted((a, b) => Date.parse(a) - Date.parse(b)).at(-1);
}

function isRecord(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isEmptyArray(value) {
  return Array.isArray(value) && value.length === 0;
}

function isCommit(value) {
  return typeof value === "string" && /^[0-9a-f]{40,64}$/u.test(value);
}

function isSha256(value) {
  return typeof value === "string" && /^[0-9a-f]{64}$/u.test(value);
}

function isIsoInstant(value) {
  if (typeof value !== "string") return false;
  if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{3})?Z$/u.test(value))
    return false;
  const timestamp = Date.parse(value);
  if (!Number.isFinite(timestamp)) return false;
  const canonical = new Date(timestamp).toISOString();
  return canonical === value || canonical === value.replace(/Z$/u, ".000Z");
}

function isBoundedString(value, maximum) {
  return (
    typeof value === "string" && value.trim() !== "" && value.length <= maximum
  );
}

function isUniqueBoundedStrings(value, maximumItems, maximumLength) {
  return (
    Array.isArray(value) &&
    value.length > 0 &&
    value.length <= maximumItems &&
    value.every((entry) => isBoundedString(entry, maximumLength)) &&
    new Set(value).size === value.length
  );
}
