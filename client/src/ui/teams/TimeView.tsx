import { For, Show, createEffect, createMemo, createSignal, on, onCleanup, onMount, untrack, type JSX } from "solid-js";
import { tr } from "../../i18n";
import { skin } from "../../skin/runtime";
import type { TeamPhaseView, TeamProfileView, TeamTimelineView, TeamView, TimelineSpan } from "../../types";
import { durationText, taskHeat } from "./heat";
import { colorOf, kindKey, labelOn, styleOf, type Pattern } from "./profile-palette";
import { roleColor } from "./roles";
import { ProfileLegend } from "./plan-panels";
import {
  TL, atOf, clamp, createTimelineReads, defaultPhase, estimateRows, fit, heightFor, panBy, pixelsToSeconds, shown, spanAt, spanTip, taskLabel,
  scopeChoices, testTone, ticks, titleOf, xOf, zoomBy, type Hit, type Viewport,
} from "./timeline";

// Task 92: the profile's Time view (Qt's TeamProfileView::timePage): the
// scope picker, the timeline drawn on one canvas (never an element per span),
// zoom and pan, hover names, the legend's kinds and the estimates. The
// lanes come from the core (`team_timeline`), read only while the view shows.

const SPAN_PAGES_DRAWN = 20 * 50;

/** A wait's hatch as a canvas pattern, the same pattern Qt's brush draws. */
function hatch(ctx: CanvasRenderingContext2D, color: string, pattern: Pattern): CanvasPattern | string {
  if (pattern === "solid") return color;
  const tile = document.createElement("canvas");
  tile.width = tile.height = 6;
  const t = tile.getContext("2d");
  if (!t) return color;
  t.strokeStyle = color; t.fillStyle = color; t.lineWidth = 1;
  const line = (x1: number, y1: number, x2: number, y2: number) => { t.beginPath(); t.moveTo(x1, y1); t.lineTo(x2, y2); t.stroke(); };
  if (pattern === "dots") { t.fillRect(1, 1, 1.5, 1.5); t.fillRect(4, 4, 1.5, 1.5); }
  // Task 118: waiting for a person's answer, one round dot in the tile (Qt: a texture of the same dot).
  if (pattern === "sparse") { t.beginPath(); t.arc(3, 3, 1.3, 0, Math.PI * 2); t.fill(); }
  if (pattern === "bdiag" || pattern === "diagcross") { line(0, 6, 6, 0); line(-1, 1, 1, -1); line(5, 7, 7, 5); }
  if (pattern === "fdiag" || pattern === "diagcross") { line(0, 0, 6, 6); line(-1, 5, 1, 7); line(5, -1, 7, 1); }
  if (pattern === "hor" || pattern === "cross") line(0, 3, 6, 3);
  if (pattern === "ver" || pattern === "cross") line(3, 0, 3, 6);
  return ctx.createPattern(tile, "repeat") ?? color;
}

function token(el: Element, name: string, fallback: string): string {
  const value = getComputedStyle(el).getPropertyValue(name).trim();
  return value || fallback;
}

function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  ctx.beginPath();
  const radius = Math.min(r, w / 2, h / 2);
  ctx.moveTo(x + radius, y); ctx.arcTo(x + w, y, x + w, y + h, radius); ctx.arcTo(x + w, y + h, x, y + h, radius);
  ctx.arcTo(x, y + h, x, y, radius); ctx.arcTo(x, y, x + w, y, radius); ctx.closePath();
}

function elide(ctx: CanvasRenderingContext2D, text: string, width: number): string {
  if (ctx.measureText(text).width <= width) return text;
  let low = 0, high = text.length;
  while (low < high) { const mid = (low + high + 1) >> 1; if (ctx.measureText(`${text.slice(0, mid)}…`).width <= width) low = mid; else high = mid - 1; }
  return low > 0 ? `${text.slice(0, low)}…` : "";
}

