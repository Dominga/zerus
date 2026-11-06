// Task 92: the profile's Time view, as Qt's TeamProfileTimeline draws it
// (tray/src/TeamProfile.cpp): the axis, the real and planned critical paths,
// a lane per member (work bar with its title, the waits strip under it, build
// and merge as a thin strip along the bottom, idle as a dashed outline) and
// the test runs. The core projects the lanes (teams/timeline.rs); this module
// holds the geometry, zoom and pan, hit testing and the words, and draws
// nothing itself, so it is tested without a canvas.
import { createSignal } from "solid-js";
import { tr } from "../../i18n";
import type { TeamPhaseView, TeamTimelineView, TimelineSpan } from "../../types";
import { durationText, momentText } from "./heat";

/** Qt's geometry: the label column, the row, and the bands of a lane from its top. */
export const TL = { Top: 34, Row: 52, Label: 210, Right: 12, WorkTop: 8, WorkHeight: 22, WaitTop: 33, WaitHeight: 7, BuildTop: 43, BuildHeight: 5 } as const;
export const MAX_ZOOM = 512;
/** Qt's axis steps, in seconds: about ten ticks at whole minutes. */
const STEPS = [60, 120, 300, 600, 900, 1800, 3600, 7200, 10800, 21600, 43200, 86400];
/** The member's own time (Qt's Busy); build runs beside it, everything else is a wait. */
export const BUSY_KINDS = ["work", "rework", "stage", "question", "permission", "background"] as const;

/** What the canvas shows: the scope's window, the zoom (1 is the whole scope), the first moment shown and the width in pixels. */
export interface Viewport { from: number; to: number; zoom: number; start: number; width: number }

export const visibleSeconds = (v: Viewport) => Math.max(1, v.to - v.from) / v.zoom;
const plot = (v: Viewport) => Math.max(1, v.width - TL.Label - TL.Right);

/** Keeps the shown time inside the scope (Qt's clampView). */
export function clamp(v: Viewport): Viewport {
  const zoom = Math.min(MAX_ZOOM, Math.max(1, v.zoom));
  const seen = Math.max(1, v.to - v.from) / zoom;
  const start = Math.min(Math.max(v.start, v.from), Math.max(v.from, v.to - seen));
  return { ...v, zoom, start };
}
export const fit = (v: Viewport): Viewport => ({ ...v, zoom: 1, start: v.from });
/** Zooms by `factor` around a moment (the pointer's), or the middle of what shows. */
export function zoomBy(v: Viewport, factor: number, around?: number): Viewport {
  const at = around ?? v.start + visibleSeconds(v) / 2;
  const share = (at - v.start) / Math.max(1, visibleSeconds(v));
  const zoom = Math.min(MAX_ZOOM, Math.max(1, v.zoom * factor));
  const next = { ...v, zoom };
  return clamp({ ...next, start: at - share * visibleSeconds(next) });
}
/** Moves what shows by `pixels` of the plot (a drag) or a tenth of it per wheel step. */
export const panBy = (v: Viewport, seconds: number): Viewport => clamp({ ...v, start: v.start + seconds });
export const pixelsToSeconds = (v: Viewport, pixels: number) => (pixels / plot(v)) * visibleSeconds(v);

/** The x of a moment; a little past either edge at most, as Qt clamps it. */
export const xOf = (v: Viewport, at: number) => TL.Label + plot(v) * Math.min(1.02, Math.max(-0.02, (at - v.start) / visibleSeconds(v)));
export const atOf = (v: Viewport, x: number) => v.start + ((x - TL.Label) / plot(v)) * visibleSeconds(v);
/** A span outside the shown time draws nothing. */
export const shown = (v: Viewport, from: number, to: number) => to >= v.start && from <= v.start + visibleSeconds(v);

