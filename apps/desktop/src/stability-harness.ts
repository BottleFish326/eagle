export interface StabilitySample {
  elapsedMs: number;
  usedJsHeapBytes: number | null;
  totalJsHeapBytes: number | null;
  domNodes: number;
  assetCards: number;
  resultCount: number | null;
  eventLoopLagMs: number;
}

export interface StabilityResultObservation {
  query: string;
  expected: number;
  observed: number | null;
  elapsedMs: number;
}

export interface StabilitySummary {
  sampleCount: number;
  actionCount: number;
  firstUsedJsHeapBytes: number | null;
  lastUsedJsHeapBytes: number | null;
  maxUsedJsHeapBytes: number | null;
  heapGrowthBytes: number | null;
  heapSlopeBytesPerMinute: number | null;
  maxDomNodes: number;
  maxAssetCards: number;
  eventLoopLagP95Ms: number;
  eventLoopLagMaxMs: number;
  longTaskCount: number;
  longTaskDurationMs: number;
  longTaskRatio: number;
  accepted: boolean;
  failures: string[];
}

export interface StabilityHarnessState {
  schema: 1;
  datasetSize: number;
  warmupMs: number;
  durationMs: number;
  status: "warming-up" | "running" | "complete" | "failed";
  startedAt?: string;
  completedAt?: string;
  samples: StabilitySample[];
  resultObservations: StabilityResultObservation[];
  actionCount: number;
  longTaskCount: number;
  longTaskDurationMs: number;
  errors: string[];
  summary?: StabilitySummary;
}

declare global {
  interface Window {
    __MATERIAL_EAGLE_STABILITY__?: StabilityHarnessState;
  }
}

interface BrowserMemory {
  usedJSHeapSize: number;
  totalJSHeapSize: number;
}

const DATASET_SIZE = 10_000;
const SAMPLE_INTERVAL_MS = 5_000;
const FILTER_INTERVAL_MS = 8_000;
const SCROLL_INTERVAL_MS = 1_000;
const HEAP_GROWTH_LIMIT = 128 * 1024 * 1024;
const HEAP_SLOPE_LIMIT_PER_MINUTE = 4 * 1024 * 1024;
const EVENT_LOOP_LAG_P95_LIMIT_MS = 1_500;
const LONG_TASK_RATIO_LIMIT = 0.35;
const VIRTUAL_CARD_LIMIT = 120;

const filterCases = [
  { query: "", expected: 10_000 },
  { query: "favorite:true", expected: 3_750 },
  { query: "color/blue", expected: 2_500 },
  {
    query: "any:(color/green|color/red) -state/review",
    expected: 3_125,
  },
] as const;

export function installStabilityHarnessFromLocation(search: string): void {
  const seconds = Number(
    new URLSearchParams(search).get("stabilitySeconds") ?? "",
  );
  const warmupSeconds = Number(
    new URLSearchParams(search).get("stabilityWarmupSeconds") ?? "60",
  );
  if (!Number.isFinite(seconds) || seconds < 10 || seconds > 3_600) return;
  if (
    !Number.isFinite(warmupSeconds) ||
    warmupSeconds < 0 ||
    warmupSeconds > 300
  )
    return;
  if (window.__MATERIAL_EAGLE_STABILITY__ !== undefined) return;

  const state: StabilityHarnessState = {
    schema: 1,
    datasetSize: DATASET_SIZE,
    warmupMs: Math.round(warmupSeconds * 1_000),
    durationMs: Math.round(seconds * 1_000),
    status: "warming-up",
    samples: [],
    resultObservations: [],
    actionCount: 0,
    longTaskCount: 0,
    longTaskDurationMs: 0,
    errors: [],
  };
  window.__MATERIAL_EAGLE_STABILITY__ = state;
  publishState(state);
  void waitForMediumDataset(120_000)
    .then(() => warmUpMediumDataset(state))
    .then(() => runStabilityHarness(state))
    .catch((error: unknown) => {
      state.status = "failed";
      state.completedAt = new Date().toISOString();
      state.errors.push(errorMessage(error));
      state.summary = summarizeStabilityState(state);
      publishState(state);
    });
}

async function warmUpMediumDataset(
  state: StabilityHarnessState,
): Promise<void> {
  if (state.warmupMs === 0) return;
  const search = document.querySelector<HTMLInputElement>(
    'input[aria-label="搜索和过滤素材"]',
  );
  if (search === null) throw new Error("stability search input is unavailable");
  const started = performance.now();
  let filterIndex = 0;
  while (performance.now() - started < state.warmupMs) {
    const filter = filterCases[filterIndex % filterCases.length];
    filterIndex += 1;
    setInputValue(search, filter.query);
    window.scrollTo({
      behavior: "auto",
      top: document.documentElement.scrollHeight,
    });
    await delay(Math.min(2_000, state.warmupMs));
    window.scrollTo({ behavior: "auto", top: 0 });
    await delay(Math.min(2_000, state.warmupMs));
    publishState(state);
  }
  setInputValue(search, "");
  window.scrollTo({ behavior: "auto", top: 0 });
  await delay(1_000);
}