export function TimeCanvas(props: {
  view: TeamTimelineView;
  profile?: TeamProfileView | null;
  statusName: (status: string) => string;
  onOpen?: (task: number) => void;
}): JSX.Element {
  let host!: HTMLDivElement;
  let canvas!: HTMLCanvasElement;
  const [width, setWidth] = createSignal(900);
  const [vp, setVp] = createSignal<Viewport>({ from: props.view.from, to: props.view.to, zoom: 1, start: props.view.from, width: 900 });
  const [selected, setSelected] = createSignal(0);
  const [tip, setTip] = createSignal<{ x: number; y: number; lines: string[] } | null>(null);
  const dark = () => skin().appearance === "dark";
  // A new scope keeps the zoom only while its window stays the same.
  createEffect(on(() => [props.view.from, props.view.to] as const, ([from, to]) => {
    setVp((old) => (old.from === from && old.to === to ? old : clamp({ from, to, zoom: 1, start: from, width: old.width })));
  }, { defer: true }));
  createEffect(() => setVp((old) => ({ ...old, width: width() })));

  const draw = () => {
    const view = props.view, v = vp();
    const ratio = window.devicePixelRatio || 1;
    const height = heightFor(view);
    canvas.width = Math.round(v.width * ratio); canvas.height = Math.round(height * ratio);
    canvas.style.width = `${v.width}px`; canvas.style.height = `${height}px`;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    ctx.clearRect(0, 0, v.width, height);
    const isDark = dark();
    const c = {
      fg: token(host, "--z-color-text", isDark ? "#e8eaed" : "#1b1f24"),
      muted: token(host, "--z-color-muted", "#8a929b"),
      border: token(host, "--z-color-card-border", "#3a3f46"),
      danger: token(host, "--z-color-danger", "#d9534f"),
      accent: token(host, "--z-color-accent", "#3f9f7b"),
      warn: token(host, "--z-color-warn", "#c99a2e"),
    };
    const small = "11px system-ui, sans-serif", bold = "600 11px system-ui, sans-serif";
    ctx.font = small; ctx.textBaseline = "middle";
    // The axis.
    ctx.fillStyle = c.muted; ctx.textAlign = "left";
    ctx.fillText(tr("MEMBERS"), 12, 16);
    ctx.textAlign = "center";
    for (const tick of ticks(v)) {
      const px = Math.round(xOf(v, tick.at)) + 0.5;
      ctx.globalAlpha = 0.6; ctx.strokeStyle = c.border; ctx.beginPath(); ctx.moveTo(px, TL.Top - 4); ctx.lineTo(px, height - 6); ctx.stroke(); ctx.globalAlpha = 1;
      ctx.fillStyle = c.muted; ctx.fillText(tick.label, px, 16);
    }
    let y = TL.Top;
    const rowLabel = (text: string, right: string, strong: boolean) => {
      ctx.textAlign = "left"; ctx.font = strong ? bold : small; ctx.fillStyle = c.fg;
      ctx.fillText(elide(ctx, text, TL.Label - 82), 12, y + TL.Row / 2);
      ctx.font = small; ctx.fillStyle = c.muted; ctx.textAlign = "right";
      ctx.fillText(right, TL.Label - 10, y + TL.Row / 2);
    };
    // Spans draw inside the plot only.
    const clip = () => { ctx.save(); ctx.beginPath(); ctx.rect(TL.Label - 2, 0, v.width - TL.Label + 2, height); ctx.clip(); };
    const bar = (span: TimelineSpan, top: number, h: number, color: string, dashed: boolean, text: string) => {
      if (!shown(v, span.from, span.to)) return;
      const x = xOf(v, span.from), w = Math.max(2, xOf(v, span.to) - x);
      roundRect(ctx, x, top, w, h, 3);
      if (dashed) { ctx.setLineDash([4, 3]); ctx.lineWidth = 1.2; ctx.strokeStyle = color; ctx.stroke(); ctx.setLineDash([]); }
      else { ctx.fillStyle = color; ctx.fill(); }
      if (span.task && span.task === selected()) { roundRect(ctx, x - 1, top - 1, w + 2, h + 2, 4); ctx.lineWidth = 2; ctx.strokeStyle = c.fg; ctx.stroke(); }
      if (text && w > 40) {
        ctx.font = small; ctx.textAlign = "left"; ctx.fillStyle = dashed ? c.fg : labelOn(color);
        ctx.fillText(elide(ctx, text, w - 9), x + 5, top + h / 2);
      }
    };
    const color = (span: TimelineSpan) => colorOf(kindKey(span.kind, span.stage_kind), isDark) ?? c.muted;
    // The real critical path, then the planned one.
    rowLabel(tr("Real critical path"), durationText(view.real_s), true);
    clip(); for (const span of view.real) bar(span, y + 14, 22, colorOf("work", isDark)!, false, taskLabel(view, span.task)); ctx.restore();
    y += TL.Row;
    rowLabel(tr("Planned path"), durationText(view.planned_s), false);
    clip(); for (const span of view.planned) bar(span, y + 14, 22, c.fg, true, `#${span.task}`); ctx.restore();
    y += TL.Row;
    for (const lane of view.lanes) {
      // The member as its role-coloured pill, and its utilization.
      const tone = roleColor(lane.role || lane.member)[isDark ? "dark" : "light"];
      ctx.font = small;
      const w = Math.min(TL.Label - 90, ctx.measureText(lane.member).width + 16);
      roundRect(ctx, 12, y + TL.Row / 2 - 10, w, 20, 10);
      ctx.globalAlpha = 0.14; ctx.fillStyle = tone; ctx.fill(); ctx.globalAlpha = 0.7; ctx.strokeStyle = tone; ctx.lineWidth = 1; ctx.stroke(); ctx.globalAlpha = 1;
      ctx.fillStyle = tone; ctx.textAlign = "center"; ctx.fillText(elide(ctx, lane.member, w - 10), 12 + w / 2, y + TL.Row / 2);
      ctx.fillStyle = c.muted; ctx.textAlign = "right"; ctx.fillText(`${Math.round(lane.utilization * 100)}%`, TL.Label - 10, y + TL.Row / 2);
      clip();
      // Idle while tasks were open: a dashed outline where the work would be.
      for (const [from, to] of lane.idle) {
        if (!shown(v, from, to)) continue;
        const x = xOf(v, from), wide = Math.max(1, xOf(v, to) - x);
        roundRect(ctx, x, y + TL.WorkTop, wide, TL.WorkHeight, 3);
        ctx.globalAlpha = 0.07; ctx.fillStyle = c.danger; ctx.fill(); ctx.globalAlpha = 0.7;
        ctx.setLineDash([4, 3]); ctx.strokeStyle = c.danger; ctx.lineWidth = 1; ctx.stroke(); ctx.setLineDash([]); ctx.globalAlpha = 1;
        if (wide > 70) { ctx.fillStyle = c.danger; ctx.textAlign = "left"; ctx.fillText(tr("idle %1", durationText(to - from)), x + 5, y + TL.WorkTop + TL.WorkHeight / 2); }
      }
      // Task #79: the work bar keeps its title; build and merge run as a thin strip along the bottom.
      for (const span of lane.spans) {
        if (span.kind === "build") continue;
        bar(span, y + TL.WorkTop, TL.WorkHeight, color(span), false, span.kind === "work" || span.kind === "rework" ? taskLabel(view, span.task) : "");
      }
      for (const span of lane.spans) if (span.kind === "build") bar(span, y + TL.BuildTop, TL.BuildHeight, colorOf("build", isDark)!, false, "");
      // The waits of its tasks: a strip under the work, each kind its own hatch.
      for (const span of lane.waits) {
        if (!shown(v, span.from, span.to)) continue;
        const key = kindKey(span.kind, span.stage_kind), tone = colorOf(key, isDark) ?? c.muted, style = styleOf(key);
        const x = xOf(v, span.from), wide = Math.max(2, xOf(v, span.to) - x);
        roundRect(ctx, x, y + TL.WaitTop, wide, TL.WaitHeight, 2);
        ctx.fillStyle = hatch(ctx, tone, style?.pattern ?? "solid"); ctx.fill();
        ctx.globalAlpha = 0.9; ctx.strokeStyle = tone; ctx.lineWidth = 1; ctx.stroke(); ctx.globalAlpha = 1;
        if (span.task === selected()) { roundRect(ctx, x - 1, y + TL.WaitTop - 1, wide + 2, TL.WaitHeight + 2, 2); ctx.lineWidth = 1.5; ctx.strokeStyle = c.fg; ctx.stroke(); }
      }
      ctx.restore();
      ctx.globalAlpha = 0.5; ctx.strokeStyle = c.border; ctx.beginPath(); ctx.moveTo(12, y + TL.Row - 0.5); ctx.lineTo(v.width - 12, y + TL.Row - 0.5); ctx.stroke(); ctx.globalAlpha = 1;
      y += TL.Row;
    }
    // Task #719: the test runs, coloured by status, named by suite.
    if (view.tests.length) {
      rowLabel(tr("Tests"), view.tests_more ? `${view.tests.length}+` : String(view.tests.length), true);
      clip(); for (const run of view.tests) bar(run, y + 12, 18, c[testTone(run.stage_kind)], false, run.member); ctx.restore();
    }
  };

  onMount(() => {
    const observer = new ResizeObserver(() => setWidth(Math.max(TL.Label + 120, Math.floor(host.clientWidth))));
    observer.observe(host);
    setWidth(Math.max(TL.Label + 120, Math.floor(host.clientWidth)));
    onCleanup(() => observer.disconnect());
  });
  // One draw per change of the view, the viewport, the selection or the theme; hovering never redraws.
  createEffect(on(() => [props.view, vp(), selected(), dark()] as const, () => draw()));

  const point = (event: MouseEvent) => { const r = canvas.getBoundingClientRect(); return { x: event.clientX - r.left, y: event.clientY - r.top }; };
  let press: { x: number; start: number } | null = null, dragged = false;
  const onWheel = (event: WheelEvent) => {
    const p = point(event);
    if (event.ctrlKey) { event.preventDefault(); setVp((old) => zoomBy(old, Math.pow(1.25, -event.deltaY / 100), atOf(old, p.x))); }
    else if (event.shiftKey || event.deltaX !== 0) {
      event.preventDefault();
      const steps = (event.deltaX !== 0 ? event.deltaX : event.deltaY) / 100;
      setVp((old) => panBy(old, (steps * (old.to - old.from)) / old.zoom / 10));
    }
  };
  const onDown = (event: MouseEvent) => { if (event.button === 0) { press = { x: point(event).x, start: vp().start }; dragged = false; } };
  const onMove = (event: MouseEvent) => {
    const p = point(event);
    if (press && (event.buttons & 1) && vp().zoom > 1) {
      const dx = p.x - press.x;
      if (!dragged && Math.abs(dx) < 5) return;
      dragged = true; setTip(null);
      const start = press.start;
      setVp((old) => clamp({ ...old, start: start - pixelsToSeconds(old, dx) }));
      return;
    }
    const hit: Hit | null = spanAt(props.view, vp(), p.x, p.y);
    if (!hit) { setTip(null); return; }
    const lines = spanTip(props.view, hit.span, props.statusName);
    const heat = hit.span.kind !== "tests" && hit.span.task ? taskHeat(props.profile, hit.span.task) : null;
    if (heat?.label) lines.push(heat.label);
    if (heat?.badge) lines.push(heat.badge);
    setTip({ x: p.x, y: p.y, lines });
  };
  const onUp = (event: MouseEvent) => {
    const wasDrag = dragged; press = null; dragged = false;
    if (wasDrag) return;
    const p = point(event);
    const hit = spanAt(props.view, vp(), p.x, p.y);
    if (hit?.span.task) { setSelected(hit.span.task); props.onOpen?.(hit.span.task); }
  };

  return (
    <div class="tl">
      <div class="tl-zoom">
        <span class="pm-note">{tr("Ctrl+wheel zooms at the pointer, drag pans, double click fits")}</span>
        <span class="grow" />
        <button type="button" class="btn ghost small" aria-label={tr("Zoom out")} title={tr("Zoom out")} onClick={() => setVp((old) => zoomBy(old, 1 / 1.5))}>−</button>
        <button type="button" class="btn ghost small" title={tr("Show the whole scope")} onClick={() => setVp((old) => fit(old))}>{tr("Fit")}</button>
        <button type="button" class="btn ghost small" aria-label={tr("Zoom in")} title={tr("Zoom in")} onClick={() => setVp((old) => zoomBy(old, 1.5))}>+</button>
      </div>
      <div class="tl-host" ref={host} classList={{ panning: vp().zoom > 1 }}>
        <canvas ref={canvas} role="img" aria-label={tr("Real critical path")} data-zoom={vp().zoom}
          onWheel={onWheel} onMouseDown={onDown} onMouseMove={onMove} onMouseUp={onUp}
          onMouseLeave={() => { setTip(null); press = null; }} onDblClick={() => setVp((old) => fit(old))} />
        <Show when={tip()}>{(t) => (
          <div class="tl-tip" style={{ left: `${Math.min(t().x + 14, width() - 260)}px`, top: `${t().y + 14}px` }}>
            <For each={t().lines}>{(line, i) => <div classList={{ "tl-tip-head": i() === 0 }}>{line}</div>}</For>
          </div>
        )}</Show>
      </div>
    </div>
  );
}

