// In-app diagnostics panel: runs the measurement scenarios and saves the results.
//
// The panel depends on the app only through its props. It also registers
// `globalThis.zerusPerf.run(options)` so a Tauri command or a headless driver can
// start the same run and read the report object without clicking.

import { createSignal, For, onCleanup, onMount, Show, type JSX } from "solid-js";
import { frameBudget } from "./pacing";
import {
  DEFAULT_PLAN,
  SCENARIOS,
  download,
  estimateHz,
  runAll,
  toJson,
  type DiagnosticsTargets,
  type PacingRun,
  type PerfReport,
  type Plan,
  type RunAllOptions,
  type ScenarioId,
  type ScenarioResult,
  type TimingRun,
} from "./scenarios";

export interface DiagnosticsProps extends DiagnosticsTargets {
  /** The display's refresh rate from the native side (macOS: NSScreen.maximumFramesPerSecond); null when unknown. */
  displayHz?: () => Promise<number | null>;
  /** Resource sample of the process tree (the Tauri `metrics` command). */
  metrics?: () => Promise<unknown>;
  /** Saves the report (for example in the app data directory) and resolves to where it went. */
  save?: (json: string, name: string) => Promise<string>;
  /** Platform label in the report, for example "mac-promotion" or "arch-kde-125". */
  label?: string;
  /** False for headless or otherwise non-representative runs (Xvfb). */
  representative?: boolean;
  /** Scenario start and end, for an unattended run's log. */
  onScenarioStart?: (id: ScenarioId, index: number, total: number) => void;
  onScenarioEnd?: RunAllOptions["onScenarioEnd"];
}

declare global {
  // eslint-disable-next-line no-var
  var zerusPerf: { run: (options?: Partial<Plan> & { ids?: ScenarioId[] }) => Promise<PerfReport> } | undefined;
}

const cell: JSX.CSSProperties = { padding: "3px 8px", "text-align": "right", "white-space": "nowrap" };
const head: JSX.CSSProperties = { ...cell, color: "var(--z-color-dim, #8a8f98)", "font-weight": "500" };

function ms(value: number | null | undefined) {
  return value == null || !Number.isFinite(value) ? "n/a" : value.toFixed(2);
}

function share(value: number | null | undefined) {
  return value == null || !Number.isFinite(value) ? "n/a" : `${(value * 100).toFixed(2)}%`;
}

function mark(ok: boolean | null | undefined) {
  if (ok == null) return "n/a";
  return ok ? "pass" : "fail";
}

