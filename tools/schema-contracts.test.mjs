import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

import Ajv2020 from "ajv/dist/2020.js";
import addFormats from "ajv-formats";

import {
  makeP2ExternalReceiptFixture,
  makeP2LocalFaultReceiptFixture,
  TEST_P2_CANDIDATE_COMMIT,
} from "./p2-acceptance-test-fixtures.mjs";
import {
  buildP2DataSafetyAuditReport,
  P2_DATA_SAFETY_REPORTS,
} from "./p2-data-safety-audit.mjs";
import { buildPhase2ExitGatesReport } from "./phase-2-exit-gates.mjs";
import { FORMAL_SOAK_BASELINE_COMMIT } from "./soak-baseline-audit.mjs";

const repository = path.resolve(import.meta.dirname, "..");
const schemaDirectory = path.join(repository, "schemas");
const schemas = await loadSchemas();
const validators = compileSchemas(schemas);

test("compiles every repository JSON Schema as strict draft 2020-12", () => {
  assert.equal(validators.size, schemas.length);
  for (const schema of schemas)
    assert.equal(typeof validators.get(schema.$id), "function", schema.$id);
});

test("validates the tracked draft defect register", async () => {
  const register = JSON.parse(
    await readFile(path.join(repository, "docs", "defects.json"), "utf8"),
  );
  validate("defect-register.schema.json", register);
});

test("validates generated P2 source and stage receipts", () => {
  const fixture = acceptedReceiptFixture();
  validate("platform-matrix-evidence.schema.json", fixture.matrix);
  validate("p2-hosted-run-evidence.schema.json", fixture.hostedRun);
  validate("phase-2-external-gates.schema.json", fixture.external);
  validate("p2-local-fault-gates.schema.json", fixture.localFaults);
  validate("p2-data-safety-audit.schema.json", fixture.dataSafety);
  validate("phase-2-exit-evidence.schema.json", fixture.finalExit);
});

test("rejects boundary mutations in every P2 accepted-only receipt", () => {
  const fixture = acceptedReceiptFixture();
  const mutations = [
    [
      "platform matrix duplicate",
      "platform-matrix-evidence.schema.json",
      fixture.matrix,
      (value) => {
        value.artifacts[2] = structuredClone(value.artifacts[1]);
      },
    ],
    [
      "hosted run job duplicate",
      "p2-hosted-run-evidence.schema.json",
      fixture.hostedRun,
      (value) => {
        value.jobs[4] = structuredClone(value.jobs[3]);
      },
    ],
    [
      "hosted archive file duplicate",
      "p2-hosted-run-evidence.schema.json",
      fixture.hostedRun,
      (value) => {
        value.archive.files[3] = structuredClone(value.archive.files[0]);
      },
    ],
    [
      "external duration boundary",
      "phase-2-external-gates.schema.json",
      fixture.external,
      (value) => {
        value.p2A11.durationSeconds = 1;
      },
    ],
    [
      "external hosted job duplicate",
      "phase-2-external-gates.schema.json",
      fixture.external,
      (value) => {
        value.p2A12.hostedJobs[4] = structuredClone(value.p2A12.hostedJobs[3]);
      },
    ],
    [
      "local recovery boundary",
      "p2-local-fault-gates.schema.json",
      fixture.localFaults,
      (value) => {
        value.p2A04.recoveredCount = 999;
      },
    ],
    [
      "data-safety priority boundary",
      "p2-data-safety-audit.schema.json",
      fixture.dataSafety,
      (value) => {
        value.defectRegister.counts.open.P1 = 1;
      },
    ],
    [
      "data-safety report duplicate",
      "p2-data-safety-audit.schema.json",
      fixture.dataSafety,
      (value) => {
        value.reports[8] = structuredClone(value.reports[7]);
      },
    ],
    [
      "data-safety control order",
      "p2-data-safety-audit.schema.json",
      fixture.dataSafety,
      (value) => {
        value.controls.reverse();
      },
    ],
    [
      "final defect boundary",
      "phase-2-exit-evidence.schema.json",
      fixture.finalExit,
      (value) => {
        value.p2DataSafety.openP0 = 1;
      },
    ],
  ];
  for (const [label, fileName, original, mutate] of mutations) {
    const changed = structuredClone(original);
    mutate(changed);
    const validator = validatorFor(fileName);
    assert.equal(validator(changed), false, label);
    assert.ok(validator.errors?.length > 0, label);
  }
});

async function loadSchemas() {
  const fileNames = (await readdir(schemaDirectory))
    .filter((fileName) => fileName.endsWith(".schema.json"))
    .toSorted();
  return Promise.all(
    fileNames.map(async (fileName) => {
      const schema = JSON.parse(
        await readFile(path.join(schemaDirectory, fileName), "utf8"),
      );
      assert.equal(
        schema.$schema,
        "https://json-schema.org/draft/2020-12/schema",
        `${fileName} does not declare draft 2020-12`,
      );
      assert.equal(typeof schema.$id, "string", `${fileName} has no $id`);
      return { ...schema, __fileName: fileName };
    }),
  );
}

function compileSchemas(values) {
  const ajv = new Ajv2020({ allErrors: true, strict: true });
  addFormats(ajv);
  for (const { __fileName: _fileName, ...schema } of values)
    ajv.addSchema(schema);
  const compiled = new Map();
  for (const schema of values) {
    const validator = ajv.getSchema(schema.$id);
    assert.equal(
      typeof validator,
      "function",
      `${schema.__fileName} did not compile`,
    );
    compiled.set(schema.$id, validator);
  }
  return compiled;
}

