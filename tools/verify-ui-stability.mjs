import { spawn } from "node:child_process";
import { access, mkdtemp, readFile, rm } from "node:fs/promises";
import net from "node:net";
import os from "node:os";
import path from "node:path";

const repository = path.resolve(import.meta.dirname, "..");
const desktop = path.join(repository, "apps", "desktop");
const vite = path.join(desktop, "node_modules", "vite", "bin", "vite.js");
const defaults = {
  chrome: "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  durationSeconds: 1_800,
  warmupSeconds: 60,
};
const options = parseArguments(process.argv.slice(2));

let viteProcess;
let chromeProcess;
let connection;
let profileDirectory;

async function main() {
  try {
    await access(options.chrome);
    await access(vite);
    const port = await availablePort();
    const viteOutput = bufferedOutput();
    viteProcess = spawn(
      process.execPath,
      [vite, "--host", "127.0.0.1", "--port", String(port), "--strictPort"],
      { cwd: desktop, stdio: ["ignore", "pipe", "pipe"] },
    );
    viteProcess.stdout.on("data", viteOutput.append);
    viteProcess.stderr.on("data", viteOutput.append);
    const baseUrl = `http://127.0.0.1:${port}`;
    await waitForHttp(baseUrl, viteProcess, viteOutput, 20_000);

    profileDirectory = await mkdtemp(
      path.join(os.tmpdir(), "material-eagle-stability-chrome-"),
    );
    const pageUrl = `${baseUrl}/?demoDataset=medium&stabilitySeconds=${options.durationSeconds}&stabilityWarmupSeconds=${options.warmupSeconds}&run=standalone`;
    const chromeOutput = bufferedOutput();
    chromeProcess = spawn(
      options.chrome,
      [
        "--headless=new",
        "--disable-background-networking",
        "--disable-component-update",
        "--disable-default-apps",
        "--disable-extensions",
        "--disable-sync",
        "--metrics-recording-only",
        "--no-default-browser-check",
        "--no-first-run",
        "--remote-debugging-port=0",
        `--user-data-dir=${profileDirectory}`,
        pageUrl,
      ],
      { stdio: ["ignore", "ignore", "pipe"] },
    );
    chromeProcess.stderr.on("data", chromeOutput.append);
    const debugPort = await readDebugPort(
      profileDirectory,
      chromeProcess,
      chromeOutput,
      20_000,
    );
    const target = await findPageTarget(debugPort, pageUrl, 20_000);
    connection = await CdpConnection.open(target.webSocketDebuggerUrl);
    await connection.send("Runtime.enable");

    const state = await waitForCompleteReport({
      connection,
      chromeProcess,
      durationSeconds: options.durationSeconds,
      warmupSeconds: options.warmupSeconds,
    });
    const evidence = buildEvidence(state);
    if (!state.summary?.accepted) {
      throw new Error(
        `UI stability rejected: ${JSON.stringify(state.summary?.failures ?? state.errors)}`,
      );
    }
    console.log(`UI stability accepted ${JSON.stringify(evidence)}`);
  } finally {
    if (connection !== undefined) {
      try {
        await connection.send("Browser.close");
      } catch {
        // The browser may already be closing after a failed run.
      }
      connection.close();
    }
    await stopProcess(chromeProcess);
    await stopProcess(viteProcess);
    if (profileDirectory !== undefined) {
      await rm(profileDirectory, {
        recursive: true,
        force: true,
        maxRetries: 3,
      });
    }
  }
}

