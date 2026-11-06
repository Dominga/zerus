import { createRoot } from "solid-js";
import { describe, expect, it } from "vitest";
import type { TeamPhaseView, TeamTimelineView, TimelineSpan } from "../../types";
import { KIND_STYLES, kindKey } from "./profile-palette";
import {
  BUSY_KINDS, MAX_ZOOM, TIMELINE_STALE_MS, TL, atOf, clamp, createTimelineReads, defaultPhase, estimateRows, fit, heightFor,
  kindName, panBy, scopeChoices, spanAt, spanTip, testTone, ticks, visibleSeconds, xOf, zoomBy, type Viewport,
} from "./timeline";

const T0 = 1_793_000_000;
const span = (task: number, kind: string, from: number, to: number, extra: Partial<TimelineSpan> = {}): TimelineSpan =>
  ({ task, attempt: 1, kind, stage_kind: "", member: "", from: T0 + from, to: T0 + to, running: false, ...extra });

function view(): TeamTimelineView {
  return {
    team: "t", phase: 2, from: T0, to: T0 + 3600, elapsed_s: 3600, worked_s: 2400,
    lanes: [
      { member: "core", role: "Tauri core (Rust)", model: "", utilization: 0.5, busy_s: 1800, idle_s: 600,
        spans: [span(1, "work", 0, 1200, { member: "core" }), span(1, "build", 1200, 1300, { member: "core" })],
        waits: [span(1, "deps", 1300, 1800)], idle: [[T0 + 1800, T0 + 2400]] },
      { member: "ui1", role: "Frontend (SolidJS)", model: "", utilization: 0.8, busy_s: 2400, idle_s: 0,
        spans: [span(2, "stage", 600, 1500, { member: "ui1", stage_kind: "review" })], waits: [], idle: [] },
    ],
    real: [span(1, "work", 0, 1300)], planned: [span(1, "work", 0, 1200), span(2, "work", 1200, 2100)], real_s: 1300, planned_s: 2100,
    tests: [span(1, "tests", 900, 960, { member: "client-core", stage_kind: "passed", record: "r1" })], tests_more: false,
    estimates: [
      { task: 1, estimate_min: 30, worked_s: 1200, running: false },
      { task: 2, estimate_min: 10, worked_s: 1800, running: true },
      { task: 3, estimate_min: null, worked_s: 600, running: false },
    ],
    titles: { "1": "Core read", "2": "Time view" }, tasks: 3, truncated: false,
  };
}
const viewport = (v: TeamTimelineView, width = 1210): Viewport => ({ from: v.from, to: v.to, zoom: 1, start: v.from, width });
/** The middle of a row's band: 0 real, 1 planned, then the lanes, then the tests. */
const rowY = (row: number, inRow: number) => TL.Top + TL.Row * row + inRow;

