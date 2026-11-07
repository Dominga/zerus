import { For, Show, createEffect, createMemo, createSignal, onMount, type JSX } from "solid-js";
import { createVirtualizer } from "@tanstack/solid-virtual";
import { Icon } from "../lib/icons";
import { tr } from "../../i18n";
import type { PlanGraph, TeamView } from "./types";
import type { TeamProfileView, TeamTestsView, TeamTimelineView } from "../../types";
import { BoardView, HeatLegend, HeatLine, LayersView, PlanModeBar, ProfilePanel, TestsPanel, runStatusLabel, stageLabel } from "./plan-panels";
import { ProfileTime } from "./TimeView";
import { spawnedFrom, heatSeed, stageChips, type PlanMode } from "./planModes";
import { settings, settingsReady } from "../../store/settings";
import { taskHeat } from "./heat";
import {
  CARD_H,
  ROW_H,
  buildRows,
  chain,
  edgePath,
  edgeState,
  externalBlocks,
  externalDeps,
  phaseNeedsApproval,
  phasePlans,
  phaseStatus,
  rowEdges,
  rowOrigins,
  type PlacedNode,
  type PlanPhase,
  type PlanRow,
} from "./plan";
import { stageName } from "./actions";
import { phaseHeader } from "./phaseHeader";
import "../styles/plan.css";

export interface PlanViewProps {
  team: TeamView;
  graph: PlanGraph;
  /** Opens the task in the host's inspector; without it the plan shows its own card. */
  onOpenTask?: (id: number) => void;
  onApprovePhase?: (n: number) => void;
  onDiscardPhase?: (n: number) => void;
  profile?: TeamProfileView | null;
  tests?: TeamTestsView | null;
  /** The Tests mode asks for its page when opened; the host reads it through
   * the core and passes it back. */
  onLoadTests?: (before: number | null) => void;
  /** The Profile mode asks for the team's profile when opened. */
  onLoadProfile?: () => void;
  /** Task 92: the Profile mode's Time view reads one scope (a phase, or the plan with null) through the core. */
  loadTimeline?: (phase: number | null) => Promise<TeamTimelineView>;
  /** Task 40: the task card's actions (ui/teams/TeamActions.tsx TaskActions), drawn under the task's detail. */
  taskActions?: (id: number) => JSX.Element;
  /** Task #58: a phase header's mode chip and switch (ui/teams/TeamActions.tsx PhaseModeControl).
   * Task 196: and the phase's own actions and menu (TeamActions.tsx PhaseActions); with them the
   * header draws no Approve or Discard of its own. */
  phaseActions?: (n: number) => JSX.Element;
  /**
   * Task 196: editing the plan (Qt planning mode and a draft's edits): which
   * tasks take a dependency, a drag from a card's right edge onto another
   * links them, an edge's click removes it (asked first), and a click on a
   * card edits it while the team plans.
   */
  editing?: {
    canLink: (task: number) => boolean;
    link: (dependency: number, task: number) => void;
    unlink: (dependency: number, task: number) => void;
    editTask?: (id: number) => void;
  };
  class?: string;
}

const TONE: Record<string, string> = {
  todo: "pending",
  pending: "pending",
  ready: "pending",
  working: "working",
  in_progress: "working",
  running: "working",
  review: "review",
  reviewing: "review",
  submitted: "submitted",
  done: "done",
  accepted: "done",
  failed: "danger",
  blocked: "danger",
  problem: "danger",
  cancelled: "muted",
  archived: "muted",
};

function tone(status: string): string {
  return TONE[status] ?? "pending";
}

function short(value: string, limit = 150): string {
  return value.length > limit ? `${value.slice(0, limit - 1)}…` : value;
}

function memberModel(team: TeamView, assignee: string): string {
  const member = team.members.find((candidate) => candidate.id === assignee);
  return member?.model || member?.agent || "";
}

function statusLabel(kind: string): string {
  // Task 196: Qt's phase state badges.
  if (kind === "rejected") return tr("Sent back");
  if (kind === "awaiting") return tr("Awaiting your acceptance");
  if (kind === "queued") return tr("Accepted, queued");
  if (kind === "planned") return tr("In the plan");
  if (kind === "cancelled") return tr("Cancelled");
  if (kind === "done") return tr("Done");
  if (kind === "running") return tr("Running");
  if (kind === "approved") return tr("Approved");
  if (kind === "draft") return tr("Draft");
  if (kind === "paused") return tr("Paused");
  return tr("Open");
}