/** Owner 2026-10-31 (#746): each task's estimate against its working time, the largest overrun first. */
function Estimates(props: { view: TeamTimelineView; onOpen?: (task: number) => void }): JSX.Element {
  const rows = createMemo(() => estimateRows(props.view));
  const [open, setOpen] = createSignal(true);
  return (
    <Show when={rows().rows.length > 0}>
      <div class="tl-estimates">
        <div class="tl-estimates-head">
          <button type="button" class="btn ghost small" aria-expanded={open()} onClick={() => setOpen(!open())}>{open() ? "▾" : "▸"}</button>
          <b>{tr("ESTIMATE AND ACTUAL")}</b>
          <Show when={rows().known > 0}><span classList={{ "pm-note": true, warn: rows().past > 0 }}>{tr("%1 of %2 over the estimate", rows().past, rows().known)}</span></Show>
        </div>
        <Show when={open()}>
          <table class="tl-estimates-table">
            <thead><tr><th>{tr("TASK")}</th><th>{tr("ESTIMATE")}</th><th>{tr("ACTUAL")}</th><th>{tr("DIFFERENCE")}</th></tr></thead>
            <tbody>
              <For each={rows().rows.slice(0, 30)}>{(row) => {
                const diff = row.estimate_min ? row.worked_s - row.estimate_min * 60 : null;
                return (
                  <tr>
                    <td><button type="button" class="link" onClick={() => props.onOpen?.(row.task)}>{`#${row.task} ${titleOf(props.view, row.task)}`}</button></td>
                    <td>{row.estimate_min ? durationText(row.estimate_min * 60) : tr("no estimate")}</td>
                    <td>{durationText(row.worked_s)}{row.running ? ` ${tr("now")}` : ""}</td>
                    <td classList={{ warn: (diff ?? 0) > 0 }}>{diff == null ? "" : `${diff > 0 ? "+" : "−"}${durationText(Math.abs(diff))}`}</td>
                  </tr>
                );
              }}</For>
            </tbody>
          </table>
        </Show>
      </div>
    </Show>
  );
}