async function waitForMediumDataset(timeoutMs: number): Promise<void> {
  const deadline = performance.now() + timeoutMs;
  while (performance.now() < deadline) {
    const discovered = parseLocalizedCount(
      document.querySelector(".sidebar-heading p")?.textContent,
    );
    const scanning = document.querySelector(".scan-progress") !== null;
    if (discovered === DATASET_SIZE && !scanning) return;
    await delay(250);
  }
  throw new Error("M dataset did not finish loading before the stability test");
}

async function runStabilityHarness(
  state: StabilityHarnessState,
): Promise<void> {
  const search = document.querySelector<HTMLInputElement>(
    'input[aria-label="搜索和过滤素材"]',
  );
  if (search === null) throw new Error("stability search input is unavailable");

  state.status = "running";
  state.startedAt = new Date().toISOString();
  publishState(state);
  const started = performance.now();
  let expectedSampleAt = started;
  let filterIndex = 0;
  let scrollTop = 0;
  let scrollDirection = 1;
  const observer = installLongTaskObserver(state);

  const capture = () => {
    const now = performance.now();
    const memory = readBrowserMemory();
    state.samples.push({
      elapsedMs: Math.round(now - started),
      usedJsHeapBytes: memory?.usedJSHeapSize ?? null,
      totalJsHeapBytes: memory?.totalJSHeapSize ?? null,
      domNodes: document.getElementsByTagName("*").length,
      assetCards: document.querySelectorAll("[data-asset-card]").length,
      resultCount: currentResultCount(),
      eventLoopLagMs: Math.max(0, Math.round(now - expectedSampleAt)),
    });
    publishState(state);
    expectedSampleAt = now + SAMPLE_INTERVAL_MS;
  };
  capture();

  const sampleTimer = window.setInterval(capture, SAMPLE_INTERVAL_MS);
  const scrollTimer = window.setInterval(() => {
    const maximum = Math.max(
      0,
      document.documentElement.scrollHeight - window.innerHeight,
    );
    scrollTop += scrollDirection * Math.max(320, window.innerHeight * 0.8);
    if (scrollTop >= maximum) {
      scrollTop = maximum;
      scrollDirection = -1;
    } else if (scrollTop <= 0) {
      scrollTop = 0;
      scrollDirection = 1;
    }
    window.scrollTo({
      behavior: "auto",
      top: scrollTop,
    });
    state.actionCount += 1;
    publishState(state);
  }, SCROLL_INTERVAL_MS);
  const filterTimer = window.setInterval(() => {
    const filter = filterCases[filterIndex % filterCases.length];
    filterIndex += 1;
    setInputValue(search, filter.query);
    state.actionCount += 1;
    void observeResult(state, filter.query, filter.expected, started);
  }, FILTER_INTERVAL_MS);

  await delay(state.durationMs);
  window.clearInterval(sampleTimer);
  window.clearInterval(scrollTimer);
  window.clearInterval(filterTimer);
  observer?.disconnect();
  capture();
  state.status = "complete";
  state.completedAt = new Date().toISOString();
  state.summary = summarizeStabilityState(state);
  publishState(state);
}

function installLongTaskObserver(
  state: StabilityHarnessState,
): PerformanceObserver | undefined {
  if (!("PerformanceObserver" in window)) return undefined;
  try {
    const observer = new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) {
        state.longTaskCount += 1;
        state.longTaskDurationMs += entry.duration;
      }
    });
    observer.observe({ type: "longtask" });
    return observer;
  } catch {
    return undefined;
  }
}

async function observeResult(
  state: StabilityHarnessState,
  query: string,
  expected: number,
  started: number,
): Promise<void> {
  const deadline = performance.now() + FILTER_INTERVAL_MS - 500;
  let observed: number | null = null;
  while (performance.now() < deadline) {
    observed = currentResultCount();
    if (observed === expected) break;
    await delay(100);
  }
  state.resultObservations.push({
    query,
    expected,
    observed,
    elapsedMs: Math.round(performance.now() - started),
  });
  if (observed !== expected) {
    state.errors.push(
      `query ${JSON.stringify(query)} returned ${String(observed)}, expected ${expected}`,
    );
  }
  publishState(state);
}

function setInputValue(input: HTMLInputElement, value: string): void {
  const setter = Object.getOwnPropertyDescriptor(
    HTMLInputElement.prototype,
    "value",
  )?.set;
  setter?.call(input, value);
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

function currentResultCount(): number | null {
  return parseLocalizedCount(
    document.querySelector(".result-meta > span")?.textContent,
  );
}

function parseLocalizedCount(value: string | null | undefined): number | null {
  const match = value?.match(/([\d,]+)\s*项/u);
  return match ? Number(match[1].replaceAll(",", "")) : null;
}

function readBrowserMemory(): BrowserMemory | undefined {
  return (performance as Performance & { memory?: BrowserMemory }).memory;
}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, milliseconds));
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function publishState(state: StabilityHarnessState): void {
  let report = document.querySelector<HTMLScriptElement>(
    "#material-eagle-stability-report",
  );
  if (report === null) {
    report = document.createElement("script");
    report.id = "material-eagle-stability-report";
    report.type = "application/json";
    document.body.append(report);
  }
  report.textContent = JSON.stringify(state);
}

