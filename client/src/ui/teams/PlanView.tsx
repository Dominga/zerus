import { For, Show, createEffect, createMemo, createSignal, type JSX } from "solid-js";
import { createVirtualizer } from "@tanstack/solid-virtual";
import { Icon } from "../lib/icons";
import { tr } from "../../i18n";
import type { PlanGraph, TeamView } from "./types";
import type { TeamProfileView, TeamTestsView } from "../../types";
import { BoardView, LayersView, PlanModeBar, ProfilePanel, TestsPanel, stageLabel } from "./plan-panels";
import { stageChips, heatShare, type PlanMode } from "./planModes";
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
  type PlacedNode,
  type PlanPhase,
  type PlanRow,
} from "./plan";
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
  if (kind === "done") return tr("Done");
  if (kind === "running") return tr("Running");
  if (kind === "approved") return tr("Approved");
  if (kind === "draft") return tr("Draft");
  if (kind === "paused") return tr("Paused");
  return tr("Open");
}

export function PlanView(props: PlanViewProps): JSX.Element {
  const [mode, setMode] = createSignal<PlanMode>("graph");
  const [includeCancelled, setIncludeCancelled] = createSignal(false);
  createEffect(() => {
    if (mode() === "tests") props.onLoadTests?.(null);
    if (mode() === "profile") props.onLoadProfile?.();
  });
  const [collapsed, setCollapsed] = createSignal<Set<number>>(
    new Set(props.team.phases.filter((phase) => phase.status === "done").map((phase) => phase.n)),
  );
  const [hover, setHover] = createSignal<number | null>(null);
  const [selected, setSelected] = createSignal<number | null>(null);
  const phases = createMemo(() => phasePlans(props.team, props.graph));
  const rows = createMemo(() => buildRows(phases(), collapsed()));
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

  const header = (phase: PlanPhase, row: PlanRow) => {
    const status = phaseStatus(phase.phase);
    const total = phase.phase.total || phase.tasks.length;
    const done = Math.min(total, phase.phase.done || phase.tasks.filter((task) => tone(task.status) === "done").length);
    const isCollapsed = collapsed().has(phase.phase.n);
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
        <Show when={phaseNeedsApproval(phase.phase)}>
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
    const stages = stageChips(task?.pipeline?.stages);
    const heat = heatShare(props.profile, placed.node.id);
    const litNow = lit();
    if (litNow) classes.push(litNow.has(placed.node.id) ? (hover() === placed.node.id ? "pt-card-on" : "pt-card-lit") : "pt-card-dim");
    if (task?.pinned) classes.push("pt-card-pinned");
    return (
      <button
        type="button"
        class={classes.join(" ")}
        style={{ left: `${placed.x}px`, top: "0px", "--heat": String(heat ?? 0) }}
        data-task={placed.node.id}
        data-lit={litNow ? (litNow.has(placed.node.id) ? "lit" : "dim") : "none"}
        aria-label={tr("Task %1: %2", placed.node.id, placed.node.title || task?.title || "")}
        onPointerEnter={() => setHover(placed.node.id)}
        onFocus={() => setHover(placed.node.id)}
        onBlur={() => setHover(null)}
        onClick={() => open(placed.node.id)}
      >
        <span class="pt-card-l1">
          <span class={`pt-dot pt-dot-${state}`} />
          <span class="pt-id">#{placed.node.id}</span>
          <span class="pt-title">{placed.node.title || task?.title || tr("Task %1", placed.node.id)}</span>
          <Show when={task?.pinned}>
            <span class="pt-badge">{tr("Pinned")}</span>
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
    const litNow = lit();
    return (
      <div
        class={`pt-layer${row.end ? " pt-row-end" : ""}`}
        style={{ width: `${Math.max(phase.width, 320)}px`, height: `${CARD_H}px` }}
      >
        <svg class="pt-edges" width={Math.max(phase.width, 320)} height={ROW_H} aria-hidden="true">
          <For each={edges}>
            {(entry) => (
              <path
                d={edgePath(entry.from, entry.to, row.top)}
                class={`pt-edge ${edgeState(entry.edge, litNow)}`}
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
          <Show when={task.estimate_minutes > 0}>
            <span class="pt-chip">{tr("%1 min", task.estimate_minutes)}</span>
          </Show>
        </div>
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
        />
        <span class="pt-grow" />
        <span class="pt-plan-summary">
          <span>{tr("Phases: %1", phases().length)}</span>
          <span>{tr("Tasks: %1", props.team.tasks.length)}</span>
        </span>
        <Show when={mode() === "graph"}>
          <button type="button" class="btn ghost small" onClick={collapseAll}>{tr("Collapse all")}</button>
          <button type="button" class="btn ghost small" onClick={expandAll}>{tr("Expand all")}</button>
        </Show>
      </header>
      <Show when={mode() === "graph"}>
        <div class="pt-plan-body">
          <div class="pt-plan-scroll" ref={scroller} onPointerLeave={() => setHover(null)}>
            <div class="pt-plan-canvas" style={{ height: `${virtualizer.getTotalSize()}px` }}>
              <For each={virtualRows()}>
                {(item) => {
                  const row = rows()[item.index];
                  if (!row) return null;
                  return (
                    <div
                      class={`pt-row pt-row-${row.kind}`}
                      style={{ top: `${item.start}px`, height: `${item.size}px` }}
                      data-phase={row.phase.phase.n}
                    >
                      {row.kind === "header" ? header(row.phase, row) : layer(row.phase, row)}
                    </div>
                  );
                }}
              </For>
            </div>
          </div>
          {detail()}
        </div>
      </Show>
      <Show when={mode() === "board"}>
        <BoardView phases={phases()} includeCancelled={includeCancelled()} profile={props.profile} onOpen={open} />
      </Show>
      <Show when={mode() === "layers"}>
        <LayersView phases={phases()} includeCancelled={includeCancelled()} profile={props.profile} onOpen={open} />
      </Show>
      <Show when={mode() === "profile"}>
        <ProfilePanel profile={props.profile} />
      </Show>
      <Show when={mode() === "tests"}>
        <TestsPanel tests={props.tests} onMore={() => props.onLoadTests?.(props.tests?.next ?? null)} />
      </Show>
    </section>
  );
}