export function TimeSection(props: {
  phases: readonly TeamPhaseView[];
  phase: number | null;
  onPhase: (phase: number | null) => void;
  view?: TeamTimelineView | null;
  error?: string;
  loading?: boolean;
  profile?: TeamProfileView | null;
  statusName: (status: string) => string;
  onOpen?: (task: number) => void;
}): JSX.Element {
  return (
    <div class="tl-section">
      <div class="tl-head">
        <h3>{tr("Time")}</h3>
        <select class="tl-scope" aria-label={tr("Scope")} value={props.phase == null ? "" : String(props.phase)}
          onChange={(event) => props.onPhase(event.currentTarget.value ? Number(event.currentTarget.value) : null)}>
          <For each={scopeChoices(props.phases)}>{(choice) => <option value={choice.value}>{choice.label}</option>}</For>
        </select>
        <Show when={props.loading}><span class="pm-note">{tr("Reading the timeline…")}</span></Show>
      </div>
      <Show when={props.error}><p class="pm-empty warn">{props.error}</p></Show>
      <Show when={props.view} fallback={<Show when={!props.error}><p class="pm-empty">{tr("Reading the timeline…")}</p></Show>}>
        {(view) => (
          <>
            <TimeCanvas view={view()} profile={props.profile} statusName={props.statusName} onOpen={props.onOpen} />
            <Show when={view().truncated}><p class="pm-note warn">{tr("Only the first %1 tasks are drawn", SPAN_PAGES_DRAWN)}</p></Show>
            {/* Task #339: the legend sits with the lanes it explains, as Qt's
                teamProfileLegend directly under the timeline card. */}
            <ProfileLegend />
            <Estimates view={view()} onOpen={props.onOpen} />
          </>
        )}
      </Show>
    </div>
  );
}

/**
 * The Time section with its own scope (the phase in work at first) and its
 * reads (timeline.ts createTimelineReads): a read when the scope changes, and
 * again when the team changed and the shown read is old enough.
 */
export function ProfileTime(props: {
  team: TeamView;
  load: (phase: number | null) => Promise<TeamTimelineView>;
  profile?: TeamProfileView | null;
  statusName: (status: string) => string;
  onOpen?: (task: number) => void;
}): JSX.Element {
  const [phase, setPhase] = createSignal<number | null>(defaultPhase(props.team.phases));
  const reads = createTimelineReads((scope) => props.load(scope));
  createEffect(on(phase, (scope) => reads.show(scope, untrack(() => props.team.revision))));
  createEffect(on(() => props.team.revision, (revision) => reads.changed(revision), { defer: true }));
  return (
    <TimeSection phases={props.team.phases} phase={phase()} onPhase={setPhase} view={reads.view()} error={reads.error()}
      loading={reads.loading() && !!reads.view()} profile={props.profile} statusName={props.statusName} onOpen={props.onOpen} />
  );
}