export function summarizeStabilityState(
  state: StabilityHarnessState,
): StabilitySummary {
  const heapSamples = state.samples.filter(
    (sample): sample is StabilitySample & { usedJsHeapBytes: number } =>
      sample.usedJsHeapBytes !== null,
  );
  const firstHeap = heapSamples.at(0)?.usedJsHeapBytes ?? null;
  const lastHeap = heapSamples.at(-1)?.usedJsHeapBytes ?? null;
  const maxHeap =
    heapSamples.length === 0
      ? null
      : Math.max(...heapSamples.map((sample) => sample.usedJsHeapBytes));
  const heapGrowth =
    firstHeap === null || lastHeap === null ? null : lastHeap - firstHeap;
  const heapSlope = linearHeapSlopePerMinute(heapSamples);
  const lags = state.samples
    .map((sample) => sample.eventLoopLagMs)
    .sort((left, right) => left - right);
  const elapsed = Math.max(
    state.durationMs,
    state.samples.at(-1)?.elapsedMs ?? 0,
  );
  const longTaskRatio = elapsed === 0 ? 0 : state.longTaskDurationMs / elapsed;
  const failures = [...state.errors];
  if (state.status !== "complete") failures.push(`status is ${state.status}`);
  if (heapGrowth === null || heapSlope === null) {
    failures.push("browser JavaScript heap metrics are unavailable");
  } else {
    if (heapGrowth > HEAP_GROWTH_LIMIT) {
      failures.push(`heap growth ${heapGrowth} exceeds ${HEAP_GROWTH_LIMIT}`);
    }
    if (heapSlope > HEAP_SLOPE_LIMIT_PER_MINUTE) {
      failures.push(
        `heap slope ${heapSlope} exceeds ${HEAP_SLOPE_LIMIT_PER_MINUTE} bytes/minute`,
      );
    }
  }
  const lagP95 = percentile(lags, 95);
  if (lagP95 > EVENT_LOOP_LAG_P95_LIMIT_MS) {
    failures.push(
      `event loop lag p95 ${lagP95} ms exceeds ${EVENT_LOOP_LAG_P95_LIMIT_MS} ms`,
    );
  }
  if (longTaskRatio > LONG_TASK_RATIO_LIMIT) {
    failures.push(
      `long task ratio ${longTaskRatio} exceeds ${LONG_TASK_RATIO_LIMIT}`,
    );
  }
  const maxCards = Math.max(
    0,
    ...state.samples.map((sample) => sample.assetCards),
  );
  if (maxCards > VIRTUAL_CARD_LIMIT) {
    failures.push(
      `asset card count ${maxCards} exceeds virtual window limit ${VIRTUAL_CARD_LIMIT}`,
    );
  }
  if (
    state.resultObservations.some(
      (result) => result.observed !== result.expected,
    )
  ) {
    failures.push("one or more filter result counts were unstable");
  }

  return {
    sampleCount: state.samples.length,
    actionCount: state.actionCount,
    firstUsedJsHeapBytes: firstHeap,
    lastUsedJsHeapBytes: lastHeap,
    maxUsedJsHeapBytes: maxHeap,
    heapGrowthBytes: heapGrowth,
    heapSlopeBytesPerMinute: heapSlope,
    maxDomNodes: Math.max(0, ...state.samples.map((sample) => sample.domNodes)),
    maxAssetCards: maxCards,
    eventLoopLagP95Ms: lagP95,
    eventLoopLagMaxMs: Math.max(0, ...lags),
    longTaskCount: state.longTaskCount,
    longTaskDurationMs: Math.round(state.longTaskDurationMs),
    longTaskRatio,
    accepted: failures.length === 0,
    failures,
  };
}

function linearHeapSlopePerMinute(
  samples: Array<StabilitySample & { usedJsHeapBytes: number }>,
): number | null {
  if (samples.length < 2) return null;
  const xMean =
    samples.reduce((sum, sample) => sum + sample.elapsedMs, 0) / samples.length;
  const yMean =
    samples.reduce((sum, sample) => sum + sample.usedJsHeapBytes, 0) /
    samples.length;
  let numerator = 0;
  let denominator = 0;
  for (const sample of samples) {
    const x = sample.elapsedMs - xMean;
    numerator += x * (sample.usedJsHeapBytes - yMean);
    denominator += x * x;
  }
  return denominator === 0 ? 0 : (numerator / denominator) * 60_000;
}

function percentile(values: number[], percentage: number): number {
  if (values.length === 0) return 0;
  return values[
    Math.min(
      values.length - 1,
      Math.floor((values.length - 1) * (percentage / 100)),
    )
  ];
}