async function waitForCompleteReport({
  connection: cdp,
  chromeProcess: chrome,
  durationSeconds,
  warmupSeconds,
}) {
  const deadline =
    Date.now() + (durationSeconds + warmupSeconds) * 1_000 + 120_000;
  let continuousStartedAt;
  let nextProgressAt = Date.now();
  while (Date.now() < deadline) {
    if (chrome.exitCode !== null) {
      throw new Error(
        `Chrome exited before stability completion (${chrome.exitCode})`,
      );
    }
    const report = await readReport(cdp);
    if (report !== null) {
      if (
        continuousStartedAt !== undefined &&
        report.startedAt !== continuousStartedAt
      ) {
        throw new Error("stability page reloaded during the measured interval");
      }
      if (report.startedAt !== undefined)
        continuousStartedAt = report.startedAt;
      if (Date.now() >= nextProgressAt) {
        const last = report.samples.at(-1);
        console.log(
          [
            "UI stability progress",
            `status=${report.status}`,
            `elapsedMs=${last?.elapsedMs ?? 0}`,
            `samples=${report.samples.length}`,
            `actions=${report.actionCount}`,
            `observations=${report.resultObservations.length}`,
            `errors=${report.errors.length}`,
          ].join(" "),
        );
        nextProgressAt = Date.now() + 60_000;
      }
      if (report.status === "failed") {
        throw new Error(
          `stability harness failed: ${report.errors.join("; ")}`,
        );
      }
      if (report.status === "complete") return report;
    }
    await delay(1_000);
  }
  throw new Error("UI stability harness exceeded its completion deadline");
}

async function readReport(cdp) {
  const response = await cdp.send("Runtime.evaluate", {
    expression:
      'document.querySelector("#material-eagle-stability-report")?.textContent ?? null',
    returnByValue: true,
  });
  if (response.exceptionDetails !== undefined) {
    throw new Error("failed to read the UI stability report");
  }
  const value = response.result.value;
  return value === null ? null : JSON.parse(value);
}

function buildEvidence(state) {
  const targets = [
    0, 300_000, 600_000, 900_000, 1_200_000, 1_500_000, 1_800_000,
  ].filter((target) => target <= state.durationMs);
  const milestones = targets.map((target) => {
    const sample = state.samples.reduce((best, candidate) =>
      Math.abs(candidate.elapsedMs - target) < Math.abs(best.elapsedMs - target)
        ? candidate
        : best,
    );
    return {
      targetMinutes: target / 60_000,
      elapsedMs: sample.elapsedMs,
      heapMiB:
        sample.usedJsHeapBytes === null
          ? null
          : round(sample.usedJsHeapBytes / 1_048_576, 1),
      domNodes: sample.domNodes,
      assetCards: sample.assetCards,
      resultCount: sample.resultCount,
      eventLoopLagMs: sample.eventLoopLagMs,
    };
  });
  return {
    startedAt: state.startedAt,
    completedAt: state.completedAt,
    datasetSize: state.datasetSize,
    warmupMs: state.warmupMs,
    durationMs: state.durationMs,
    observationCount: state.resultObservations.length,
    summary: state.summary,
    milestones,
  };
}

class CdpConnection {
  constructor(socket) {
    this.socket = socket;
    this.nextId = 1;
    this.pending = new Map();
    socket.addEventListener("message", (event) => {
      const message = JSON.parse(event.data);
      if (message.id === undefined) return;
      const pending = this.pending.get(message.id);
      if (pending === undefined) return;
      this.pending.delete(message.id);
      clearTimeout(pending.timer);
      if (message.error !== undefined) {
        pending.reject(new Error(message.error.message));
      } else {
        pending.resolve(message.result);
      }
    });
    socket.addEventListener("close", () => {
      for (const pending of this.pending.values()) {
        clearTimeout(pending.timer);
        pending.reject(new Error("Chrome DevTools connection closed"));
      }
      this.pending.clear();
    });
  }

  static async open(url) {
    const socket = new WebSocket(url);
    await new Promise((resolve, reject) => {
      socket.addEventListener("open", resolve, { once: true });
      socket.addEventListener("error", reject, { once: true });
    });
    return new CdpConnection(socket);
  }

