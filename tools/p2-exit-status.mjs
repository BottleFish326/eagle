export const P2_EXIT_STAGES = Object.freeze([
  "evidence-conflict",
  "final-exit-invalid",
  "final-exit-uncommitted",
  "soak-missing",
  "soak-running",
  "soak-failed",
  "hosted-environment-blocked",
  "hosted-run-pending",
  "external-gates-pending",
  "external-gates-invalid",
  "local-faults-pending",
  "local-faults-invalid",
  "local-faults-uncommitted",
  "candidate-dirty",
  "final-exit-pending",
  "accepted",
]);

export function buildP2ExitStatus({
  git,
  soak,
  hostedReadiness,
  externalGates,
  localFaults,
  finalExit,
}) {
  const inputs = normalizeInputs({
    git,
    soak,
    hostedReadiness,
    externalGates,
    localFaults,
    finalExit,
  });
  const decision = decide(inputs);
  return {
    schema: 1,
    stage: decision.stage,
    readyToAdvanceStage3: decision.stage === "accepted",
    failures: decision.failures,
    git: inputs.git,
    gates: {
      p2A11: inputs.soak,
      p2A12: {
        hostedReady: inputs.hostedReadiness.ready,
        hostedFailures: [...inputs.hostedReadiness.failures],
        hostedRemediations: inputs.hostedReadiness.remediations.map(
          (entry) => ({
            ...entry,
          }),
        ),
        externalGates: inputs.externalGates,
      },
      localFaults: inputs.localFaults,
      finalExit: inputs.finalExit,
    },
    nextAction: decision.nextAction,
  };
}

function decide(inputs) {
  const { git, soak, hostedReadiness, externalGates, localFaults, finalExit } =
    inputs;

  if (finalExit.state === "invalid")
    return decision(
      "final-exit-invalid",
      prefixed("final exit", finalExit.failures),
      "inspect-final-exit",
      "npm run inspect:p2-exit",
      "The archived final exit receipt is invalid and must not be accepted.",
    );

  if (finalExit.state === "accepted") {
    const upstreamAccepted =
      soak.state === "passed" &&
      externalGates.state === "accepted" &&
      localFaults.state === "accepted" &&
      localFaults.committed === true;
    if (!upstreamAccepted)
      return decision(
        "evidence-conflict",
        ["final exit evidence conflicts with one or more upstream gate states"],
        "inspect-evidence",
        "npm run status:p2-exit",
        "Resolve the contradictory evidence set before any stage transition.",
      );
    if (finalExit.committed !== true)
      return decision(
        "final-exit-uncommitted",
        [],
        "commit-final-exit",
        "npm run inspect:p2-exit",
        "Inspect and commit the accepted final exit receipt and acceptance docs.",
      );
    return decision(
      "accepted",
      [],
      null,
      null,
      "Phase 2 evidence is complete and committed; stage 3 may advance.",
    );
  }

  if (soak.state === "missing")
    return decision(
      "soak-missing",
      [],
      "start-soak",
      "npm run test:resource-stability",
      "Run the complete formal eight-hour P2-A11 test.",
    );
  if (soak.state === "running")
    return decision(
      "soak-running",
      [],
      "wait-for-soak",
      "npm exec --yes --package=node@24 -- node tools/inspect-resource-stability-checkpoint.mjs docs/reports/evidence/p2-06-resource-soak.json.partial",
      "Keep the formal process isolated and wait for its final receipt.",
    );
  if (soak.state === "failed")
    return decision(
      "soak-failed",
      prefixed("P2-A11", soak.failures),
      "preserve-and-rerun-soak",
      null,
      "Preserve the failed evidence, correct the cause, and rerun all eight hours.",
    );

  if (externalGates.state === "invalid")
    return decision(
      "external-gates-invalid",
      prefixed("external gates", externalGates.failures),
      "rebuild-external-gates",
      "npm run verify:p2-external",
      "Rebuild external evidence only from the complete original inputs.",
    );
  if (externalGates.state === "ready")
    return decision(
      "external-gates-pending",
      [],
      "verify-external-gates",
      "npm run verify:p2-external",
      "Replay P2-A11 and P2-A12 into the immutable external gate receipt.",
    );
  if (externalGates.state !== "accepted") {
    if (hostedReadiness.ready !== true) {
      const remediation = hostedReadiness.remediations[0];
      return decision(
        "hosted-environment-blocked",
        prefixed("P2-A12 hosted readiness", hostedReadiness.failures),
        remediation?.kind ?? "prepare-hosted-environment",
        remediation?.command ?? "npm run audit:p2-hosted-readiness",
        remediation?.message ??
          "Prepare and publish the exact GitHub main candidate before triggering P2-A12.",
      );
    }
    return decision(
      "hosted-run-pending",
      [],
      "trigger-hosted-run",
      hostedReadiness.commands[0] ?? "npm run audit:p2-hosted-readiness",
      "Trigger the commit-bound manual CI run, then collect its exact attempt.",
    );
  }

  if (localFaults.state === "invalid")
    return decision(
      "local-faults-invalid",
      prefixed("local faults", localFaults.failures),
      "preserve-local-fault-evidence",
      null,
      "Do not overwrite the receipt; preserve evidence and rerun from a new clean candidate.",
    );
  if (localFaults.state === "missing")
    return decision(
      "local-faults-pending",
      [],
      "run-local-faults",
      "npm run test:p2-local-fault-gates",
      "Run all three real-process crash boundaries from the clean external-evidence candidate.",
    );
  if (localFaults.committed !== true)
    return decision(
      "local-faults-uncommitted",
      [],
      "commit-local-faults",
      "git status --short",
      "Inspect and commit the local fault receipt plus documentation before final verification.",
    );
  if (git.cleanAll !== true)
    return decision(
      "candidate-dirty",
      [],
      "clean-final-candidate",
      "git status --short",
      "Commit the allowed documentation/evidence changes before final verification.",
    );
  return decision(
    "final-exit-pending",
    [],
    "verify-final-exit",
    "npm run verify:p2-exit",
    "Generate the final accepted-only Phase 2 exit receipt.",
  );
}

