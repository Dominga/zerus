import { Show, Suspense, createSignal, lazy, onCleanup, onMount } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { listRows } from "./sessions/model";
import { orderedRows } from "../store/catalog";
import { selectedKey, setSelectedKey } from "../store/ui";
import { activity } from "../store/session";
import { inTauri, metrics } from "../bridge";
import type { ScenarioResult } from "../perf/scenarios";

// The measurement panel (src/perf), loaded on demand: `?perf=1` in the window
// URL (ZERUS_CLIENT_QUERY=perf=1) or Ctrl+Shift+D. It drives the live session
// list and Activity and registers globalThis.zerusPerf.run().
//
// ZERUS_CLIENT_PERF=auto (or `?perf=auto`) runs it unattended: once the
// catalog has rows and the followed Activity has events, it runs every
// scenario, samples the process tree after 10 s idle, writes the report to
// ZERUS_CLIENT_PERF_OUT (save_perf) and exits the app. Each stage goes to the
// app's stderr (`zerus-client perf: ...`, no session names). A scenario that
// gets no frames ends as skipped after 60 s; the whole run ends after 8 minutes
// with what it has, so a hidden or occluded window never hangs it.

const Diagnostics = lazy(() => import("../perf/Diagnostics"));

interface PerfConfig {
  mode?: string | null;
  out?: string | null;
  label?: string | null;
  representative?: boolean;
  preference?: string;
}

/** Resolves once `ready` holds; fails after `ms`. Timers, not frames: rAF stops while a window is hidden. */
function until(ready: () => boolean, ms = 5000, what = "the session's Activity did not appear in time"): Promise<void> {
  const started = performance.now();
  return new Promise((resolve, reject) => {
    const check = () => {
      if (ready()) resolve();
      else if (performance.now() - started > ms) reject(new Error(what));
      else setTimeout(check, 16);
    };
    check();
  });
}

/** One line on the app's stderr in an unattended run. */
function log(line: string) {
  if (inTauri()) void invoke("perf_log", { line }).catch(() => {});
  else console.info(`zerus-client perf: ${line}`);
}

/** The whole unattended run ends after this, saving what it has. */
const RUN_TIMEOUT_MS = 8 * 60_000;

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/** Opens the card at `index` of the visible list and waits for its Activity in the DOM. */
async function openSession(index: number) {
  const cards = listRows().filter((r) => r.type === "card");
  if (!cards.length) throw new Error("no sessions in the list");
  const id = cards[index % cards.length].id;
  // Opening the open session again measures a real open, not a no-op.
  if (selectedKey() === id) {
    setSelectedKey(undefined);
    await new Promise(requestAnimationFrame);
  }
  setSelectedKey(id);
  await until(() => activity.target()?.id === id && activity.loaded() && !!document.querySelector(".act-scroll .act-col > :not(.act-label)"));
}

const scrollList = () => document.querySelector<HTMLElement>(".sessions-pane .sp-panel .sp-scroll");
const scrollActivity = () => document.querySelector<HTMLElement>(".act-scroll");

/** Scenario results as they finish, so a run cut short still saves them. */
const finished: ScenarioResult[] = [];

const toJson = (value: unknown) => JSON.stringify(value, (_key, v) => (typeof v === "number" && !Number.isFinite(v) ? null : v), 2);