function readFlag(key: string): boolean {
  try { return localStorage.getItem(key) === "1"; } catch { return false; }
}

export function PlanView(props: PlanViewProps): JSX.Element {
  const [mode, setMode] = createSignal<PlanMode>("graph");
  const [includeCancelled, setIncludeCancelled] = createSignal(false);
  // Task 82: the lateness heat, off by default and remembered per team in
  // this browser only (a convenience; nothing depends on it).
  const heatKey = () => `zerus.teams.heat.${props.team.id}`;
  const [heatOn, setHeatOn] = createSignal(readFlag(heatKey()));
  const switchHeat = (on: boolean) => {
    setHeatOn(on);
    try { if (on) localStorage.setItem(heatKey(), "1"); else localStorage.removeItem(heatKey()); } catch { /* storage unavailable */ }
  };
  // Task 103: Qt's imported per-team heat starts this switch on once; this
  // browser's own choice, once made, always wins.
  onMount(() => {
    void settingsReady.then(() => {
      try {
        if (localStorage.getItem(heatKey()) === null && heatSeed(props.team.id, null, String(settings["teams/heatModes"] ?? "{}"))) {
          setHeatOn(true);
          localStorage.setItem(heatKey(), "1");
        }
      } catch { /* storage unavailable */ }
    });
  });
  const heatView = () => heatOn() && (mode() === "graph" || mode() === "board" || mode() === "layers");
  const heatProfile = () => (heatView() ? props.profile : null);
  createEffect(() => {
    if (mode() === "tests") props.onLoadTests?.(null);
    if (mode() === "profile" || heatView()) props.onLoadProfile?.();
  });
  const [collapsed, setCollapsed] = createSignal<Set<number>>(
    new Set(props.team.phases.filter((phase) => phase.status === "done").map((phase) => phase.n)),
  );
  const [hover, setHover] = createSignal<number | null>(null);
  const [selected, setSelected] = createSignal<number | null>(null);
  const phases = createMemo(() => phasePlans(props.team, props.graph));
  const rows = createMemo(() => buildRows(phases(), collapsed()));
  const rowByKey = createMemo(() => new Map(rows().map((row) => [row.key, row])));
  const selectedTask = createMemo(() => props.team.tasks.find((task) => task.id === selected()));
  const lit = createMemo(() => (hover() === null ? null : chain(props.team.tasks, hover()!)));
  const byId = createMemo(() => new Map(props.team.tasks.map((task) => [task.id, task])));

  let scroller: HTMLDivElement | undefined;
  const virtualizer = createVirtualizer({
    get count() {
      return rows().length;
    },
    getScrollElement: () => (scroller?.isConnected ? scroller : null),
    estimateSize: (index) => rows()[index]?.height ?? ROW_H,
    getItemKey: (index) => rows()[index]?.key ?? index,
    overscan: 6,
  });
  const virtualRows = createMemo(() => virtualizer.getVirtualItems());

  const toggle = (n: number) => {
    const next = new Set(collapsed());
    if (next.has(n)) next.delete(n);
    else next.add(n);
    setCollapsed(next);
  };
  const collapseAll = () => setCollapsed(new Set<number>(phases().map((phase) => phase.phase.n)));
  const expandAll = () => setCollapsed(new Set<number>());
  const open = (id: number) => {
    if (props.onOpenTask) props.onOpenTask(id);
    else setSelected(id);
  };
  const phaseOf = (id: number) => phases().find((phase) => phase.tasks.some((task) => task.id === id));
  // Task 196: a drag from a card's right edge onto another card makes that
  // task depend on this one (Qt's graph editing).
  const startLink = (event: PointerEvent, from: number) => {
    event.preventDefault();
    event.stopPropagation();
    const handle = event.currentTarget as HTMLElement;
    try { handle.setPointerCapture(event.pointerId); } catch { /* a synthetic event */ }
    const done = (up: PointerEvent) => {
      handle.removeEventListener("pointerup", done);
      const target = document.elementFromPoint(up.clientX, up.clientY)?.closest("[data-task]");
      const to = Number(target?.getAttribute("data-task"));
      if (to && to !== from && props.editing?.canLink(to)) props.editing.link(from, to);
    };
    handle.addEventListener("pointerup", done);
  };

  const header = (phase: PlanPhase, row: PlanRow, actions?: JSX.Element) => {
    const status = phaseStatus(phase.phase);
    // Task 196: Qt's header by state: its counts (cancelled tasks are not owed),
    // notes, the collapsed one-line summary and the lead's note.
    const head = phaseHeader(props.team, phase.phase);
    const total = head.total;
    const done = Math.min(total, phase.phase.done || head.done);
    const isCollapsed = collapsed().has(phase.phase.n);
    const note = phase.phase.note?.trim() ?? "";
    return (
      <div
        class={`pt-phase pt-phase-${status.kind}${row.end ? " pt-row-end" : ""}`}
        style={{ "min-width": `${Math.max(phase.width, 320)}px` }}
        role="button"
        tabindex="0"
        aria-label={tr("Phase %1", phase.phase.n)}
        aria-expanded={!isCollapsed}
        onClick={() => toggle(phase.phase.n)}
        onKeyDown={(event) => {
          if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            toggle(phase.phase.n);
          }
        }}
      >
        <Icon name={isCollapsed ? "chevron-right" : "chevron-down"} size={14} />
        <span class="pt-pn">{phase.phase.n}</span>
        <b class="pt-phase-title">{phase.phase.title || tr("Phase %1", phase.phase.n)}</b>
        <span class={`pt-chip pt-chip-${status.kind}`}>{statusLabel(status.kind)}</span>
        <Show when={phase.phase.acceptance}>
          <span class="pt-acceptance">{short(phase.phase.acceptance, 80)}</span>
        </Show>
        <Show when={phase.phase.auto}>
          <span class="pt-chip pt-chip-auto">{tr("Auto")}</span>
        </Show>
        <Show when={phase.phase.barrier}>
          <span class="pt-chip pt-chip-barrier">{tr("Barrier")}</span>
        </Show>
        <For each={head.notes}>{(shown) => (
          <span class={`pt-phase-note pt-phase-note-${shown.tone}`} data-note={shown.id} title={shown.tip || shown.text}>{short(shown.text, 80)}</span>
        )}</For>
        <Show when={isCollapsed && head.summary}>
          <span class="pt-phase-summary" data-summary="1" title={head.summary.split(", #").join("\n#")}>{short(head.summary, 140)}</span>
        </Show>
        {actions}
        <span class="pt-grow" />
        <Show when={phase.phase.health}>
          <span class="pt-health">{short(phase.phase.health, 60)}</span>
        </Show>
        <Show when={total > 0}>
          <span class="pt-progress" aria-label={tr("%1 of %2", done, total)}>
            <For each={Array.from({ length: Math.min(12, total) }, (_, index) => index < Math.round((done / total) * Math.min(12, total)))}>
              {(on) => <i class={on ? "on" : ""} />}
            </For>
          </span>
          <span class="pt-count">
            {done}/{total}
          </span>
        </Show>
        <Show when={!isCollapsed && note}>
          <span class="pt-phase-lead-note" data-note="lead" title={note}>{tr("Lead's note: %1", short(note, 160))}</span>
        </Show>
        <Show when={phaseNeedsApproval(phase.phase) && !props.phaseActions}>
          <span class="pt-approval">{tr("Needs approval")}</span>
          <Show when={props.onApprovePhase}>
            <button
              type="button"
              class="btn small"
              onClick={(event) => {
                event.stopPropagation();
                props.onApprovePhase?.(phase.phase.n);
              }}
            >
              {tr("Approve")}
            </button>
          </Show>
          <Show when={props.onDiscardPhase}>
            <button
              type="button"
              class="btn ghost small"
              onClick={(event) => {
                event.stopPropagation();
                props.onDiscardPhase?.(phase.phase.n);
              }}
            >
              {tr("Discard")}
            </button>
          </Show>
        </Show>
      </div>
    );
  };

  const card = (placed: PlacedNode, phase: PlanPhase) => {
    const task = placed.task;
    const state = task ? tone(task.status) : "pending";
    const blocks = task ? externalDeps(phase.phase.n, task, byId()) : [];
    const waits = task?.waits?.trim();
    const attempt = task?.attempts ? tr("Attempt %1", task.attempts) : "";
    const stateText = task?.attempt_state?.trim() || "";
    const classes = ["pt-card", `pt-card-${state}`];
    const stages = stageChips(task?.pipeline_run);
    const heat = taskHeat(heatProfile(), placed.node.id);
    const litNow = lit();
    if (litNow) classes.push(litNow.has(placed.node.id) ? (hover() === placed.node.id ? "pt-card-on" : "pt-card-lit") : "pt-card-dim");
    if (task?.pinned) classes.push("pt-card-pinned");
    return (
      <button
        type="button"
        class={classes.join(" ")}
        style={{ left: `${placed.x}px`, top: "0px", "--heat": String(heat?.tint ?? 0) }}
        title={heat?.tip || undefined}
        data-task={placed.node.id}
        data-lit={litNow ? (litNow.has(placed.node.id) ? "lit" : "dim") : "none"}
        aria-label={tr("Task %1: %2", placed.node.id, placed.node.title || task?.title || "")}
        onPointerEnter={() => setHover(placed.node.id)}
        onFocus={() => setHover(placed.node.id)}
        onBlur={() => setHover(null)}
        onClick={() => (props.editing?.editTask ? props.editing.editTask(placed.node.id) : open(placed.node.id))}
      >
        <Show when={props.editing?.canLink(placed.node.id) || (props.editing && placed.node.status !== "cancelled" && props.editing.editTask)}>
          <span
            class="pt-link-handle"
            role="presentation"
            title={tr("Drag from a node's right edge onto a task to connect them")}
            onClick={(event) => event.stopPropagation()}
            onPointerDown={(event) => startLink(event, placed.node.id)}
          />
        </Show>
        <span class="pt-card-l1">
          <span class={`pt-dot pt-dot-${state}`} />
          <span class="pt-id">#{placed.node.id}</span>
          <span class="pt-title">{placed.node.title || task?.title || tr("Task %1", placed.node.id)}</span>
          <Show when={task?.pinned}>
            <span class="pt-badge">{tr("Pinned")}</span>
          </Show>
          <Show when={task?.held}>
            <span class="pt-badge">{tr("Held")}</span>
          </Show>
          <Show when={task?.pipeline_run?.open}>{(open) => (
            <span class="pt-badge" data-stage={open().kind} title={tr("Pipeline stage")}>{stageName(open().kind)}</span>
          )}</Show>
          <Show when={!task?.pipeline_run && task?.pipeline && task.pipeline !== "none"}>
            <span class="pt-badge" title={tr("Pipeline")}>{task!.pipeline}</span>
          </Show>
        </span>
        <span class="pt-card-l2">
          <span class="pt-role">{placed.node.role || task?.assignee || tr("Unassigned")}</span>
          <Show when={memberModel(props.team, task?.assignee ?? "")}>
            <span class="pt-model">{memberModel(props.team, task?.assignee ?? "")}</span>
          </Show>
          <Show when={attempt || stateText}>
            <span class="pt-meta">{stateText || attempt}</span>
          </Show>
          <span class="pt-grow" />
          <HeatLine heat={heat} compact />
          <Show when={spawnedFrom(placed.node, task)}>{(origin) => (
            <span class="pt-origin" data-origin={origin()} title={tr("Spawned from #%1; it does not wait for it", origin())}>
              {tr("Spawned from #%1", origin())}
            </span>
          )}</Show>
          <Show when={blocks.length > 0}>
            <span class="pt-deps" title={tr("Depends on tasks in other phases")}>
              ↤{blocks.map((id) => `#${id}`).join(" ")}
            </span>
          </Show>
        </span>
        <span class="pt-card-l3">
          <Show when={stages.length > 0} fallback={<Show when={waits} fallback={<span class="pt-muted">{placed.node.ready ? tr("ready") : short(placed.node.status, 24)}</span>}><span class="pt-waits">{short(waits!, 76)}</span></Show>}>
            <span class="pt-stages">
              <For each={stages}>
                {(stage) => <span class={`pt-stage pt-stage-${stage.state}`}>{stageLabel(stage.kind)}{stage.attempt > 1 ? ` ${stage.attempt}` : ""}</span>}
              </For>
            </span>
          </Show>
        </span>
      </button>
    );
  };

  const layer = (phase: PlanPhase, row: PlanRow) => {
    const edges = rowEdges(phase, row);
    const origins = rowOrigins(phase, row);
    const marker = `pt-origin-arrow-${phase.phase.n}-${row.layer}`;
    const litNow = lit();
    // Node y values are phase-local; row.top is the whole plan's offset, which
    // grows with every phase above and sent edges far outside their phase
    // (owner's screenshot, 2026-11-07). The row's own phase-local top anchors them.
    const localTop = row.nodes[0]?.y ?? 0;
    return (
      <div
        class={`pt-layer${row.end ? " pt-row-end" : ""}`}
        style={{ width: `${Math.max(phase.width, 320)}px`, height: `${CARD_H}px` }}
      >
        <svg class="pt-edges" width={Math.max(phase.width, 320)} height={ROW_H} aria-hidden="true">
          <defs>
            <marker id={marker} viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
              <path d="M 0 0 L 8 4 L 0 8" class="pt-origin-head" />
            </marker>
          </defs>
          <For each={edges}>
            {(entry) => (
              <path
                d={edgePath(entry.from, entry.to, localTop)}
                class={`pt-edge ${edgeState(entry.edge, litNow)}`}
              />
            )}
          </For>
          <Show when={props.editing}>
            <For each={edges.filter((entry) => props.editing!.canLink(entry.edge.to))}>
              {(entry) => (
                <path
                  d={edgePath(entry.from, entry.to, localTop)}
                  class="pt-edge-hit"
                  data-edge={`${entry.edge.from}-${entry.edge.to}`}
                  onClick={() => props.editing!.unlink(entry.edge.from, entry.edge.to)}
                ><title>{tr("Remove the dependency of #%1 on #%2", entry.edge.to, entry.edge.from)}</title></path>
              )}
            </For>
          </Show>
          <For each={origins}>
            {(entry) => (
              <path
                d={edgePath(entry.from, entry.to, localTop)}
                class="pt-origin-edge"
                marker-end={`url(#${marker})`}
                data-origin={entry.origin}
              />
            )}
          </For>
        </svg>
        <For each={row.nodes.filter((placed) => includeCancelled() || placed.node.status !== "cancelled")}>{(placed) => card(placed, phase)}</For>
      </div>
    );
  };

  const detail = () => {
    const task = selectedTask();
    if (!task) return null;
    const phase = phaseOf(task.id);
    const blocks = props.team.tasks.filter((candidate) => candidate.deps.some((dep) => dep.task === task.id));
    return (
      <aside class="pt-detail" aria-label={tr("Task %1: %2", task.id, task.title)}>
        <div class="pt-detail-head">
          <span class={`pt-dot pt-dot-${tone(task.status)}`} />
          <span class="pt-id">#{task.id}</span>
          <b>{task.title}</b>
          <span class="pt-grow" />
          <button type="button" class="icon-btn small" aria-label={tr("Close task")} onClick={() => setSelected(null)}>
            <Icon name="x" size={14} />
          </button>
        </div>
        <div class="pt-detail-meta">
          <span class="pt-chip">{short(task.status || "pending", 24)}</span>
          <span class="pt-chip">{tr("Phase %1", phase?.phase.n ?? task.phase)}</span>
          <span class="pt-chip">{task.assignee || tr("Unassigned")}</span>
          <Show when={task.origin}>{(origin) => (
            <button type="button" class="pt-chip pt-chip-origin" data-origin={origin()} title={tr("Open task %1", origin())}
              onClick={() => open(origin())}>{tr("Spawned from #%1", origin())}</button>
          )}</Show>
          <Show when={task.estimate_minutes > 0}>
            <span class="pt-chip">{tr("%1 min", task.estimate_minutes)}</span>
          </Show>
        </div>
        <Show when={taskHeat(heatProfile(), task.id)}>
          {(heat) => (
            <div class="pt-detail-section pt-detail-heat">
              <span class="pt-detail-label">{tr("Lateness")}</span>
              <HeatLine heat={heat()} />
              <For each={heat().tip.split("\n")}>{(line) => <p class="pt-detail-p">{line}</p>}</For>
            </div>
          )}
        </Show>
        <Show when={task.waits}>
          <p class="pt-detail-note">{task.waits}</p>
        </Show>
        <Show when={task.deps.length > 0}>
          <div class="pt-detail-section">
            <span class="pt-detail-label">{tr("Depends on")}</span>
            <div class="pt-detail-links">
              <For each={task.deps}>
                {(dep) => (
                  <button type="button" class="pt-link" onClick={() => open(dep.task)}>
                    #{dep.task}
                    <Show when={!dep.hard}> {tr("soft")}</Show>
                  </button>
                )}
              </For>
            </div>
          </div>
        </Show>
        <Show when={blocks.length > 0}>
          <div class="pt-detail-section">
            <span class="pt-detail-label">{tr("Blocks")}</span>
            <div class="pt-detail-links">
              <For each={blocks}>{(blocked) => <button type="button" class="pt-link" onClick={() => open(blocked.id)}>#{blocked.id}</button>}</For>
            </div>
          </div>
        </Show>
        <Show when={externalBlocks(phase?.phase.n ?? task.phase, task.id, props.team.tasks).length > 0}>
          <div class="pt-detail-section">
            <span class="pt-detail-label">{tr("Other phases wait")}</span>
            <div class="pt-detail-links">
              <For each={externalBlocks(phase?.phase.n ?? task.phase, task.id, props.team.tasks)}>
                {(id) => <button type="button" class="pt-link" onClick={() => open(id)}>#{id}</button>}
              </For>
            </div>
          </div>
        </Show>
        <Show when={task.output.length > 0}>
          <div class="pt-detail-section">
            <span class="pt-detail-label">{tr("Report")}</span>
            <For each={task.output.slice(-3)}>{(line) => <p class="pt-detail-p">{short(line, 400)}</p>}</For>
          </div>
        </Show>
        <Show when={task.decided_by}>
          <p class="pt-detail-note">{tr("Decided by %1", task.decided_by)}</p>
        </Show>
        <Show when={props.taskActions}>{(actions) => actions()(task.id)}</Show>
      </aside>
    );
  };

  return (
    <section class={`pt-plan${props.class ? ` ${props.class}` : ""}`} aria-label={tr("Plan")}>
      <header class="pt-plan-bar">
        <PlanModeBar
          mode={mode()}
          includeCancelled={includeCancelled()}
          onMode={setMode}
          onCancelled={setIncludeCancelled}
          heat={heatOn()}
          onHeat={switchHeat}
        />
        <span class="pt-grow" />
        <span class="pt-legend" aria-label={tr("Link legend")}>
          <span class="pt-legend-item"><i class="pt-legend-dep" />{tr("Depends on")}</span>
          <span class="pt-legend-item"><i class="pt-legend-origin" />{tr("Spawned from")}</span>
        </span>
        <span class="pt-plan-summary">
          <span>{tr("Phases: %1", phases().length)}</span>
          <span>{tr("Tasks: %1", props.team.tasks.length)}</span>
        </span>
        <Show when={mode() === "graph"}>
          <button type="button" class="btn ghost small" onClick={collapseAll}>{tr("Collapse all")}</button>
          <button type="button" class="btn ghost small" onClick={expandAll}>{tr("Expand all")}</button>
        </Show>
      </header>
      <Show when={heatView()}>
        <HeatLegend profile={props.profile} />
      </Show>
      <Show when={mode() === "graph"}>
        <div class="pt-plan-body">
          <div class="pt-plan-scroll" ref={scroller} onPointerLeave={() => setHover(null)}>
            <div class="pt-plan-canvas" style={{ height: `${virtualizer.getTotalSize()}px` }}>
              <For each={virtualRows()}>
                {(item) => (
                  // Task 196: solid-virtual keeps an item object per index, so
                  // the row at that index is read reactively: after Collapse
                  // all the index holds another row (a header, not a layer)
                  // and the old cards must not stay on screen.
                  // The row's text follows each poll; the phase's actions are
                  // made once per row, so an open menu or dialog survives it.
                  <Show when={rows()[item.index]?.key} keyed>
                    {(key) => {
                      const row = () => rowByKey().get(key) ?? rows()[item.index];
                      const first = row();
                      const actions = first?.kind === "header" ? props.phaseActions?.(first.phase.phase.n) : undefined;
                      return (
                        <Show when={row()}>
                          {(current) => (
                            <div
                              class={`pt-row pt-row-${current().kind}`}
                              style={{ top: `${item.start}px`, height: `${item.size}px` }}
                              data-phase={current().phase.phase.n}
                            >
                              {current().kind === "header" ? header(current().phase, current(), actions) : layer(current().phase, current())}
                            </div>
                          )}
                        </Show>
                      );
                    }}
                  </Show>
                )}
              </For>
            </div>
          </div>
          {detail()}
        </div>
      </Show>
      <Show when={mode() === "board"}>
        <BoardView phases={phases()} includeCancelled={includeCancelled()} profile={heatProfile()} onOpen={open} />
      </Show>
      <Show when={mode() === "layers"}>
        <LayersView phases={phases()} includeCancelled={includeCancelled()} profile={heatProfile()} onOpen={open} />
      </Show>
      <Show when={mode() === "profile"}>
        <div class="pm-profile-scroll">
          <Show when={props.loadTimeline}>{(load) => (
            <ProfileTime team={props.team} load={load()} profile={props.profile} statusName={runStatusLabel} onOpen={open} />
          )}</Show>
          <ProfilePanel profile={props.profile} />
        </div>
      </Show>
      <Show when={mode() === "tests"}>
        <TestsPanel tests={props.tests} onMore={() => props.onLoadTests?.(props.tests?.next ?? null)} />
      </Show>
    </section>
  );
}