function normalizeInputs(value) {
  return {
    git: {
      currentCommit: value.git?.currentCommit ?? null,
      cleanTracked: value.git?.cleanTracked === true,
      cleanAll: value.git?.cleanAll === true,
    },
    soak: normalizeGate(value.soak, ["missing", "running", "passed", "failed"]),
    hostedReadiness: {
      ready: value.hostedReadiness?.ready === true,
      failures: stringArray(value.hostedReadiness?.failures),
      commands: stringArray(value.hostedReadiness?.commands),
      remediations: remediationArray(value.hostedReadiness?.remediations),
    },
    externalGates: normalizeGate(value.externalGates, [
      "missing",
      "ready",
      "accepted",
      "invalid",
    ]),
    localFaults: normalizeReceipt(value.localFaults),
    finalExit: normalizeReceipt(value.finalExit),
  };
}

function normalizeGate(value, allowed) {
  return {
    state: allowed.includes(value?.state) ? value.state : "failed",
    failures: stringArray(value?.failures),
    summary: value?.summary ?? null,
  };
}

function normalizeReceipt(value) {
  return {
    state: ["missing", "accepted", "invalid"].includes(value?.state)
      ? value.state
      : "invalid",
    failures: stringArray(value?.failures),
    committed: value?.committed === true,
    summary: value?.summary ?? null,
  };
}

function stringArray(value) {
  return Array.isArray(value)
    ? value.filter((entry) => typeof entry === "string")
    : [];
}

function remediationArray(value) {
  if (!Array.isArray(value)) return [];
  return value.flatMap((entry) =>
    typeof entry?.kind === "string" &&
    (typeof entry?.command === "string" || entry?.command === null) &&
    typeof entry?.message === "string"
      ? [
          {
            kind: entry.kind,
            command: entry.command,
            message: entry.message,
          },
        ]
      : [],
  );
}

function prefixed(label, failures) {
  return failures.length === 0
    ? [`${label}: invalid evidence without a diagnostic`]
    : failures.map((failure) => `${label}: ${failure}`);
}

function decision(stage, failures, kind, command, message) {
  return {
    stage,
    failures,
    nextAction:
      kind === null
        ? null
        : {
            kind,
            command,
            message,
          },
  };
}
