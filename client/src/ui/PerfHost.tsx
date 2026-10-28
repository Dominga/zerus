import { Show, Suspense, createSignal, lazy, onCleanup, onMount } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { listRows } from "./sessions/model";
import { orderedRows } from "../store/catalog";
import { selectedKey, setSelectedKey } from "../store/ui";
import { activity } from "../store/session";
import { end, perform } from "../store/actions";
import { snapshotOf, terminateCopy, type ActionRow } from "./sessions/actions-ui";
import type { Action, Outcome } from "../types";
import { inTauri, metrics, pumpStats } from "../bridge";
import { restoreSettings, setSetting } from "../store/settings";
import type { ScenarioResult } from "../perf/scenarios";

// The measurement panel (src/perf), loaded on demand: `?perf=1` in the window
// URL (ZERUS_CLIENT_QUERY=perf=1) or Ctrl+Shift+D. It drives the live session
// list and Activity and registers globalThis.zerusPerf.run().
//
// ZERUS_CLIENT_PERF=auto (or `?perf=auto`) runs it unattended: once the
// catalog has rows and the followed Activity has events, it runs every
// scenario, samples the process tree after 10 s idle, writes the report to
// ZERUS_CLIENT_PERF_OUT (save_perf) and exits the app. The idle windows run
// as displayed, with Animate working sessions off, with Reduce motion (both
// tried without being stored), with the window hidden and then minimised
// (perf_away), each followed by a window as displayed again, all 10 s. Each stage goes to the
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
  display?: string | null;
  actions_smoke?: boolean;
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

/** The display's refresh rate from the native side (display_hz); null when unknown, then the panel estimates. */
async function displayHz(): Promise<number | null> {
  if (!inTauri()) return null;
  const hz = await invoke<number | null>("display_hz").catch(() => null);
  return typeof hz === "number" && hz > 0 ? hz : null;
}

/** Animation frames delivered within `ms` (a timer ends the count, so none still resolves). */
function countFrames(ms: number): Promise<number> {
  return new Promise((resolve) => {
    let count = 0;
    let running = true;
    const tick = () => { if (!running) return; count++; requestAnimationFrame(tick); };
    requestAnimationFrame(tick);
    setTimeout(() => { running = false; resolve(count); }, ms);
  });
}

/** Animations by play state ("running 0, paused 10"), for the hidden-window log. */
function animationStates(): string {
  const counts = new Map<string, number>();
  for (const animation of document.getAnimations()) counts.set(animation.playState, (counts.get(animation.playState) ?? 0) + 1);
  return [...counts].map(([state, count]) => `${state} ${count}`).join(", ") || "none";
}

/** Running animations by name ("halo x12, zl-spin x1"), for the idle log. */
function runningAnimations(): string {
  const counts = new Map<string, number>();
  for (const animation of document.getAnimations()) {
    if (animation.playState !== "running") continue;
    const name = (animation as CSSAnimation).animationName || animation.constructor.name;
    counts.set(name, (counts.get(name) ?? 0) + 1);
  }
  return [...counts].map(([name, count]) => `${name} x${count}`).join(", ") || "none";
}

/** The process tree over a 10 s window: CPU is the cpu_seconds delta over the wall time between two samples. */
async function idleSample(what: string): Promise<unknown> {
  let sample: unknown;
  try { sample = (await metrics(10_000)) ?? null; } catch (error) { sample = { error: String(error) }; }
  const s = sample as { total?: { cpu_percent?: number; rss_bytes?: number }; processes?: { role?: string; cpu_percent?: number | null }[]; window_ms?: number } | null;
  const roles = (s?.processes ?? []).map((p) => `${p.role} ${p.cpu_percent ?? "n/a"}%`).join(", ");
  log(`${what}: ${s?.total?.cpu_percent ?? "n/a"}% CPU over ${s?.window_ms ?? "n/a"} ms (${roles}), RSS ${Math.round((s?.total?.rss_bytes ?? 0) / 1048576)} MiB`);
  return sample;
}