function acceptedReceiptFixture() {
  const external = makeP2ExternalReceiptFixture();
  const matrix = {
    schema: 1,
    accepted: true,
    failures: [],
    verifiedAt: external.p2A12.verifiedAt,
    gitCommit: external.p2A12.gitCommit,
    workflow: {
      githubRunId: external.p2A12.verificationEnvironment.githubRunId,
      githubRunAttempt: external.p2A12.verificationEnvironment.githubRunAttempt,
      githubWorkflowRef:
        external.p2A12.verificationEnvironment.githubWorkflowRef,
      githubRepository: external.p2A12.verificationEnvironment.githubRepository,
      githubServerUrl: external.p2A12.verificationEnvironment.githubServerUrl,
      runUrl: external.p2A12.runUrl,
    },
    verificationEnvironment: structuredClone(
      external.p2A12.verificationEnvironment,
    ),
    artifacts: structuredClone(external.p2A12.artifacts),
  };
  const hostedRun = {
    schema: 1,
    accepted: true,
    failures: [],
    repository: external.p2A12.verificationEnvironment.githubRepository,
    verifiedAt: external.p2A12.hostedRunVerifiedAt,
    run: {
      databaseId: Number(external.p2A12.verificationEnvironment.githubRunId),
      attempt: Number(external.p2A12.githubRunAttempt),
      event: "workflow_dispatch",
      headBranch: "main",
      headSha: external.p2A12.gitCommit,
      status: "completed",
      conclusion: "success",
      workflowName: "CI",
      createdAt: "2026-08-20T02:00:00.000Z",
      startedAt: "2026-08-20T02:01:00.000Z",
      updatedAt: external.p2A12.hostedRunVerifiedAt,
      url: external.p2A12.runUrl,
    },
    jobs: structuredClone(external.p2A12.hostedJobs),
    archive: {
      gitCommit: external.p2A12.gitCommit,
      githubRunAttempt: external.p2A12.githubRunAttempt,
      runUrl: external.p2A12.runUrl,
      files: [
        `p2-a12-matrix-${external.p2A12.gitCommit}-attempt-${external.p2A12.githubRunAttempt}/p2-08-platform-matrix.json`,
        `p2-a12-source-Linux-${external.p2A12.gitCommit}-attempt-${external.p2A12.githubRunAttempt}/p2-a12-platform-paths.json`,
        `p2-a12-source-Windows-${external.p2A12.gitCommit}-attempt-${external.p2A12.githubRunAttempt}/p2-a12-platform-paths.json`,
        `p2-a12-source-macOS-${external.p2A12.gitCommit}-attempt-${external.p2A12.githubRunAttempt}/p2-a12-platform-paths.json`,
      ].map((relativePath, index) => ({
        relativePath,
        sha256: String(index + 1).repeat(64),
        bytes: 1_000 + index,
      })),
    },
    temporaryDownloadRemoved: true,
  };
  const localFaults = makeP2LocalFaultReceiptFixture();
  const defectRegister = {
    schema: 1,
    scope: "phase-2-exit",
    status: "reviewed",
    reviewedAt: "2026-08-20T03:30:00.000Z",
    findings: [],
  };
  const dataSafety = buildP2DataSafetyAuditReport({
    candidateCommit: TEST_P2_CANDIDATE_COMMIT,
    candidateCommittedAt: "2026-08-20T04:00:00.000Z",
    repositoryClean: true,
    commitOrderVerified: true,
    inputsCommitted: true,
    defectRegister,
    defectRegisterBytes: bytes(defectRegister),
    externalReceipt: external,
    externalBytes: bytes(external),
    localFaultReceipt: localFaults,
    localFaultBytes: bytes(localFaults),
    reportFiles: P2_DATA_SAFETY_REPORTS.map((fileName) => ({
      fileName,
      bytes: Buffer.from(fileName),
    })),
  });
  assert.equal(dataSafety.accepted, true, dataSafety.failures.join("; "));
  const finalExit = buildPhase2ExitGatesReport({
    externalBytes: bytes(external),
    externalReport: external,
    externalReplay: structuredClone(external),
    localFaultBytes: bytes(localFaults),
    localFaultReceipt: localFaults,
    dataSafetyBytes: bytes(dataSafety),
    dataSafetyReceipt: dataSafety,
    dataSafetyReplay: structuredClone(dataSafety),
    candidateCommit: TEST_P2_CANDIDATE_COMMIT,
    workingTreeClean: true,
    commitOrderVerified: true,
    externalEvidenceInLocalCandidate: true,
    localEvidenceCommitted: true,
    dataSafetyEvidenceCommitted: true,
    soakBaselineAudit: {
      schema: 1,
      accepted: true,
      failures: [],
      baselineCommit: FORMAL_SOAK_BASELINE_COMMIT,
      currentCommit: TEST_P2_CANDIDATE_COMMIT,
      descendantOfBaseline: true,
      loadedInputs: { changedPaths: [] },
      productInputs: { changedPaths: [] },
    },
    localCandidateDriftPaths: [],
  });
  assert.equal(finalExit.accepted, true, finalExit.failures.join("; "));
  return {
    matrix,
    hostedRun,
    external,
    localFaults,
    dataSafety,
    finalExit,
  };
}

function validatorFor(fileName) {
  const schema = schemas.find((candidate) => candidate.__fileName === fileName);
  assert.notEqual(schema, undefined, fileName);
  return validators.get(schema.$id);
}

function validate(fileName, value) {
  const validator = validatorFor(fileName);
  assert.equal(
    validator(value),
    true,
    `${fileName}: ${JSON.stringify(validator.errors)}`,
  );
}

function bytes(value) {
  return Buffer.from(`${JSON.stringify(value, null, 2)}\n`);
}