/** The unattended run: wait for data, run every scenario, save, exit. */
async function autoRun(config: PerfConfig) {
  const begun = performance.now();
  const elapsed = () => `${((performance.now() - begun) / 1000).toFixed(1)} s`;
  let done = false;
  const finish = async (json: string, code: number) => {
    if (done) return;
    done = true;
    if (!inTauri()) { console.info("Zerus Next perf report", json); return; }
    try {
      const where = await invoke<string>("save_perf", { path: config.out ?? "", json });
      log(`saved ${json.length} bytes to ${where}`);
    } catch (error) {
      log(`saving failed: ${String(error)}`);
      code = code || 2;
    }
    log(`exit ${code} after ${elapsed()}`);
    await invoke("exit_app", { code }).catch(() => {});
  };
  const failed = (error: unknown) => finish(toJson({
    schema: "zerus-perf-v1", client: "tauri", error: String(error), created_at: new Date().toISOString(),
    platform: { label: config.label ?? null, representative: config.representative ?? null }, preference: config.preference ?? null,
    scenarios: finished,
  }), 1);
  const cutoff = setTimeout(() => { log(`run timeout after ${elapsed()}, saving ${finished.length} scenarios`); void failed("run timeout (8 min)"); }, RUN_TIMEOUT_MS);
  try {
    log(`start (preference ${config.preference ?? "n/a"}, visibility ${document.visibilityState})`);
    await until(() => orderedRows().length > 0, 180_000, "the catalog stayed empty for 3 minutes");
    log(`rows ready: ${orderedRows().length} after ${elapsed()}`);
    if (!selectedKey()) {
      const first = listRows().find((r) => r.type === "card");
      if (first) setSelectedKey(first.id);
    }
    const key = selectedKey();
    log(`selected: ${key ? `${key.startsWith("\n") ? "this computer" : "a peer"}, ${key.includes("\nrun:") ? "run key" : "name key"}` : "none"}`);
    // A session without recorded events still lets the list scenarios run.
    await until(() => activity.events().length > 0, 60_000).catch(() => {});
    log(`activity events: ${activity.events().length} after ${elapsed()}`);
    await until(() => !!globalThis.zerusPerf, 30_000, "the Diagnostics panel did not start");
    const report = await globalThis.zerusPerf!.run();
    log(`scenarios done after ${elapsed()}; idle 10 s`);
    await sleep(10_000);
    let idle: unknown;
    try { idle = await metrics(); } catch (error) { idle = { error: String(error) }; }
    report.resources = { ...(report.resources ?? {}), idle: idle ?? null };
    clearTimeout(cutoff);
    await finish(toJson({ ...report, preference: config.preference ?? null }), 0);
  } catch (error) {
    clearTimeout(cutoff);
    log(`failed: ${String(error)}`);
    await failed(error);
  }
}

const onScenarioStart = (id: string, index: number, total: number) => log(`scenario ${index + 1}/${total} ${id} start`);
const onScenarioEnd = (id: string, index: number, total: number, ms: number, result: ScenarioResult) => {
  finished.push(result);
  log(`scenario ${index + 1}/${total} ${id} end after ${(ms / 1000).toFixed(1)} s${result.skipped ? ` (skipped: ${result.skipped})` : ""}`);
};

export function PerfHost() {
  const urlMode = new URLSearchParams(location.search).get("perf");
  const [config, setConfig] = createSignal<PerfConfig>({ mode: urlMode });
  const [open, setOpen] = createSignal(urlMode === "1" || urlMode === "auto");
  const keys = (e: KeyboardEvent) => {
    if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key.toLowerCase() === "d") {
      e.preventDefault();
      setOpen(!open());
    }
  };
  onMount(async () => {
    addEventListener("keydown", keys);
    if (inTauri()) {
      const native = await invoke<PerfConfig>("perf_config").catch(() => undefined);
      if (native) setConfig({ ...native, mode: native.mode ?? urlMode });
    }
    if (config().mode === "auto") {
      setOpen(true);
      void autoRun(config());
    }
  });
  onCleanup(() => removeEventListener("keydown", keys));
  const auto = () => config().mode === "auto";
  return (
    <Show when={open()}>
      <aside class="perf-host" aria-label="Diagnostics">
        <Suspense>
          {/* The unattended run samples resources itself, after its own 10 s idle pause. */}
          <Diagnostics scrollList={scrollList} scrollActivity={scrollActivity} openSession={openSession}
            metrics={auto() ? undefined : metrics} label={config().label ?? undefined} representative={config().representative}
            onScenarioStart={auto() ? onScenarioStart : undefined} onScenarioEnd={auto() ? onScenarioEnd : undefined} />
        </Suspense>
      </aside>
    </Show>
  );
}