export default function Diagnostics(props: DiagnosticsProps) {
  const [hz, setHz] = createSignal(60);
  const [hzSource, setHzSource] = createSignal<Plan["hzSource"]>("estimated");
  const [warmupMs, setWarmupMs] = createSignal(DEFAULT_PLAN.warmupMs);
  const [runMs, setRunMs] = createSignal(DEFAULT_PLAN.runMs);
  const [runs, setRuns] = createSignal(DEFAULT_PLAN.runs);
  const [synthetic, setSynthetic] = createSignal(DEFAULT_PLAN.syntheticInput);
  const [running, setRunning] = createSignal(false);
  const [status, setStatus] = createSignal("");
  const [report, setReport] = createSignal<PerfReport | null>(null);
  let stageRef: HTMLDivElement | undefined;

  const plan = (overrides: Partial<Plan> = {}): Plan => ({
    ...DEFAULT_PLAN,
    hz: hz(),
    hzSource: hzSource(),
    warmupMs: warmupMs(),
    runMs: runMs(),
    runs: runs(),
    syntheticInput: synthetic(),
    ...overrides,
  });

  const start = async (ids?: ScenarioId[], overrides: Partial<Plan> = {}): Promise<PerfReport> => {
    if (running()) throw new Error("a measurement is already running");
    if (!stageRef) throw new Error("the diagnostics panel is not mounted");
    setRunning(true);
    setReport(null);
    const options: RunAllOptions = {
      ids,
      label: props.label,
      representative: props.representative,
      metrics: props.metrics,
      onProgress: (id, index, total) => {
        setStatus(`Running ${SCENARIOS.find((s) => s.id === id)?.label ?? id} (${index + 1} of ${total})`);
        props.onScenarioStart?.(id, index, total);
      },
      onScenarioEnd: (...args) => props.onScenarioEnd?.(...args),
    };
    try {
      const result = await runAll(props, { root: stageRef }, plan(overrides), options);
      setReport(result);
      setStatus(`Finished at ${new Date(result.created_at).toLocaleTimeString()}`);
      return result;
    } catch (error) {
      setStatus(`Failed: ${String(error)}`);
      throw error;
    } finally {
      setRunning(false);
    }
  };

  onMount(async () => {
    globalThis.zerusPerf = { run: ({ ids, ...overrides } = {}) => start(ids, overrides) };
    const native = await props.displayHz?.().catch(() => null);
    if (native && native > 0) {
      setHz(native);
      setHzSource("native");
    } else {
      setHz(await estimateHz());
      setHzSource("estimated");
    }
  });
  onCleanup(() => {
    if (globalThis.zerusPerf) globalThis.zerusPerf = undefined;
  });

  const save = async () => {
    const current = report();
    if (!current) return;
    const name = `zerus-perf-${current.platform.label}-${current.created_at.replace(/[:]/g, "-")}.json`;
    if (props.save) setStatus(`Saved to ${await props.save(toJson(current), name)}`);
    else download(current, name);
  };

  const numberInput = (value: () => number, set: (v: number) => void, label: string, min = 1) => (
    <label style={{ display: "inline-flex", gap: "6px", "align-items": "center" }}>
      {label}
      <input
        type="number"
        min={min}
        value={value()}
        disabled={running()}
        style={{ width: "80px" }}
        onInput={(event) => {
          const next = Number(event.currentTarget.value);
          if (Number.isFinite(next) && next >= min) set(next);
        }}
      />
    </label>
  );

  return (
    <section
      aria-label="Performance diagnostics"
      style={{
        display: "flex",
        "flex-direction": "column",
        gap: "10px",
        padding: "12px",
        color: "var(--z-color-text, #e6e8eb)",
        background: "var(--z-color-surface, #17191c)",
        "font-size": "var(--fs-sm, 12px)",
        overflow: "auto",
      }}
    >
      <header style={{ display: "flex", gap: "12px", "align-items": "baseline", "flex-wrap": "wrap" }}>
        <strong style={{ "font-size": "var(--fs-md, 14px)" }}>Frame pacing and responsiveness</strong>
        <span style={{ color: "var(--z-color-dim, #8a8f98)" }}>
          T {frameBudget(hz()).toFixed(2)} ms at {hz()} Hz ({hzSource()})
        </span>
      </header>

      <div style={{ display: "flex", gap: "14px", "flex-wrap": "wrap", "align-items": "center" }}>
        {numberInput(hz, (v) => (setHz(v), setHzSource("manual")), "Display Hz", 24)}
        {numberInput(warmupMs, setWarmupMs, "Warm-up ms", 0)}
        {numberInput(runMs, setRunMs, "Run ms", 100)}
        {numberInput(runs, setRuns, "Runs")}
        <label style={{ display: "inline-flex", gap: "6px", "align-items": "center" }}>
          <input type="checkbox" checked={synthetic()} disabled={running()} onChange={(e) => setSynthetic(e.currentTarget.checked)} />
          Synthetic input (off: click the probe yourself)
        </label>
      </div>

      <div style={{ display: "flex", gap: "8px", "flex-wrap": "wrap" }}>
        <button type="button" disabled={running()} onClick={() => void start()}>
          Run all
        </button>
        <For each={SCENARIOS}>
          {(scenario) => (
            <button type="button" disabled={running()} onClick={() => void start([scenario.id])}>
              {scenario.label}
            </button>
          )}
        </For>
        <button type="button" disabled={running() || !report()} onClick={() => void save()}>
          {props.save ? "Save results" : "Download JSON"}
        </button>
      </div>

      <div role="status" style={{ color: "var(--z-color-dim, #8a8f98)", "min-height": "1.4em" }}>
        {status()}
      </div>

      {/* Animations and the input probe run here; a fixed size keeps the page light. */}
      <div
        ref={stageRef}
        aria-hidden="true"
        style={{
          position: "relative",
          width: "240px",
          height: "32px",
          border: "1px solid var(--z-color-border, #2b2f35)",
          "border-radius": "var(--z-radius-sm, 4px)",
          overflow: "hidden",
        }}
      />

      <Show when={report()}>
        {(current) => (
          <table style={{ "border-collapse": "collapse" }}>
            <thead>
              <tr>
                <th style={{ ...head, "text-align": "left" }}>Scenario (worst of {current().settings.runs})</th>
                <th style={head}>Mean</th>
                <th style={head}>Median</th>
                <th style={head}>p95</th>
                <th style={head}>p99</th>
                <th style={head}>Over 1.5 T</th>
                <th style={head}>Over 2.5 T</th>
                <th style={head}>15.0-18.5 ms</th>
                <th style={head}>Criterion 1</th>
              </tr>
            </thead>
            <tbody>
              <For each={current().scenarios}>{(scenario) => <ResultRow scenario={scenario} />}</For>
            </tbody>
          </table>
        )}
      </Show>
    </section>
  );
}

function ResultRow(props: { scenario: ScenarioResult }) {
  const s = () => props.scenario;
  return (
    <tr>
      <td style={{ ...cell, "text-align": "left" }}>{s().label}</td>
      <Show
        when={!s().skipped}
        fallback={
          <td style={{ ...cell, "text-align": "left" }} colSpan={8}>
            Skipped: {s().skipped}
          </td>
        }
      >
        <Show
          when={s().kind === "pacing"}
          fallback={(() => {
            const run = s().worst as TimingRun | null;
            const summary = run?.wall_ms ?? run?.latency_ms;
            return (
              <>
                <td style={cell}>{ms(summary?.mean)}</td>
                <td style={cell}>{ms(summary?.median)}</td>
                <td style={cell}>{ms(summary?.p95)}</td>
                <td style={cell}>{ms(summary?.p99)}</td>
                <td style={{ ...cell, "text-align": "left" }} colSpan={4}>
                  {run?.count ?? 0} samples, max {ms(summary?.max)} ms
                  {run?.to_paint_ms ? `, after paint p95 ${ms(run.to_paint_ms.p95)} ms` : ""}
                </td>
              </>
            );
          })()}
        >
          {(() => {
            const run = s().worst as PacingRun | null;
            const i = run?.interval_ms;
            return (
              <>
                <td style={cell}>{ms(i?.mean)}</td>
                <td style={cell}>{ms(i?.median)}</td>
                <td style={cell}>{ms(i?.p95)}</td>
                <td style={cell}>{ms(i?.p99)}</td>
                <td style={cell}>{share(i?.over1_5T)}</td>
                <td style={cell}>{share(i?.over2_5T)}</td>
                <td style={cell}>{share(i?.band60)}</td>
                <td style={cell}>{mark(s().verdict?.all)}</td>
              </>
            );
          })()}
        </Show>
      </Show>
    </tr>
  );
}