  send(method, params = {}, timeoutMs = 5_000) {
    const id = this.nextId;
    this.nextId += 1;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(
          new Error(
            `Chrome DevTools ${method} timed out after ${timeoutMs} ms`,
          ),
        );
      }, timeoutMs);
      this.pending.set(id, { resolve, reject, timer });
      this.socket.send(JSON.stringify({ id, method, params }));
    });
  }

  close() {
    this.socket.close();
  }
}

async function readDebugPort(directory, processHandle, output, timeoutMs) {
  const file = path.join(directory, "DevToolsActivePort");
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (processHandle.exitCode !== null) {
      throw new Error(`Chrome exited during startup: ${output.value()}`);
    }
    try {
      const [port] = (await readFile(file, "utf8")).trim().split("\n");
      if (/^\d+$/u.test(port)) return Number(port);
    } catch {
      // Chrome creates the file after its debugging endpoint is ready.
    }
    await delay(100);
  }
  throw new Error(`Chrome did not expose DevTools: ${output.value()}`);
}

async function findPageTarget(port, expectedUrl, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const targets = await fetch(`http://127.0.0.1:${port}/json/list`)
      .then((response) => response.json())
      .catch(() => []);
    const target = targets.find(
      (candidate) =>
        candidate.type === "page" && candidate.url.startsWith(expectedUrl),
    );
    if (target !== undefined) return target;
    await delay(100);
  }
  throw new Error("Chrome did not open the stability page");
}

async function waitForHttp(url, processHandle, output, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (processHandle.exitCode !== null) {
      throw new Error(`Vite exited during startup: ${output.value()}`);
    }
    try {
      const response = await fetch(url);
      if (response.ok) return;
    } catch {
      // The server has not opened the socket yet.
    }
    await delay(100);
  }
  throw new Error(`Vite did not become ready: ${output.value()}`);
}

async function availablePort() {
  const server = net.createServer();
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const address = server.address();
  const port =
    typeof address === "object" && address !== null ? address.port : 0;
  await new Promise((resolve, reject) =>
    server.close((error) => (error ? reject(error) : resolve())),
  );
  if (port === 0) throw new Error("failed to reserve a Vite port");
  return port;
}

async function stopProcess(processHandle) {
  if (
    processHandle === undefined ||
    processHandle.exitCode !== null ||
    processHandle.signalCode !== null
  )
    return;
  processHandle.kill("SIGTERM");
  await Promise.race([
    new Promise((resolve) => processHandle.once("exit", resolve)),
    delay(3_000),
  ]);
  if (processHandle.exitCode === null && processHandle.signalCode === null) {
    processHandle.kill("SIGKILL");
  }
}

function bufferedOutput() {
  let value = "";
  return {
    append(chunk) {
      value = `${value}${chunk}`.slice(-8_192);
    },
    value() {
      return value.trim();
    },
  };
}

function parseArguments(argumentsList) {
  const parsed = { ...defaults };
  for (let index = 0; index < argumentsList.length; index += 2) {
    const name = argumentsList[index];
    const value = argumentsList[index + 1];
    if (value === undefined) throw new Error(`missing value for ${name}`);
    if (name === "--chrome") parsed.chrome = value;
    else if (name === "--duration-seconds")
      parsed.durationSeconds = Number(value);
    else if (name === "--warmup-seconds") parsed.warmupSeconds = Number(value);
    else throw new Error(`unknown argument ${name}`);
  }
  if (
    !Number.isInteger(parsed.durationSeconds) ||
    parsed.durationSeconds < 10 ||
    parsed.durationSeconds > 3_600
  ) {
    throw new Error("duration seconds must be an integer from 10 to 3600");
  }
  if (
    !Number.isInteger(parsed.warmupSeconds) ||
    parsed.warmupSeconds < 0 ||
    parsed.warmupSeconds > 300
  ) {
    throw new Error("warmup seconds must be an integer from 0 to 300");
  }
  return parsed;
}

function round(value, decimals) {
  const factor = 10 ** decimals;
  return Math.round(value * factor) / factor;
}

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

await main();