describe("the profile's Time view (task 92)", () => {
  it("maps time to the plot and back, and fits the whole scope at zoom 1", () => {
    const v = viewport(view());
    expect(xOf(v, v.from)).toBe(TL.Label);
    expect(xOf(v, v.to)).toBeCloseTo(v.width - TL.Right);
    expect(atOf(v, xOf(v, v.from + 900))).toBeCloseTo(v.from + 900);
    expect(visibleSeconds(v)).toBe(3600);
    // A moment far outside clamps a little past the edge, as Qt draws it.
    expect(xOf(v, v.to + 99_999)).toBeLessThanOrEqual(TL.Label + (v.width - TL.Label - TL.Right) * 1.02 + 1e-9);
  });

  it("zooms around the pointer, pans within the scope and fits again", () => {
    const v = viewport(view());
    const at = v.from + 900;
    const zoomed = zoomBy(v, 4, at);
    expect(zoomed.zoom).toBe(4);
    expect(atOf(zoomed, xOf(v, at))).toBeCloseTo(at, 0);
    expect(zoomBy(zoomed, 1e9).zoom).toBe(MAX_ZOOM);
    expect(zoomBy(v, 0.1).zoom).toBe(1);
    expect(panBy(zoomed, -99_999).start).toBe(v.from);
    expect(panBy(zoomed, 99_999).start).toBeCloseTo(v.to - visibleSeconds(zoomed));
    expect(clamp({ ...v, start: v.from + 500 }).start).toBe(v.from);
    expect(fit(zoomed)).toEqual({ ...zoomed, zoom: 1, start: v.from });
  });

  it("puts about ten ticks at whole minutes on the axis", () => {
    const hour = ticks(viewport(view()));
    expect(hour.length).toBeGreaterThanOrEqual(5);
    expect(hour.length).toBeLessThanOrEqual(11);
    expect(hour.every((tick) => tick.at % 60 === 0 && /^\d\d:\d\d$/.test(tick.label))).toBe(true);
    const day = ticks({ ...viewport(view()), to: T0 + 30 * 86400 });
    expect(day.length).toBeLessThanOrEqual(32);
  });

  it("finds the span under the pointer in Qt's rows and bands", () => {
    const tv = view(), v = viewport(tv);
    const x = (at: number) => xOf(v, T0 + at);
    expect(spanAt(tv, v, x(600), rowY(0, 25))?.row).toBe("real");
    expect(spanAt(tv, v, x(1500), rowY(1, 25))?.span.task).toBe(2);
    // core's lane: the work band, the waits strip under it and the build strip at the bottom.
    expect(spanAt(tv, v, x(600), rowY(2, TL.WorkTop + 5))?.span.kind).toBe("work");
    expect(spanAt(tv, v, x(1500), rowY(2, TL.WaitTop + 2))?.span.kind).toBe("deps");
    expect(spanAt(tv, v, x(1250), rowY(2, TL.BuildTop + 2))?.span.kind).toBe("build");
    expect(spanAt(tv, v, x(1250), rowY(2, TL.WorkTop + 5))).toBeNull();
    expect(spanAt(tv, v, x(1000), rowY(3, TL.WorkTop + 5))).toMatchObject({ lane: "ui1", span: { kind: "stage" } });
    // The tests row follows the lanes; the label column and the gap below draw nothing.
    expect(spanAt(tv, v, x(930), rowY(4, 20))?.span.record).toBe("r1");
    expect(spanAt(tv, v, 20, rowY(2, 10))).toBeNull();
    expect(spanAt(tv, v, x(600), rowY(9, 10))).toBeNull();
    expect(heightFor(tv)).toBe(TL.Top + TL.Row * 5 + 8);
    expect(heightFor({ ...tv, tests: [] })).toBe(TL.Top + TL.Row * 4 + 8);
  });

  it("names every kind Qt draws and gives each its palette entry", () => {
    const kinds = [...BUSY_KINDS, "build", "delivery", "start_delay", "member", "deps", "lead", "folder_held", "hold", "stage_wait", "no_report", "stop"];
    for (const kind of kinds) {
      expect(kindName(kind, "review")).not.toBe("Not recorded");
      expect(KIND_STYLES.some((style) => style.key === kindKey(kind, "review"))).toBe(true);
    }
    expect(kindName("stage", "review")).toBe("Review stage");
    expect(kindName("stage", "lint")).toBe("Stage: lint");
    expect(kindName("invented")).toBe("Not recorded");
  });

  it("words a span's hover lines, a test run's too", () => {
    const tv = view();
    const lines = spanTip(tv, tv.lanes[0].spans[0]);
    expect(lines[0]).toBe("#1 Core read");
    // Qt's HH:mm, with the day in front when it is not today.
    expect(lines[1]).toMatch(/^Work: 20m, (.+ )?\d\d:\d\d to (.+ )?\d\d:\d\d$/);
    expect(spanTip(tv, { ...tv.lanes[0].spans[0], running: true })[1]).toMatch(/ to now$/);
    expect(spanTip(tv, tv.tests[0], (status) => status.toUpperCase())).toEqual([
      "Tests: client-core", expect.stringMatching(/^PASSED, 1m, /), "Asked for #1 Core read",
    ]);
    expect(testTone("passed")).toBe("accent");
    expect(testTone("timeout")).toBe("warn");
    expect(testTone("failed")).toBe("danger");
  });

  it("opens on the phase in work and offers the plan and every phase without separator dots", () => {
    const phase = (n: number, state: string, title = ""): TeamPhaseView =>
      ({ n, title, state, status: state, done: 0, total: 1, barrier: false, auto: false, acceptance: "", paused: false, health: "" });
    expect(defaultPhase([phase(1, "done"), phase(2, "running"), phase(3, "draft")])).toBe(2);
    expect(defaultPhase([phase(1, "done"), phase(2, "draft")])).toBe(2);
    expect(defaultPhase([])).toBeNull();
    const choices = scopeChoices([phase(1, "done", "Teams first"), phase(2, "running")]);
    expect(choices.map((choice) => choice.value)).toEqual(["", "1", "2"]);
    expect(choices.map((choice) => choice.label)).toEqual(["The plan, every phase", "Phase 1: Teams first", "Phase 2"]);
    expect(choices.some((choice) => /[·•]/.test(choice.label))).toBe(false);
  });

  it("lists the estimates with the largest overrun first", () => {
    const rows = estimateRows(view());
    expect(rows.rows.map((row) => row.task)).toEqual([2, 1, 3]);
    expect(rows).toMatchObject({ known: 2, past: 1 });
  });

  it("reads a scope once, again only when the team changed and the read went stale, and drops late answers", async () => {
    let clock = 0;
    const asked: (number | null)[] = [];
    const answers: ((value: TeamTimelineView) => void)[] = [];
    await createRoot(async (dispose) => {
      const reads = createTimelineReads((phase) => { asked.push(phase); return new Promise((resolve) => answers.push(resolve)); }, () => clock);
      reads.show(2, 10);
      reads.show(2, 10);
      expect(asked).toEqual([2]);
      expect(reads.loading()).toBe(true);
      // Another scope before the first answer: the first answer is dropped.
      reads.show(null, 10);
      answers[0]({ ...view(), phase: 2 });
      answers[1]({ ...view(), phase: null });
      await Promise.resolve(); await Promise.resolve();
      expect(reads.view()?.phase).toBeNull();
      expect(reads.loading()).toBe(false);
      // The team changed, but the read is fresh: no read.
      clock = TIMELINE_STALE_MS;
      reads.changed(11);
      expect(asked).toEqual([2, null]);
      // Older than the stale bound: read again, keeping the shown lanes meanwhile.
      clock = TIMELINE_STALE_MS + 1;
      reads.changed(11);
      expect(asked).toEqual([2, null, null]);
      expect(reads.view()).not.toBeNull();
      // The same revision again never reads.
      answers[2](view()); await Promise.resolve(); await Promise.resolve();
      clock *= 3;
      reads.changed(11);
      expect(reads.reads()).toBe(3);
      dispose();
    });
  });
});