/** An Activity that scrolls at least this far serves the Activity scroll scenario. */
const SCROLLABLE_PX = 2000;

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
  /** The page while the window was last away (perf_away): its animations and what the pump did until it returned. */
  type Pump = typeof pumpStats;
  let away: { visibility: string; animations?: string; hidden_ms?: number; pump?: Pump } = { visibility: document.visibilityState };
  let atHide: { at: number; pump: Pump } | undefined;
  const onVisibility = () => {
    if (document.visibilityState === "hidden") {
      atHide = { at: performance.now(), pump: { ...pumpStats } };
      away = { visibility: "hidden", animations: animationStates() };
    } else if (atHide) {
      const before = atHide.pump;
      away.hidden_ms = Math.round(performance.now() - atHide.at);
      away.pump = Object.fromEntries((Object.keys(pumpStats) as (keyof Pump)[]).map((k) => [k, pumpStats[k] - before[k]])) as Pump;
      atHide = undefined;
    }
    log(`visibility ${document.visibilityState} after ${elapsed()}${away.animations && document.visibilityState === "hidden" ? `; animations ${away.animations}` : ""}`);
  };
  document.addEventListener("visibilitychange", onVisibility);
  const cutoff = setTimeout(() => { log(`run timeout after ${elapsed()}, saving ${finished.length} scenarios`); void failed("run timeout (8 min)"); }, RUN_TIMEOUT_MS);
  try {
    log(`start (display ${config.display ?? "n/a"}, preference ${config.preference ?? "n/a"}, visibility ${document.visibilityState}, cross-origin isolated ${crossOriginIsolated})`);
    await until(() => orderedRows().length > 0, 180_000, "the catalog stayed empty for 3 minutes");
    log(`rows ready: ${orderedRows().length} after ${elapsed()}`);
    // Many sessions keep no journal events; move on until one has some (the
    // Activity scenarios need them). A chosen `?select=` session is tried first.
    const cards = listRows().filter((r) => r.type === "card").map((r) => r.id);
    const candidates = [...new Set([selectedKey(), ...cards].filter((id): id is string => !!id))].slice(0, 16);
    // The Activity scenario needs a transcript that scrolls: read older pages
    // of a short one, and keep the session that scrolls furthest.
    const range = () => { const el = scrollActivity(); return el ? el.scrollHeight - el.clientHeight : 0; };
    let tried = 0;
    let best: { id: string; range: number; pages: number } | undefined;
    /** Reads up to `pages` older pages, fewer once the Activity scrolls far enough. */
    const readOlder = async (pages: number, enough: number) => {
      let read = 0;
      while (read < pages && activity.events().length > 0 && range() < enough && !activity.olderDone()) {
        activity.loadOlder();
        await until(() => !activity.loadingOlder(), 8000).catch(() => {});
        await sleep(150);
        read++;
      }
      await sleep(300);
      return read;
    };
    for (const id of candidates) {
      tried++;
      setSelectedKey(id);
      const kind = `${id.startsWith("\n") ? "this computer" : "a peer"}, ${id.includes("\nrun:") ? "run key" : "name key"}`;
      await until(() => activity.target()?.id === id && (activity.loaded() || activity.events().length > 0), 15_000).catch(() => {});
      // Events can follow the first (empty) answer by a moment.
      await until(() => activity.events().length > 0, 1500).catch(() => {});
      const pages = await readOlder(4, SCROLLABLE_PX);
      const error = activity.error();
      log(`selected ${tried}/${candidates.length}: ${kind}; events ${activity.events().length}; scrolls ${range()} px${error ? `; follow failed: ${error}` : ""}`);
      if (activity.events().length > 0 && (!best || range() > best.range)) best = { id, range: range(), pages };
      if (best && best.range >= SCROLLABLE_PX) break;
    }
    if (best && selectedKey() !== best.id) {
      const chosen = best;
      setSelectedKey(chosen.id);
      await until(() => activity.target()?.id === chosen.id && activity.events().length > 0, 15_000).catch(() => {});
      await readOlder(chosen.pages, chosen.range);
    }
    log(`activity events: ${activity.events().length}, scrolls ${range()} px after ${elapsed()} (${tried} sessions tried)`);
    // Without animation frames every pacing scenario would time out: say so at once.
    const frames = await countFrames(3000);
    log(`frames: ${frames} in 3 s (visibility ${document.visibilityState}, focus ${document.hasFocus()}, window ${innerWidth}x${innerHeight} at ${devicePixelRatio}x, display ${await displayHz() ?? "unknown"} Hz, cross-origin isolated ${crossOriginIsolated})`);
    await until(() => !!globalThis.zerusPerf, 30_000, "the Diagnostics panel did not start");
    const report = await globalThis.zerusPerf!.run();
    log(`scenarios done after ${elapsed()}; settling 10 s, then seven 10 s idle windows`);
    await sleep(10_000);
    // Idle as displayed (working halos, header pulses, the mark while a session
    // needs attention keep producing frames), then with each motion setting, so
    // a report separates what the motion costs from what the app does. The
    // settings are tried without being stored.
    log(`running animations: ${runningAnimations()}`);
    const idle = await idleSample("idle");
    setSetting("animateActivity", false, { persist: false });
    await sleep(1000);
    log(`running animations with Animate working sessions off: ${runningAnimations()}`);
    const still = await idleSample("idle with Animate working sessions off");
    restoreSettings();
    setSetting("reduceMotion", true, { persist: false });
    await sleep(1000);
    log(`running animations with Reduce motion: ${runningAnimations()}`);
    const calm = await idleSample("idle with Reduce motion");
    restoreSettings();
    await sleep(1000);
    // Hidden, then minimised: the native side takes the window away, samples
    // the process tree and brings it back. The pump and the motion must stop.
    const gone: Record<string, unknown> = {};
    for (const how of ["hide", "minimize"] as const) {
      away = { visibility: document.visibilityState };
      const result = await invoke<{ sample?: unknown; minimized?: boolean }>("perf_away", { how, window_ms: 10_000 }).catch((error) => ({ error: String(error) }));
      // The page reports its return a moment after the window is back.
      await until(() => document.visibilityState === "visible" && (away.visibility !== "hidden" || !!away.pump), 3000).catch(() => {});
      const s = (result as { sample?: { total?: { cpu_percent?: number }; processes?: { role?: string; cpu_percent?: number | null }[]; window_ms?: number } }).sample;
      const roles = (s?.processes ?? []).map((p) => `${p.role} ${p.cpu_percent ?? "n/a"}%`).join(", ");
      const pump = away.pump ? Object.entries(away.pump).map(([k, v]) => `${k} +${v}`).join(", ") : "n/a";
      const page = away.visibility === "hidden" ? `hidden for ${away.hidden_ms ?? "n/a"} ms, animations ${away.animations}, pump while hidden ${pump}` : `stayed ${away.visibility}`;
      log(`window ${how}: ${s?.total?.cpu_percent ?? "n/a"}% CPU over ${s?.window_ms ?? "n/a"} ms (${roles}); page ${page}${"minimized" in result ? `; minimized ${result.minimized}` : ""}${"error" in result ? `; ${(result as { error: string }).error}` : ""}`);
      await sleep(1000);
      // As displayed again: the motion resumed and the pump caught up.
      log(`running animations after the window returned: ${runningAnimations()}`);
      gone[how] = { ...result, page: away, returned: await idleSample(`idle after ${how}`) };
    }
    report.resources = { ...(report.resources ?? {}), idle, idle_still_working: still, idle_reduced_motion: calm, idle_away: gone };
    clearTimeout(cutoff);
    await finish(toJson({ ...report, preference: config.preference ?? null }), 0);
  } catch (error) {
    clearTimeout(cutoff);
    log(`failed: ${String(error)}`);
    await failed(error);
  }
}