/** The axis ticks: about ten, at whole minutes, labelled HH:mm. */
export function ticks(v: Viewport): { at: number; label: string }[] {
  const span = Math.max(60, visibleSeconds(v));
  const step = STEPS.find((candidate) => span / candidate <= 11) ?? STEPS[STEPS.length - 1];
  const out: { at: number; label: string }[] = [];
  for (let at = Math.ceil(v.start / step) * step; at <= v.start + visibleSeconds(v) + 1 && out.length < 32; at += step) {
    const when = new Date(at * 1000);
    out.push({ at, label: `${String(when.getHours()).padStart(2, "0")}:${String(when.getMinutes()).padStart(2, "0")}` });
  }
  return out;
}

/** The canvas height: the two paths, a lane per member and the tests row. */
export const heightFor = (view: TeamTimelineView | null | undefined) =>
  view ? TL.Top + TL.Row * (2 + view.lanes.length + (view.tests.length ? 1 : 0)) + 8 : 120;

export interface Hit { span: TimelineSpan; lane?: string; row: "real" | "planned" | "lane" | "tests" }

/** The span under a point (Qt's spanAt): a lane's work band, its waits strip or its build strip. */
export function spanAt(view: TeamTimelineView, v: Viewport, x: number, y: number): Hit | null {
  if (y < TL.Top || x < TL.Label - 2) return null;
  const row = Math.floor((y - TL.Top) / TL.Row);
  const over = (span: TimelineSpan) => shown(v, span.from, span.to) && x >= xOf(v, span.from) - 1 && x <= xOf(v, span.to) + 1;
  const first = (spans: TimelineSpan[]) => spans.find(over);
  const hit = (span: TimelineSpan | undefined, kind: Hit["row"], lane?: string): Hit | null => (span ? { span, row: kind, lane } : null);
  if (row === 0) return hit(first(view.real), "real");
  if (row === 1) return hit(first(view.planned), "planned");
  const at = row - 2;
  if (at === view.lanes.length && view.tests.length) return hit(first(view.tests), "tests");
  const lane = view.lanes[at];
  if (!lane) return null;
  const inRow = (y - TL.Top) % TL.Row;
  if (inRow >= TL.BuildTop - 2) return hit(lane.spans.find((span) => span.kind === "build" && over(span)), "lane", lane.member);
  if (inRow >= TL.WaitTop - 1) return hit(first(lane.waits), "lane", lane.member);
  return hit(lane.spans.find((span) => span.kind !== "build" && over(span)), "lane", lane.member);
}

/** A span kind's full name (Qt's kindName). */
export function kindName(kind: string, stageKind = ""): string {
  switch (kind) {
    case "work": return tr("Work");
    case "rework": return tr("Rework (a returned attempt)");
    case "build": return tr("Build and merge");
    case "stage": return stageKind === "review" ? tr("Review stage") : stageKind === "test" ? tr("Test stage") : tr("Stage: %1", stageKind);
    case "question": return tr("Blocked on a question");
    case "permission": return tr("Blocked on a permission");
    case "background": return tr("Background work");
    case "delivery": return tr("Delivery");
    case "start_delay": return tr("Start delay");
    case "member": return tr("Waited for a free member");
    case "deps": return tr("Waited for dependencies");
    case "lead": return tr("Waited for the lead");
    case "folder_held": return tr("Folder held by another task");
    case "hold": return tr("Held");
    case "stage_wait": return tr("Waited for a stage");
    case "no_report": return tr("No report");
    case "stop": return tr("Stopped");
    default: return tr("Not recorded");
  }
}

export const titleOf = (view: TeamTimelineView, task: number) => view.titles[String(task)] ?? "";
/** The bar's text: "#12 Title". */
export const taskLabel = (view: TeamTimelineView, task: number) => `#${task} ${titleOf(view, task)}`.trim();

/** The hover lines of a span (Qt's tooltip): the task, then what it was, how long and when. */
export function spanTip(view: TeamTimelineView, span: TimelineSpan, statusName: (status: string) => string = (s) => s): string[] {
  const when = tr("%1 to %2", momentText(span.from), span.running ? tr("now") : momentText(span.to));
  const length = durationText(span.to - span.from);
  if (span.kind === "tests") {
    const lines = [tr("Tests: %1", span.member), `${statusName(span.stage_kind)}, ${length}, ${when}`];
    if (span.task) lines.push(tr("Asked for #%1 %2", span.task, titleOf(view, span.task)));
    return lines;
  }
  return [taskLabel(view, span.task), `${kindName(span.kind, span.stage_kind)}: ${length}, ${when}`];
}

/**
 * The scope the view opens on (Qt's setTeam): the last phase approved or
 * running, else the last phase, else the plan (null).
 */
export function defaultPhase(phases: readonly TeamPhaseView[]): number | null {
  let phase: number | null = null;
  for (const candidate of phases) if (candidate.state === "approved" || candidate.state === "running") phase = candidate.n;
  return phase ?? (phases.length ? phases[phases.length - 1].n : null);
}

/** The test row's colour token by a run's status (Qt's testColor). */
export function testTone(status: string): "accent" | "warn" | "danger" {
  if (status === "passed" || status === "reused") return "accent";
  if (status === "timeout" || status === "timed_out" || status === "timed-out") return "warn";
  return "danger";
}

/** Owner 2026-10-31 (#746): the estimates, the largest overrun first; how many of the known ones ran over. */
export function estimateRows(view: TeamTimelineView) {
  const over = (row: TeamTimelineView["estimates"][number]) => (row.estimate_min ? row.worked_s / 60 - row.estimate_min : -1e9);
  const rows = [...view.estimates].sort((a, b) => over(b) - over(a));
  const known = rows.filter((row) => row.estimate_min && row.worked_s > 0);
  return { rows, known: known.length, past: known.filter((row) => row.worked_s / 60 > (row.estimate_min ?? 0)).length };
}

/** The scope picker's choices: the plan, then each phase (Qt's m_scope, without its separator dots). */
export function scopeChoices(phases: readonly TeamPhaseView[]): { value: string; label: string }[] {
  return [
    { value: "", label: tr("The plan, every phase") },
    ...phases.map((phase) => ({ value: String(phase.n), label: phase.title ? tr("Phase %1: %2", phase.n, phase.title) : tr("Phase %1", phase.n) })),
  ];
}

/** Qt's Cache::Stale: a changed team is read again once the shown read is this old. */
export const TIMELINE_STALE_MS = 60_000;

/**
 * The Time view's reads (Qt's TeamProfile::Cache for one view): `show` reads
 * a scope when it is another one than shown; `changed` reads the shown scope
 * again when the team's revision moved and the read is older than
 * TIMELINE_STALE_MS. One read at a time counts: an answer for a scope or a
 * read no longer current is dropped.
 */
export function createTimelineReads(load: (phase: number | null) => Promise<TeamTimelineView>, now: () => number = Date.now) {
  const [view, setView] = createSignal<TeamTimelineView | null>(null);
  const [error, setError] = createSignal("");
  const [loading, setLoading] = createSignal(false);
  let token = 0, readAt = 0, revision = -1, scope: number | null | undefined;
  const read = (next: number | null, at: number) => {
    const mine = ++token;
    scope = next; readAt = now(); revision = at;
    setLoading(true); setError("");
    load(next).then(
      (answer) => { if (mine === token) { setView(answer); setLoading(false); } },
      (failure) => { if (mine === token) { setError(failure instanceof Error ? failure.message : String(failure)); setLoading(false); } },
    );
  };
  return {
    view, error, loading,
    scope: () => scope ?? null,
    reads: () => token,
    /** Shows a scope; another scope than shown is read at once and the old lanes go. */
    show(next: number | null, at: number) {
      if (scope !== undefined && next === scope) return;
      setView(null);
      read(next, at);
    },
    /** The team changed: the shown scope is read again once its read is stale. */
    changed(at: number) {
      if (scope === undefined || loading() || at === revision || now() - readAt <= TIMELINE_STALE_MS) return;
      read(scope, at);
    },
  };
}