// `?actions-smoke=1` (ZERUS_CLIENT_QUERY=actions-smoke=1, with ZERUS_CLIENT_PERF
// set to any value but auto so that exit_app is allowed) is the end-to-end check
// of the guarded actions: with ZERUS_CLIENT_REPLAY supplying the catalog and
// ZERUS_CLIENT_HGS a fake hgs, it selects the replay session (its details come
// from the Activity state value, or one inspect), runs pause, resume, rename,
// archive, the end call the Terminate dialog picks for it through the
// dialog's confirmation (`end`), then the other end call (terminate or kill)
// once each, and exits 0 (1 when an outcome failed).
const SMOKE_SESSION = "claude/sample-project/review";
const SMOKE_RENAMED = "claude/sample-project/review-renamed";

async function actionsSmoke(allowed: boolean) {
  const exit = (code: number) => invoke("exit_app", { code }).catch(() => {});
  if (!allowed) {
    log("actions-smoke not started: it needs ZERUS_CLIENT_REPLAY, ZERUS_CLIENT_HGS and ZERUS_CLIENT_PERF");
    return;
  }
  try {
    await until(() => orderedRows().some((s) => s.name === SMOKE_SESSION && s.runId), 30_000, "the replay session did not appear");
    const row = (): ActionRow => {
      const s = orderedRows().find((r) => r.name === SMOKE_SESSION);
      if (!s) throw new Error("the replay session is gone");
      return snapshotOf(s);
    };
    setSelectedKey(row().key);
    const chosen = terminateCopy(row()).action;
    log(`actions-smoke end call the Terminate dialog picks: ${chosen}`);
    const steps: [string, () => Promise<Outcome>][] = [
      ...([{ kind: "pause" }, { kind: "resume" }, { kind: "rename", new_name: SMOKE_RENAMED, archive_id: null }, { kind: "archive" }] as Action[])
        .map((action): [string, () => Promise<Outcome>] => [action.kind, () => perform(row(), action)]),
      [chosen, () => end(row(), chosen)],
      ...(["terminate", "kill"] as const).filter((kind) => kind !== chosen)
        .map((kind): [string, () => Promise<Outcome>] => [kind, () => perform(row(), { kind })]),
    ];
    let bad = 0;
    for (const [kind, step] of steps) {
      const out = await step();
      log(`actions-smoke ${kind}: ${out.outcome}${out.outcome === "failed" ? ` (${out.detail})` : ""}`);
      if (out.outcome === "failed") bad++;
    }
    await exit(bad ? 1 : 0);
  } catch (error) {
    log(`actions-smoke failed: ${String(error)}`);
    await exit(1);
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
    if (new URLSearchParams(location.search).get("actions-smoke") === "1" && inTauri()) void actionsSmoke(!!config().actions_smoke);
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
          <Diagnostics scrollList={scrollList} scrollActivity={scrollActivity} openSession={openSession} displayHz={displayHz}
            metrics={auto() ? undefined : metrics} label={config().label ?? undefined} representative={config().representative}
            onScenarioStart={auto() ? onScenarioStart : undefined} onScenarioEnd={auto() ? onScenarioEnd : undefined} />
        </Suspense>
      </aside>
    </Show>
  );
}
