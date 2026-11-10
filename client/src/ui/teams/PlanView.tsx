import { For, Index, Show, createEffect, createMemo, createSignal, onCleanup, onMount, type JSX } from "solid-js";
import { createVirtualizer } from "@tanstack/solid-virtual";
import { Icon, ZerusMark } from "../lib/icons";
import { Dropdown } from "../lib/Dropdown";
import { tr } from "../../i18n";
import type { PlanGraph, TeamView } from "./types";
import type { TeamBuildsView, TeamProfileView, TeamTaskView, TeamTestsView, TeamTimelineView } from "../../types";
import { BuildsPanel, HeatLegend, HeatLine, PlanModeBar, TestsPanel, runStatusLabel, stageLabel } from "./plan-panels";
import { ProfilePage } from "./profile-page";
import { LanesView, type LaneEdit } from "./LanesView";
import { spawnedFrom, heatModeSeed, stageChips, type PlanMode } from "./planModes";
import { settings, settingsReady } from "../../store/settings";
import { heatOf, taskHeat, type HeatMode, type TaskHeat } from "./heat";
import { chain, externalBlocks, externalDeps, keptPlans, phaseNeedsApproval, phasePlans, phaseStatus, type PlanPhase } from "./plan";
import { NODE_H, NODE_W, orthoPath, placePhase, routeEdges, type PhaseLayout, type Route } from "./planLayout";
import {
  activityText, assigneeAgent, assigneeRole, boardColumn, boardKeys, boardTitle, matches, phaseFinished, statusIcon, taskState, taskTime,
  type BoardKey, type PlanFilter, type TaskProblem,
} from "./planTasks";
import { AgentBadge } from "../sessions/SessionCard";
import { agentKind } from "../../store/catalog";
import { roleStyle } from "./roles";
import { stageName } from "./actions";
import { phaseHeader } from "./phaseHeader";
import "../styles/plan.css";

export interface PlanViewProps {
  team: TeamView;
  graph: PlanGraph;
  /** Opens the task in the host's inspector; without it the plan shows its own card. */
  onOpenTask?: (id: number) => void;
  onOpenReport?: (id: number) => void;
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
  /** Task #415: a No report task's kept last reply, shown on its board card. */
  noReportReply?: (task: number) => string;
  /** Task #415: the board card's "Use last reply as report". */
  onUseLastReply?: (task: TeamTaskView) => void;
  /** Task #58: a phase header's mode chip and switch (ui/teams/TeamActions.tsx PhaseModeControl). */
  phaseActions?: (n: number) => JSX.Element;
  /** Task 197 (P2): the tasks with a problem and its reason (Qt problemTasks/problemTexts). */
  problems?: ReadonlyMap<number, TaskProblem>;
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
  /** Task 199: a task the side column opens (a queue row, Needs your answer); each new object opens it again. */
  openTask?: { task: number } | null;
  /** Task 199: the Tests mode's Builds tab and Force run (TestsPanel). */
  builds?: TeamBuildsView | null;
  buildsError?: string;
  onLoadBuilds?: () => void;
  onForceTest?: (suite: string) => Promise<string>;
  /** Task 197: whether the task's member works on it now (its session is busy). */
  working?: (task: TeamTaskView) => boolean;
  /** Task 197 (P8): only one member's tasks; empty for every member. */
  member?: string;
  onMember?: (member: string) => void;
  /** Task 197 (P6): the team's Auto-accept drafts switch. */
  onAutoAccept?: (on: boolean) => void;
  /** The clock (seconds) the time lines read; tests set it. */
  now?: () => number;
  /** The mode it opens in (Graph by default). */
  initialMode?: PlanMode;
  /** Task 342: the tests open it with cancelled tasks shown or finished ones hidden. */
  initialShowCancelled?: boolean;
  initialHideFinished?: boolean;
  /** Task #226: the Lanes mode's rename and move go through edit-draft; false on a refusal. */
  onLaneEdit?: (phase: number, updates: LaneEdit[]) => Promise<boolean>;
  class?: string;
}

const HEADER_H = 44;
const COLLAPSED_H = 44;
const SCROLLBAR = 12;

/** The card tone class: task 342 (owner, 2026-11-08) gives the plan's five task
 * states a colour each - done green, in work a light blue, in review a deeper
 * blue, not started a pale grey and cancelled a grey hatch - while a problem or
 * a wait keeps its amber/red. */
function toneClass(tone: string): string {
  if (tone === "active") return "work";
  if (tone === "review") return "review";
  if (tone === "done") return "done";
  if (tone === "danger") return "danger";
  if (tone === "warn") return "warn";
  if (tone === "cancelled") return "cancelled";
  return "notstarted";
}

function short(value: string, limit = 150): string {
  return value.length > limit ? `${value.slice(0, limit - 1)}…` : value;
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

function stored(key: string): string | null {
  try { return localStorage.getItem(key); } catch { return null; }
}

function store(key: string, value: string | null) {
  try { if (value === null) localStorage.removeItem(key); else localStorage.setItem(key, value); } catch { /* storage unavailable */ }
}

// Task 197 (P3): a phase scrolled sideways keeps its place through every
// render and mode switch (Qt keepScroll "phasegraph/<team>/<phase>").
const scrolls = new Map<string, number>();

/** A horizontally scrolling phase body that keeps its scroll position. */
function PhaseScroll(props: { key: string; class: string; children: JSX.Element; style?: JSX.CSSProperties }): JSX.Element {
  let element!: HTMLDivElement;
  onMount(() => {
    const left = scrolls.get(props.key);
    if (left) element.scrollLeft = left;
  });
  return (
    <div ref={element} class={props.class} style={props.style} data-scroll={props.key}
      onScroll={() => scrolls.set(props.key, element.scrollLeft)}>
      {props.children}
    </div>
  );
}

/**
 * Task 428: a card's hover follows the pointer. It lights a card while the
 * pointer is over it (or while the keyboard focuses it) and ends when the
 * pointer leaves that card, the board or the window; a click's focus never
 * keeps it. The selection is separate (pt-card-selected).
 */
export const planHover = {
  enter: (id: number): number | null => id,
  leave: (current: number | null, id: number): number | null => (current === id ? null : current),
  focus: (current: number | null, id: number, keyboard: boolean): number | null => (keyboard ? id : current),
};

/** Task 428: a click on the board's empty space (not a card, a header or a control) clears the selection. */
export const clearsSelection = (target: Element | null) =>
  !!target && !target.closest(".pt-card, .pt-phase, .pt-free-accepted, .pt-detail, button, a, input, select, textarea, [role='button']");


/** Task #16: two lists of the same objects in the same order. */
function sameItems<T>(a: readonly T[], b: readonly T[]): boolean {
  return a.length === b.length && a.every((item, index) => item === b[index]);
}

export function PlanView(props: PlanViewProps): JSX.Element {
  const now = () => props.now?.() ?? Date.now() / 1000;
  const [mode, setMode] = createSignal<PlanMode>(props.initialMode ?? "graph");
  const [includeCancelled, setIncludeCancelled] = createSignal(props.initialShowCancelled ?? false);
  const [filter, setFilter] = createSignal<PlanFilter>("all");
  const [ownMember, setOwnMember] = createSignal("");
  const member = () => props.member ?? ownMember();
  const chooseMember = (id: string) => (props.onMember ? props.onMember(id) : setOwnMember(id));
  const hideKey = "zerus.teams.hideFinished";
  const [hideFinished, setHideFinished] = createSignal(props.initialHideFinished ?? stored(hideKey) === "1");
  const switchHide = (on: boolean) => { setHideFinished(on); store(hideKey, on ? "1" : null); };
  const [zoom, setZoom] = createSignal(1);
  // Task 197 (P6): Off, Lateness or Tokens, remembered per team in this
  // browser; Qt's imported per-team mode starts it once.
  const heatKey = () => `zerus.teams.heat.${props.team.id}`;
  const [heatMode, setHeatMode] = createSignal<HeatMode>(heatModeSeed(props.team.id, stored(heatKey()), "{}"));
  const switchHeat = (next: HeatMode) => { setHeatMode(next); store(heatKey(), next === "off" ? "off" : next); };
  onMount(() => {
    void settingsReady.then(() => {
      if (stored(heatKey()) === null) {
        const seeded = heatModeSeed(props.team.id, null, String(settings["teams/heatModes"] ?? "{}"));
        if (seeded !== "off") { setHeatMode(seeded); store(heatKey(), seeded); }
      }
    });
  });
  const heatView = () => heatMode() !== "off" && (mode() === "graph" || mode() === "board" || mode() === "layers");
  const heatProfile = () => (heatView() ? props.profile : null);
  const heat = (id: number): TaskHeat | null => (heatView() ? heatOf(heatMode(), props.profile, id) : null);
  createEffect(() => {
    if (mode() === "tests") props.onLoadTests?.(null);
    // Task #462: the Builds mode is its own tab and reads the broker when opened.
    if (mode() === "builds") props.onLoadBuilds?.();
    if (mode() === "profile" || heatView()) props.onLoadProfile?.();
  });
  const [collapsed, setCollapsed] = createSignal<Set<number>>(
    new Set(props.team.phases.filter((phase) => phase.status === "done").map((phase) => phase.n)),
  );
  const [hover, setHover] = createSignal<number | null>(null);
  // Task 428: leaving the window ends the hover too.
  onMount(() => {
    const clear = () => setHover(null);
    window.addEventListener("blur", clear);
    document.documentElement.addEventListener("pointerleave", clear);
    onCleanup(() => { window.removeEventListener("blur", clear); document.documentElement.removeEventListener("pointerleave", clear); });
  });
  // Task 342: the tests open a task on mount; otherwise a click selects it.
  const [selected, setSelected] = createSignal<number | null>(props.openTask?.task ?? null);
  createEffect(() => {
    const request = props.openTask;
    if (!request) return;
    setMode("graph");
    setSelected(request.task);
  });
  const problems = () => props.problems ?? new Map<number, TaskProblem>();
  const planning = () => props.team.status === "planning";
  const byId = createMemo(() => new Map(props.team.tasks.map((task) => [task.id, task])));
  const visibleTask = (task: TeamTaskView | undefined) =>
    !!task && (includeCancelled() || task.status !== "cancelled") && !(hideFinished() && task.status === "accepted");
  // Task #16 (team ede724c4): a phase whose tasks, nodes and links did not
  // change keeps its plan object, so a task's status change rebuilds no phase
  // row in any view (the <For>s are keyed by these objects).
  let keptPhases = new Map<number, PlanPhase>();
  const allPhases = createMemo(() => {
    const plans = keptPlans(keptPhases, phasePlans(props.team, props.graph));
    keptPhases = new Map(plans.map((plan) => [plan.n, plan]));
    return plans;
  });
  // U58 and owner 2026-10-30: cancelled tasks and the phases they emptied
  // stay out unless shown; Hide finished leaves out every decided phase.
  const phases = createMemo(() => allPhases().filter((phase) => {
    const tasks = phase.tasks.filter((task) => visibleTask(task));
    if (phase.tasks.length > 0 && tasks.length === 0) return false;
    return !(hideFinished() && phaseFinished(phase.tasks));
  }), undefined, { equals: sameItems });
  const finishedCount = createMemo(() => allPhases().filter((phase) => phaseFinished(phase.tasks)).length);
  // Task 316 (F6): the open free phase folds its accepted tasks into one row
  // (Qt's freeAcceptedFold), so the graph draws only the work still open.
  const [unfoldedAccepted, setUnfoldedAccepted] = createSignal<Set<string>>(new Set());
  const acceptedKey = (n: number) => `${props.team.id}/${n}`;
  const acceptedTasks = (phase: PlanPhase) => phase.tasks.filter((task) => visibleTask(task) && task.status === "accepted");
  const acceptedFolded = (phase: PlanPhase) => props.team.free_phase === phase.n && !unfoldedAccepted().has(acceptedKey(phase.n));
  const toggleAccepted = (n: number) => {
    const key = acceptedKey(n);
    const next = new Set(unfoldedAccepted());
    if (next.has(key)) next.delete(key);
    else next.add(key);
    setUnfoldedAccepted(next);
  };
  // Task #16: a phase's layout is kept while its visible nodes and links are
  // the same; a status change that hides or folds no card keeps it.
  type LayoutEntry = { layout: PhaseLayout; routes: Route[]; originRoutes: Route[]; edges: PlanPhase["edges"] };
  type KeptLayout = { phase: PlanPhase; nodes: PlanPhase["nodes"]; origins: { from: number; to: number }[]; entry: LayoutEntry };
  let keptLayouts = new Map<number, KeptLayout>();
  const layouts = createMemo(() => {
    const next = new Map<number, KeptLayout>();
    const shown = new Map(phases().map((phase) => {
      const folded = acceptedFolded(phase);
      const nodes = phase.nodes.filter((placed) => visibleTask(placed.task) && !(folded && placed.task?.status === "accepted"));
      const origins = nodes
        .map((placed) => ({ from: spawnedFrom(placed.node, placed.task) ?? 0, to: placed.node.id }))
        .filter((link) => link.from > 0 && link.from !== link.to);
      const kept = keptLayouts.get(phase.n);
      // The same phase plan with the same visible nodes and spawn links: its
      // layout stands (the links follow the nodes and the plan).
      let entry = kept && kept.phase === phase && sameItems(kept.nodes, nodes)
        && kept.origins.length === origins.length && kept.origins.every((link, index) => link.from === origins[index].from && link.to === origins[index].to)
        ? kept.entry : undefined;
      if (!entry) {
        const ids = new Set(nodes.map((placed) => placed.node.id));
        const edges = phase.edges.filter((edge) => ids.has(edge.from) && ids.has(edge.to));
        const layout = placePhase(nodes.map((placed) => ({ id: placed.node.id, layer: placed.node.layer })), edges, 320);
        entry = { layout, routes: routeEdges(layout, edges), originRoutes: routeEdges(layout, origins.filter((link) => ids.has(link.from))), edges };
      }
      next.set(phase.n, { phase, nodes, origins, entry });
      return [phase.n, entry] as const;
    }));
    keptLayouts = next;
    return shown;
  }, undefined, { equals: (a, b) => a.size === b.size && [...a].every(([n, entry]) => b.get(n) === entry) });
  /** Task #16: what a task card shows from the task and the view's state. A
   * card is rebuilt when this changes (its status, attempt, problem, the
   * filter match, the selection, its heat), and only that card. */
  const cardKey = (task: TeamTaskView) => {
    const problem = problems().get(task.id);
    const pipeline = task.pipeline_run as { open?: { kind?: string } } | undefined;
    return ["card", task.status, task.attempt_state, task.attempts, task.decided_by, task.no_report ? 1 : 0, task.severity ?? "",
      problem?.tone ?? "", problem?.text ?? "", matchesTask(task) ? 1 : 0, selected() === task.id ? 1 : 0,
      heat(task.id)?.tint ?? "", pipeline?.open?.kind ?? ""].join("|");
  };
  const selectedTask = createMemo(() => props.team.tasks.find((task) => task.id === selected()));
  const lit = createMemo(() => (hover() === null ? null : chain(props.team.tasks, hover()!)));
  const matchesTask = (task: TeamTaskView | undefined) => !!task && matches(task, filter(), problems(), member(), planning());
  const working = (task: TeamTaskView) => props.working?.(task) ?? false;
  const memberName = () => {
    const found = props.team.members.find((candidate) => candidate.id === member());
    return found?.role || member();
  };

  const isCollapsed = (n: number) => collapsed().has(n);
  const toggle = (n: number) => {
    const next = new Set(collapsed());
    if (next.has(n)) next.delete(n);
    else next.add(n);
    setCollapsed(next);
  };
  const collapseAll = () => setCollapsed(new Set<number>(phases().map((phase) => phase.n)));
  const expandAll = () => setCollapsed(new Set<number>());
  const allOpen = () => phases().every((phase) => !collapsed().has(phase.n));
  const open = (id: number) => {
    if (props.onOpenTask) props.onOpenTask(id);
    else setSelected(id);
  };
  const phaseOf = (id: number) => allPhases().find((phase) => phase.tasks.some((task) => task.id === id));

  // The graph windows whole phases: a phase's links never vanish while it
  // scrolls (task 197 P1; the old rows drew one SVG per layer).
  let scroller: HTMLDivElement | undefined;
  const phaseHeight = (phase: PlanPhase) => {
    if (isCollapsed(phase.n)) return COLLAPSED_H + 8;
    const layout = layouts().get(phase.n)?.layout;
    const fold = acceptedTasks(phase).length > 0 && props.team.free_phase === phase.n ? 26 : 0;
    return HEADER_H + fold + (layout ? Math.ceil(layout.height * zoom()) + SCROLLBAR : 0) + 8;
  };
  const virtualizer = createVirtualizer({
    get count() {
      return phases().length;
    },
    getScrollElement: () => (scroller?.isConnected ? scroller : null),
    estimateSize: (index) => (phases()[index] ? phaseHeight(phases()[index]) : HEADER_H),
    getItemKey: (index) => phases()[index]?.n ?? index,
    overscan: 3,
  });
  createEffect(() => {
    // A collapse, a zoom or new tasks change a phase's height.
    collapsed(); zoom(); layouts();
    virtualizer.measure();
  });
  // Before the scroller is laid out (the first paint, a server render) every
  // phase is placed by its own height; the virtualizer windows them after.
  const windowed = createMemo(() => {
    const items = virtualizer.getVirtualItems();
    if (items.length > 0 || phases().length === 0) return items.map((item) => ({ index: item.index, start: item.start }));
    let start = 0;
    return phases().map((phase, index) => {
      const item = { index, start };
      start += phaseHeight(phase);
      return item;
    });
  });
  const fit = () => {
    const widest = Math.max(320, ...[...layouts().values()].map((entry) => entry.layout.width));
    const room = (scroller?.clientWidth ?? widest) - 32;
    setZoom(Math.max(0.4, Math.min(1, room / widest)));
  };
  const stepZoom = (delta: number) => setZoom((value) => Math.max(0.4, Math.min(1.6, Math.round((value + delta) * 10) / 10)));

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

  /** Qt's phase header: title, chips, "Healthy N/M", the progress, and the
   * draft's decision. A collapsed phase is this one line with its tasks' titles. */
  const header = (phase: PlanPhase, collapsible = true, actions?: JSX.Element) => {
    // Task #16 (team ede724c4): what the header shows, in one memo the JSX
    // reads, so a status change updates its counts in place. The element stays
    // (a rebuilt header made the row's :has() rules restyle its whole graph).
    const view = createMemo(() => {
      const record = phase.phase;
      const status = phaseStatus(record);
      // Task 196: Qt's header by state: its notes, the collapsed one-line summary and the lead's note.
      const head = phaseHeader(props.team, record);
      const note = record.note?.trim() ?? "";
      const tasks = phase.tasks.filter((task) => visibleTask(task));
      const total = tasks.length;
      const done = tasks.filter((task) => task.status === "accepted").length;
      const closed = collapsible && isCollapsed(phase.n);
      const healthy = tasks.filter((task) => !problems().has(task.id)).length;
      const running = status.kind === "running" || status.kind === "approved" || status.kind === "open";
      return { record, status, head, note, total, done, closed, healthy, running };
    });
    return (
      <div
        // Task 332: one class string. Solid's class and classList on one element
        // overwrite each other, which dropped pt-phase-closed: a folded card lost
        // its bottom border and its bottom corners.
        class={`pt-phase pt-phase-${view().status.kind}${view().closed ? " pt-phase-closed" : ""}`}
        role={collapsible ? "button" : undefined}
        tabindex={collapsible ? "0" : undefined}
        aria-label={tr("Phase %1", phase.n)}
        aria-expanded={collapsible ? !view().closed : undefined}
        data-phase-head={phase.n}
        data-phase-kind={view().record.kind || undefined}
        onClick={() => { if (collapsible) toggle(phase.n); }}
        onKeyDown={(event) => {
          if (collapsible && (event.key === "Enter" || event.key === " ")) {
            event.preventDefault();
            toggle(phase.n);
          }
        }}
      >
        <Show when={collapsible}><Icon name={view().closed ? "chevron-right" : "chevron-down"} size={14} /></Show>
        <span class="pt-pn">{phase.n}</span>
        <b class="pt-phase-title">{view().record.title || tr("Phase %1", phase.n)}</b>
        <Show when={view().record.archived}><span class="pt-badge">{tr("Archived")}</span></Show>
        <span class={`pt-chip pt-chip-${view().status.kind}`}>{statusLabel(view().status.kind)}</span>
        <Show when={view().record.kind === "minor"}><span class="pt-chip pt-chip-minor" data-phase-kind="minor">{tr("Minor")}</span></Show>
        <Show when={view().record.crit_gate}>{(gate) => <span class="pt-crit-gate" data-crit-gate title={gate()}>{short(gate(), 80)}</span>}</Show>
        <Show when={view().record.auto}><span class="pt-chip pt-chip-auto">{tr("Auto")}</span></Show>
        <Show when={view().record.barrier}><span class="pt-chip pt-chip-barrier">{tr("Barrier")}</span></Show>
        {/* Task 356: no acceptance label. The core's acceptance names who accepts the
            phase (lead or owner), which Qt shows only in the phase menu
            (Requires owner acceptance, checked), as this header's menu does. */}
        <For each={view().head.notes.filter((shown) => shown.id !== "health")}>{(shown) => (
          <span class={`pt-phase-note pt-phase-note-${shown.tone}`} data-note={shown.id} title={shown.tip || shown.text}>{short(shown.text, 80)}</span>
        )}</For>
        <Show when={view().closed && view().head.summary}>
          <span class="pt-phase-summary" data-summary="1" title={view().head.summary.split(", #").join("\n#")}>{short(view().head.summary, 140)}</span>
        </Show>
        {actions ?? props.phaseActions?.(phase.n)}
        <span class="pt-grow" />
        <Show when={view().running && view().total > 0 && view().status.kind !== "done"}>
          <span class="pt-healthy" classList={{ warn: view().healthy < view().total }} data-healthy={`${view().healthy}/${view().total}`} title={view().record.health || undefined}>
            {tr("Healthy %1/%2", view().healthy, view().total)}
          </span>
        </Show>
        <Show when={view().record.health && !view().running}><span class="pt-health">{short(view().record.health, 60)}</span></Show>
        <Show when={view().total > 0}>
          <span class="pt-progress" aria-label={tr("%1 of %2", view().done, view().total)}>
            <For each={Array.from({ length: Math.min(12, view().total) }, (_, index) => index < Math.round((view().done / view().total) * Math.min(12, view().total)))}>
              {(on) => <i class={on ? "on" : ""} />}
            </For>
          </span>
          <span class="pt-count">{view().done}/{view().total}</span>
        </Show>
        <Show when={!view().closed && view().note}>
          <span class="pt-phase-lead-note" data-note="lead" title={view().note}>{tr("Lead's note: %1", short(view().note, 160))}</span>
        </Show>
        <Show when={phaseNeedsApproval(view().record) && !props.phaseActions}>
          <span class="pt-approval">{tr("Needs approval")}</span>
          <Show when={props.onApprovePhase}>
            <button type="button" class="btn small" data-approve={phase.n}
              onClick={(event) => { event.stopPropagation(); props.onApprovePhase?.(phase.n); }}>{tr("Approve")}</button>
          </Show>
          <Show when={props.onDiscardPhase}>
            <button type="button" class="btn ghost small"
              onClick={(event) => { event.stopPropagation(); props.onDiscardPhase?.(phase.n); }}>{tr("Discard")}</button>
          </Show>
        </Show>
      </div>
    );
  };

  /** The second line's end: the problem's reason, else the activity and the
   * time against the estimate (Qt's activity label and taskTime). */
  const timeBits = (task: TeamTaskView) => {
    const problem = problems().get(task.id);
    if (problem) return <span class={`pt-problem pt-problem-${toneClass(problem.tone)}`} data-problem={task.id} title={problem.text}>{problem.text}</span>;
    const activity = activityText(task, working(task));
    const time = taskTime(task, now());
    return (
      <span class="pt-time" data-time={task.id}>
        <Show when={activity}><span class="pt-activity">{activity}</span></Show>
        <Show when={time.text}><span class="pt-worked" classList={{ over: time.over }}>{time.text}</span></Show>
      </span>
    );
  };

  const tone = (task: TeamTaskView) => {
    const problem = problems().get(task.id);
    return toneClass(problem ? problem.tone : taskState(task).tone);
  };

  /** A graph node: Qt's Wide node, two lines. */
  const node = (task: TeamTaskView, phase: PlanPhase, rect: { x: number; y: number }) => {
    const external = externalDeps(phase.n, task, byId());
    const taskHeatNow = heat(task.id);
    const litNow = lit();
    const problem = problems().get(task.id);
    const classes = ["pt-card", `pt-card-${tone(task)}`];
    if (task.severity === "crit") classes.push("pt-card-crit");
    if (task.severity === "minor") classes.push("pt-card-minor");
    if (litNow) classes.push(litNow.has(task.id) ? (hover() === task.id ? "pt-card-on" : "pt-card-lit") : "pt-card-dim");
    if (!matchesTask(task)) classes.push("pt-card-off");
    if (problem) classes.push("pt-card-problem");
    if (task.status === "draft") classes.push("pt-card-draft");
    if (selected() === task.id) classes.push("pt-card-selected");
    const stages = stageChips(task.pipeline_run);
    return (
      <button
        type="button"
        class={classes.join(" ")}
        style={{ left: `${rect.x}px`, top: `${rect.y}px`, width: `${NODE_W}px`, height: `${NODE_H}px`, "--heat": String(taskHeatNow?.tint ?? 0) }}
        title={[`#${task.id} ${task.title}`, problem?.text ?? taskState(task).text, taskHeatNow?.tip ?? ""].filter(Boolean).join("\n")}
        data-task={task.id}
        data-lit={litNow ? (litNow.has(task.id) ? "lit" : "dim") : "none"}
        aria-label={tr("Task %1: %2", task.id, task.title)}
        aria-pressed={selected() === task.id}
        onPointerEnter={() => setHover(planHover.enter(task.id))}
        onPointerLeave={() => setHover(planHover.leave(hover(), task.id))}
        onFocus={(event) => setHover(planHover.focus(hover(), task.id, event.currentTarget.matches(":focus-visible")))}
        onBlur={() => setHover(planHover.leave(hover(), task.id))}
        onClick={() => (props.editing?.editTask ? props.editing.editTask(task.id) : open(task.id))}
      >
        <Show when={props.editing?.canLink(task.id) || (props.editing && task.status !== "cancelled" && props.editing.editTask)}>
          <span
            class="pt-link-handle"
            role="presentation"
            title={tr("Drag from a node's right edge onto a task to connect them")}
            onClick={(event) => event.stopPropagation()}
            onPointerDown={(event) => startLink(event, task.id)}
          />
        </Show>
        <span class="pt-card-l1">
          <span class="pt-state"><StateGlyph task={task} problem={!!problem} size={12} /></span>
          <span class="pt-id">#{task.id}</span>
          <Show when={task.severity === "crit"}><span class="pt-chip pt-chip-crit" data-severity="crit" title={task.origin ? tr("Critical follow-up of #%1", task.origin) : tr("Critical follow-up")}>Crit{task.origin ? ` #${task.origin}` : ""}</span></Show>
          <Show when={task.severity === "minor"}><span class="pt-chip pt-chip-minor" data-severity="minor">Minor</span></Show>
          <span class="pt-title">{task.title || tr("Task %1", task.id)}</span>
          <Show when={task.pipeline_run?.open}>{(stage) => <span class="pt-badge" data-stage={stage().kind}>{stageName(stage().kind)}</span>}</Show>
          <Show when={external.length > 0}>
            <span class="pt-deps" title={`${tr("Depends on tasks in other phases")}\n${external.map((id) => `#${id}`).join(" ")}`}>
              ↤{external.slice(0, 2).map((id) => `#${id}`).join(" ")}{external.length > 2 ? ` +${external.length - 2}` : ""}
            </span>
          </Show>
        </span>
        <span class="pt-card-l2">
          <span class="pt-role role-badge" style={roleStyle(assigneeRole(props.team, task))}>{assigneeRole(props.team, task)}</span>
          <Show when={assigneeAgent(props.team, task)}><AgentBadge agent={agentKind(assigneeAgent(props.team, task))} narrow /></Show>
          <Show when={taskHeatNow} fallback={timeBits(task)}>
            <HeatLine heat={taskHeatNow} compact />
          </Show>
        </span>
        <Show when={stages.length > 0}>
          <span class="pt-stages">
            <For each={stages}>{(stage) => <span class={`pt-stage pt-stage-${stage.state}`}>{stageLabel(stage.kind)}</span>}</For>
          </span>
        </Show>
      </button>
    );
  };

  const routeClass = (route: Route, litNow: ReadonlySet<number> | null, crit: ReadonlySet<number>) => {
    const state = !litNow ? "" : litNow.has(route.from) && litNow.has(route.to) ? "lit" : "dim";
    return `pt-edge ${state}${route.back ? " back" : ""}${crit.has(route.from) || crit.has(route.to) ? " pt-edge-crit" : ""}`;
  };

  /** One expanded phase's graph: one SVG for every link of the phase, its
   * cards over it, scrolled sideways on its own. */
  const graphBody = (phase: PlanPhase, layout: PhaseLayout, routes: Route[], originRoutes: Route[]) => {
    const litNow = lit();
    const crit = new Set(phase.tasks.filter((task) => task.severity === "crit").map((task) => task.id));
    const marker = `pt-origin-arrow-${props.team.id}-${phase.n}`;
    return (
      <PhaseScroll key={`graph/${props.team.id}/${phase.n}`} class="pt-phase-body" style={{ height: `${Math.ceil(layout.height * zoom()) + SCROLLBAR}px` }}>
        <div class="pt-phase-sizer" style={{ width: `${Math.ceil(layout.width * zoom())}px`, height: `${Math.ceil(layout.height * zoom())}px` }}>
          <div class="pt-phase-canvas" style={{ width: `${layout.width}px`, height: `${layout.height}px`, transform: `scale(${zoom()})` }}>
            {/* Qt's band layers: each layer's columns shaded, numbered inside the phase. */}
            <For each={layout.labels}>
              {(label, index) => (
                <>
                  <span class="pt-layer-band" classList={{ odd: index() % 2 === 1 }} style={{ left: `${label.x}px`, width: `${label.w}px` }} />
                  <span class="pt-layer-label" style={{ left: `${label.x}px`, width: `${label.w}px` }} data-layer={label.layer}>
                    <b>{tr("Layer %1", index() + 1)}</b><span>{label.count}</span>
                  </span>
                </>
              )}
            </For>
            <svg class="pt-edges" width={layout.width} height={layout.height} aria-hidden="true" data-edges={phase.n}>
              <defs>
                <marker id={marker} viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                  <path d="M 0 0 L 8 4 L 0 8" class="pt-origin-head" />
                </marker>
                <marker id={`${marker}-dep`} viewBox="0 0 8 8" refX="7" refY="4" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
                  <path d="M 0 0 L 8 4 L 0 8 z" class="pt-edge-head" />
                </marker>
              </defs>
              <For each={routes}>
                {(route) => <path d={orthoPath(route.points)} class={routeClass(route, litNow, crit)} marker-end={`url(#${marker}-dep)`} data-edge={`${route.from}-${route.to}`} />}
              </For>
              <Show when={props.editing}>
                <For each={routes.filter((route) => props.editing!.canLink(route.to))}>
                  {(route) => (
                    <path
                      d={orthoPath(route.points)}
                      class="pt-edge-hit"
                      data-edge={`${route.from}-${route.to}`}
                      onClick={() => props.editing!.unlink(route.from, route.to)}
                    ><title>{tr("Remove the dependency of #%1 on #%2", route.to, route.from)}</title></path>
                  )}
                </For>
              </Show>
              <For each={originRoutes}>
                {(route) => <path d={orthoPath(route.points)} class="pt-origin-edge" marker-end={`url(#${marker})`} data-origin={route.from} />}
              </For>
            </svg>
            <For each={phase.tasks.filter((task) => visibleTask(task) && layout.rects.has(task.id))}>
              {(task) => <Show when={cardKey(task)} keyed>{(_shown) => node(task, phase, layout.rects.get(task.id)!)}</Show>}
            </For>
          </div>
        </div>
      </PhaseScroll>
    );
  };

  /** Task 197 (P7): a Board card. */
  const boardCard = (task: TeamTaskView) => {
    const problem = problems().get(task.id);
    const taskHeatNow = heat(task.id);
    const reply = () => (task.no_report ? props.noReportReply?.(task.id) ?? "" : "");
    const activate = () => open(task.id);
    return (
      <div role="button" tabindex="0" class={`pm-card pm-board-card pt-tone-${tone(task)}`} classList={{ problem: !!problem, draft: task.status === "draft", crit: task.severity === "crit", minor: task.severity === "minor" }}
        style={taskHeatNow ? { "--heat": String(taskHeatNow.tint) } : undefined}
        data-task={task.id} data-severity={task.severity || undefined} data-no-report={task.no_report ? "true" : undefined}
        title={[`#${task.id} ${task.title}`, problem?.text ?? taskState(task).text, taskHeatNow?.tip ?? "", reply()].filter(Boolean).join("\n")}
        onClick={activate} onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); activate(); } }}>
        <span class="pm-card-line">
          <span class="pt-state"><StateGlyph task={task} problem={!!problem} size={13} /></span>
          <span class="pm-id">#{task.id}</span>
          <Show when={task.severity === "crit"}><span class="pt-chip pt-chip-crit" data-severity="crit">Crit{task.origin ? ` #${task.origin}` : ""}</span></Show>
          <Show when={task.severity === "minor"}><span class="pt-chip pt-chip-minor" data-severity="minor">Minor</span></Show>
          <Show when={task.deps.length > 0}><span class="pm-deps">←{task.deps.map((dep) => `#${dep.task}`).join(" ")}</span></Show>
          <b class="pm-title">{task.title}</b>
        </span>
        <span class="pm-card-meta">
          <span class="pt-role role-badge" style={roleStyle(assigneeRole(props.team, task))}>{assigneeRole(props.team, task)}</span>
          <Show when={assigneeAgent(props.team, task)}><AgentBadge agent={agentKind(assigneeAgent(props.team, task))} narrow /></Show>
          {timeBits(task)}
        </span>
        <Show when={taskHeatNow}><HeatLine heat={taskHeatNow} /></Show>
        <Show when={stageChips(task.pipeline_run).length > 0}>
          <span class="pm-stages">
            <For each={stageChips(task.pipeline_run)}>{(stage) => <span class={`pm-stage pm-stage-${stage.state || "waiting"}`}>{stageLabel(stage.kind)}</span>}</For>
          </span>
        </Show>
        <Show when={task.no_report}>
          <span class="pm-card-noreport">
            <Show when={reply()} fallback={<span class="dim">{tr("No report")}</span>}><span class="ell" title={reply()}>{reply()}</span></Show>
            <Show when={props.onUseLastReply}>
              <button type="button" class="btn small primary" data-action="use-last-reply"
                onClick={(event) => { event.stopPropagation(); props.onUseLastReply!(task); }}>{tr("Use last reply as report")}</button>
            </Show>
          </span>
        </Show>
      </div>
    );
  };

  /** Task 197 (P7): one phase's columns, the same in every phase so they
   * line up; the column titles stay pinned while the board scrolls. */
  const boardPhase = (phase: PlanPhase, keys: BoardKey[]) => {
    // Task #16: the columns follow the statuses in their own memos, so a
    // status change moves one card and rebuilds nothing else of the phase.
    const tasks = createMemo(() => phase.tasks.filter((task) => visibleTask(task) && (!member() || task.assignee === member())));
    const column = (key: BoardKey) => tasks().filter((task) => boardColumn(task, problems().has(task.id)) === key);
    return (
      <PhaseScroll key={`board/${props.team.id}/${phase.n}`} class="pm-board-phase">
        <div class="pm-board" style={{ "grid-template-columns": `repeat(${keys.length}, minmax(190px, 1fr))` }}>
          <For each={keys}>
            {(key) => {
              const shown = createMemo(() => column(key));
              return (
                <section class="pm-column" classList={{ empty: shown().length === 0, problems: key === "problems" && shown().length > 0 }} data-column={key} aria-label={boardTitle(key)}>
                  <header class="pm-column-head"><b>{boardTitle(key).toUpperCase()}</b><span class="grow" /><span class="pm-count">{shown().length}</span></header>
                  <div class="pm-column-body">
                    <For each={shown().slice(0, 200)}>{(task) => <Show when={cardKey(task)} keyed>{(_shown) => boardCard(task)}</Show>}</For>
                    <Show when={shown().length === 0}><span class="pm-empty-dash">-</span></Show>
                    <Show when={shown().length > 200}><p class="pm-more">{tr("%1 more not shown", shown().length - 200)}</p></Show>
                  </div>
                </section>
              );
            }}
          </For>
        </div>
      </PhaseScroll>
    );
  };

  /** Task 197 (P8): one phase's tasks as compact rows by layer then id. */
  const layersPhase = (phase: PlanPhase) => {
    // Qt's layer rows: the phase's own layers numbered from L1 (phaseLayers),
    // each row labelled, in layer then id order.
    // Task #16: the rows follow the statuses in their own memos, so a status
    // change rebuilds its one row.
    const all = createMemo(() => phase.nodes.filter((placed) => visibleTask(placed.task)));
    const own = createMemo(() => new Map([...new Set(all().map((placed) => placed.node.layer))].sort((a, b) => a - b).map((layer, index) => [layer, index] as const)));
    const nodes = createMemo(() => all().filter((placed) => matchesTask(placed.task)));
    const layers = createMemo(() => [...new Set(nodes().map((placed) => placed.node.layer))].sort((a, b) => a - b));
    return (
      <div class="pm-layers-phase">
        <For each={layers()}>
          {(layer) => {
            const rows = createMemo(() => nodes().filter((placed) => placed.node.layer === layer).sort((a, b) => a.node.id - b.node.id));
            return (
              <div class="pm-layer-group" data-layer={layer}>
                <For each={rows()}>
                  {(placed) => <Show when={cardKey(placed.task!)} keyed>{(_shown) => {
                    const task = placed.task!;
                    const problem = problems().get(task.id);
                    const time = taskTime(task, now());
                    const activity = activityText(task, working(task));
                    const taskHeatNow = heat(task.id);
                    return (
                      <button type="button" class={`pm-layer-row pt-tone-${tone(task)}`} classList={{ problem: !!problem, crit: task.severity === "crit", minor: task.severity === "minor" }} data-task={task.id}
                        data-severity={task.severity || undefined}
                        style={taskHeatNow ? { "--heat": String(taskHeatNow.tint) } : undefined}
                        title={[problem?.text ?? taskState(task).text, taskHeatNow?.tip ?? ""].filter(Boolean).join("\n")} onClick={() => open(task.id)}>
                        <span class="pm-layer-label">{tr("L%1", (own().get(layer) ?? 0) + 1)}</span>
                        <span class="pt-state"><StateGlyph task={task} problem={!!problem} size={12} /></span>
                        <span class="pm-id">#{task.id}</span>
                        <Show when={task.severity === "crit"}><span class="pt-chip pt-chip-crit" data-severity="crit">Crit{task.origin ? ` #${task.origin}` : ""}</span></Show>
                        <Show when={task.severity === "minor"}><span class="pt-chip pt-chip-minor" data-severity="minor">Minor</span></Show>
                        <Show when={task.deps.length > 0}><span class="pm-deps">←{task.deps.map((dep) => `#${dep.task}`).join(" ")}</span></Show>
                        <span class="pm-layer-title">{task.title}</span>
                        <Show when={problem} fallback={<span class="pm-layer-activity">{activity}</span>}>
                          <span class={`pt-problem pt-problem-${toneClass(problem!.tone)}`}>{problem!.text}</span>
                        </Show>
                        <span class="grow" />
                        <Show when={taskHeatNow}><HeatLine heat={taskHeatNow} compact /></Show>
                        <span class="pm-layer-time" classList={{ over: time.over }}>{time.text}</span>
                        <span class="pt-role role-badge" style={roleStyle(assigneeRole(props.team, task))}>{assigneeRole(props.team, task)}</span>
                        <span class="pm-layer-agent"><Show when={assigneeAgent(props.team, task)}><AgentBadge agent={agentKind(assigneeAgent(props.team, task))} narrow /></Show></span>
                      </button>
                    );
                  }}</Show>}
                </For>
              </div>
            );
          }}
        </For>
        <Show when={layers().length === 0}><p class="pm-empty">{tr("No task matches")}</p></Show>
      </div>
    );
  };

  const detail = () => {
    const task = selectedTask();
    if (!task) return null;
    const phase = phaseOf(task.id);
    const blocks = props.team.tasks.filter((candidate) => candidate.deps.some((dep) => dep.task === task.id));
    return (
      <aside class={`pt-detail pt-detail-${tone(task)}`} data-task-tone={tone(task)} aria-label={tr("Task %1: %2", task.id, task.title)}>
        <div class="pt-detail-head">
          <span class={`pt-dot pt-dot-${tone(task)}`} />
          <span class="pt-id">#{task.id}</span>
          <b>{task.title}</b>
          <span class="pt-grow" />
          <button type="button" class="icon-btn small" aria-label={tr("Close task")} onClick={() => setSelected(null)}>
            <Icon name="x" size={14} />
          </button>
        </div>
        <div class="pt-detail-meta">
          <span class="pt-chip">{taskState(task).text}</span>
          <Show when={task.archived}><span class="pt-chip">{tr("Archived")}</span></Show>
          <span class="pt-chip">{short(task.status || "pending", 24)}</span>
          <span class="pt-chip">{tr("Phase %1", phase?.n ?? task.phase)}</span>
          <span class="pt-chip">{task.assignee || tr("Unassigned")}</span>
          <Show when={task.severity === "crit"}><span class="pt-chip pt-chip-crit" data-severity="crit">{tr("Crit")}{task.origin ? ` ${tr("from #%1", task.origin)}` : ""}</span></Show>
          <Show when={task.severity === "minor"}><span class="pt-chip pt-chip-minor" data-severity="minor">{tr("Minor")}</span></Show>
          <Show when={task.origin}>{(origin) => (
            <button type="button" class="pt-chip pt-chip-origin" data-origin={origin()} title={tr("Open task %1", origin())}
              onClick={() => open(origin())}>{tr("Spawned from #%1", origin())}</button>
          )}</Show>
          <Show when={task.estimate_minutes > 0}>
            <span class="pt-chip">{tr("%1 min", task.estimate_minutes)}</span>
          </Show>
        </div>
        <Show when={problems().get(task.id)}>{(problem) => <p class="pt-detail-note pt-problem">{problem().text}</p>}</Show>
        <Show when={task.archived}>
          <button class="btn small" onClick={() => props.onOpenReport?.(task.id)}>{tr("Open archived report")}</button>
        </Show>
        <Show when={taskHeat(heatProfile(), task.id)}>
          {(found) => (
            <div class="pt-detail-section pt-detail-heat">
              <span class="pt-detail-label">{tr("Lateness")}</span>
              <HeatLine heat={found()} />
              <For each={found().tip.split("\n")}>{(line) => <p class="pt-detail-p">{line}</p>}</For>
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
        <Show when={externalBlocks(phase?.n ?? task.phase, task.id, props.team.tasks).length > 0}>
          <div class="pt-detail-section">
            <span class="pt-detail-label">{tr("Other phases wait")}</span>
            <div class="pt-detail-links">
              <For each={externalBlocks(phase?.n ?? task.phase, task.id, props.team.tasks)}>
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

  // Esc clears the member filter (Qt: "press Esc to show every member again").
  const onKey = (event: KeyboardEvent) => { if (event.key === "Escape" && member()) chooseMember(""); };
  onMount(() => window.addEventListener("keydown", onKey));
  onCleanup(() => window.removeEventListener("keydown", onKey));
  // Task #16: the same columns are the same list, so a status change that
  // keeps them rebuilds no board.
  const boardKeyList = createMemo(() => boardKeys(props.team, problems()), undefined, {
    equals: (a, b) => a.length === b.length && a.every((key, index) => key === b[index]),
  });
  const autoAccept = () => props.team.settings?.auto_accept === true;
  const shownMode = () => mode() === "graph" || mode() === "board" || mode() === "layers";

  return (
    <section class={`pt-plan${props.class ? ` ${props.class}` : ""}`} aria-label={tr("Plan")}>
      <header class="pt-plan-bar">
        <div class="pm-tools" role="toolbar" aria-label={tr("Plan view")}>
          <PlanModeBar mode={mode()} onMode={setMode} />
          <Show when={shownMode() && mode() !== "board" && !planning()}>
            <div class="pm-group" data-plan-group="filter">
              <div class="pm-segment" role="radiogroup" aria-label={tr("Show")} data-filter={filter()}>
                <For each={[["all", tr("All")], ["active", tr("Active")], ["problems", tr("Problems")]] as const}>
                  {([value, label]) => (
                    <button type="button" role="radio" aria-checked={filter() === value} classList={{ on: filter() === value }}
                      data-filter-value={value} onClick={() => setFilter(value)}>{label}</button>
                  )}
                </For>
              </div>
            </div>
          </Show>
          <Show when={member()}>
            <button type="button" class="pm-focus" data-member-filter={member()}
              title={tr("A click on a member's card shows only its tasks. Click here or press Esc to show every member again.")}
              onClick={() => chooseMember("")}>{tr("Only %1's tasks", memberName())}<Icon name="x" size={11} /></button>
          </Show>
          <Show when={mode() !== "profile" && mode() !== "tests" && mode() !== "builds"}>
            <div class="pm-group pm-switches" data-plan-group="switches">
              <button type="button" class="pm-toggle" classList={{ on: includeCancelled() }} aria-pressed={includeCancelled()}
                data-toggle="cancelled" onClick={() => setIncludeCancelled(!includeCancelled())}>
                <Icon name="eye" size={12} />{tr("Show cancelled")}
              </button>
              <Show when={shownMode()}>
                <label class="pm-check pm-heat" title={tr("Tint each task by how late it is against the plan, or by the tokens it lost")}>
                  <span>{tr("Heat")}</span>
                  <Dropdown class="pm-heat-pick" aria-label={tr("Heat")} value={heatMode()} onChange={(next) => switchHeat(next as HeatMode)}
                    options={[{ value: "off", label: tr("Off") }, { value: "time", label: tr("Lateness") }, { value: "tokens", label: tr("Tokens") }]} />
                </label>
                {/* Owner, 2026-11-10: it also hides accepted tasks (task 342), so it shows
                    whenever a task is accepted, not only once a whole phase is decided. */}
                <Show when={finishedCount() > 0 || hideFinished() || props.team.tasks.some((task) => task.status === "accepted")}>
                  <button type="button" class="pm-toggle" classList={{ on: hideFinished() }} aria-pressed={hideFinished()} data-toggle="finished"
                    title={finishedCount() === 0 ? tr("Hide accepted tasks") : finishedCount() === 1 ? tr("1 phase has every task decided") : tr("%1 phases have every task decided", finishedCount())}
                    onClick={() => switchHide(!hideFinished())}>
                    <Icon name="eye-off" size={12} />{tr("Hide finished")}
                  </button>
                </Show>
                <Show when={props.onAutoAccept}>
                  <button type="button" class="pm-toggle" classList={{ on: autoAccept() }} aria-pressed={autoAccept()} data-toggle="auto-accept"
                    title={autoAccept() ? tr("Auto accepts a draft phase at its first hand-out") : tr("A draft phase waits for your approval")}
                    onClick={() => props.onAutoAccept?.(!autoAccept())}>
                    <Icon name="sparkles" size={12} />{tr("Auto-accept drafts")}
                  </button>
                </Show>
              </Show>
            </div>
          </Show>
          <span class="pt-grow" />
          <div class="pm-group pm-legend-group" data-plan-group="legend">
            {/* Task 342 (owner, 2026-11-08): the five task states, as the cards tint. */}
            <span class="pt-legend pt-state-legend" aria-label={tr("Task states")} data-state-legend>
              <span class="pt-legend-item"><i class="pt-swatch pt-swatch-done" />{tr("Done")}</span>
              <span class="pt-legend-item"><i class="pt-swatch pt-swatch-work" />{tr("In progress")}</span>
              <span class="pt-legend-item"><i class="pt-swatch pt-swatch-review" />{tr("In review")}</span>
              <span class="pt-legend-item"><i class="pt-swatch pt-swatch-notstarted" />{tr("Not started")}</span>
              <span class="pt-legend-item"><i class="pt-swatch pt-swatch-cancelled" />{tr("Cancelled")}</span>
            </span>
          </div>
          <Show when={mode() === "graph"}>
            <div class="pm-group" data-plan-group="links">
              <span class="pt-legend" aria-label={tr("Link legend")}>
                <span class="pt-legend-item"><i class="pt-legend-dep" />{tr("Depends on")}</span>
                <span class="pt-legend-item"><i class="pt-legend-origin" />{tr("Spawned from")}</span>
              </span>
            </div>
            <div class="pm-group pm-zoom" data-plan-group="zoom">
              <button type="button" class="icon-btn small" aria-label={tr("Zoom out")} onClick={() => stepZoom(-0.1)}><Icon name="minus" size={12} /></button>
              <span class="pm-zoom-value" data-zoom={zoom()}>{Math.round(zoom() * 100)}%</span>
              <button type="button" class="icon-btn small" aria-label={tr("Zoom in")} onClick={() => stepZoom(0.1)}><Icon name="plus" size={12} /></button>
              <button type="button" class="btn ghost small" onClick={fit}><Icon name="maximize-2" size={12} />{tr("Fit")}</button>
            </div>
          </Show>
          <Show when={shownMode()}>
            <div class="pm-group" data-plan-group="fold">
              <button type="button" class="btn ghost small" data-expand-all onClick={() => (allOpen() ? collapseAll() : expandAll())}>
                <Icon name={allOpen() ? "arrow-up-to-line" : "chevrons-down"} size={12} />{allOpen() ? tr("Collapse all") : tr("Expand all")}
              </button>
            </div>
          </Show>
        </div>
      </header>
      <Show when={props.team.archived_phases_not_shown}>
        <p class="pt-detail-note">{tr("%1 older archived phases are not shown here", props.team.archived_phases_not_shown ?? 0)}</p>
      </Show>
      <Show when={props.team.tasks.some((task) => task.archived)}>
        <p class="pt-detail-note">{tr("Showing the status window of up to 1000 archived tasks and 200 archived phase headers")}</p>
      </Show>
      <Show when={heatView()}>
        <HeatLegend profile={props.profile} />
      </Show>
      {/* Task 335 (owner, 2026-11-08): no phase strip above the plan. Qt has
          none: each view with phase headers carries the Standard/Free toggle in
          the phase's own header (Board and Layers below, Lanes in its header),
          and Profile and Tests have no phase headers, as in Qt. */}
      <Show when={mode() === "graph"}>
        <div class="pt-plan-body">
          <div class="pt-plan-scroll" ref={scroller} onPointerLeave={() => setHover(null)}
            onClick={(event) => { if (clearsSelection(event.target as Element)) setSelected(null); }}>
            <div class="pt-plan-canvas" style={{ height: `${Math.max(virtualizer.getTotalSize(), ...windowed().map((item) => item.start + phaseHeight(phases()[item.index])))}px` }}>
              {/* Task #16: windowed by position; a row keeps its element while its
                  phase object stays, and only its top follows a new start. */}
              <Index each={windowed()}>
                {(item) => {
                  const phase = () => phases()[item().index];
                  return (
                    <Show when={phase()}>{(shown) => {
                      const entry = createMemo(() => layouts().get(shown().n));
                      // Task 196: made once per row, so an open menu or dialog survives a poll.
                      const actions = props.phaseActions?.(shown().n);
                      return (
                        <div class="pt-row" style={{ top: `${item().start}px` }} data-phase={shown().n}>
                          {header(shown(), true, actions)}
                          <Show when={!isCollapsed(shown().n) && entry()}>
                            <Show when={acceptedTasks(shown()).length > 0 && props.team.free_phase === shown().n}>
                              <div class="pt-free-accepted" data-accepted-fold={acceptedTasks(shown()).length}>
                                <Icon name="check" size={12} />
                                <b>{tr("%1 accepted", acceptedTasks(shown()).length)}</b>
                                <span class="pt-free-accepted-ids">{acceptedTasks(shown()).slice(0, 12).map((task) => `#${task.id}`).join(" ")}{acceptedTasks(shown()).length > 12 ? ` ${tr("and %1 more", acceptedTasks(shown()).length - 12)}` : ""}</span>
                                <button type="button" class="btn ghost small" data-accepted-toggle={acceptedFolded(shown()) ? "folded" : "open"}
                                  onClick={() => toggleAccepted(shown().n)}>{acceptedFolded(shown()) ? tr("Show") : tr("Hide")}</button>
                              </div>
                            </Show>
                            {graphBody(shown(), entry()!.layout, entry()!.routes, entry()!.originRoutes)}
                          </Show>
                        </div>
                      );
                    }}</Show>
                  );
                }}
              </Index>
            </div>
          </div>
          {detail()}
        </div>
      </Show>
      <Show when={mode() === "board"}>
        <div class="pt-plan-body">
          <div class="pt-plan-scroll pm-board-scroll">
            <For each={phases()}>
              {(phase) => (
                <div class="pm-phase" data-phase={phase.n}>
                  {header(phase)}
                  <Show when={!isCollapsed(phase.n)}>{boardPhase(phase, boardKeyList())}</Show>
                </div>
              )}
            </For>
          </div>
          {detail()}
        </div>
      </Show>
      <Show when={mode() === "layers"}>
        <div class="pt-plan-body">
          <div class="pt-plan-scroll pm-layers-scroll">
            <For each={phases()}>
              {(phase) => (
                <div class="pm-phase" data-phase={phase.n}>
                  {header(phase)}
                  <Show when={!isCollapsed(phase.n)}>{layersPhase(phase)}</Show>
                </div>
              )}
            </For>
          </div>
          {detail()}
        </div>
      </Show>
      <Show when={mode() === "lanes"}>
        <LanesView team={props.team} includeCancelled={includeCancelled()} hideFinished={hideFinished()} profile={props.profile} onOpen={open} onEdit={props.onLaneEdit}
          phaseActions={props.phaseActions} />
      </Show>
      <Show when={mode() === "profile"}>
        <div class="pm-profile-scroll">
          <ProfilePage team={props.team} profile={props.profile} loadTimeline={props.loadTimeline}
            statusName={runStatusLabel} onOpen={open} onOpenReport={props.onOpenReport} />
          {detail()}
        </div>
      </Show>
      <Show when={mode() === "tests"}>
        <TestsPanel tests={props.tests} onMore={() => props.onLoadTests?.(props.tests?.next ?? null)} onForce={props.onForceTest} />
        {detail()}
      </Show>
      <Show when={mode() === "builds"}>
        <BuildsPanel builds={props.builds} error={props.buildsError} />
        {detail()}
      </Show>
    </section>
  );
}

/** Owner, 2026-11-09: a task in progress shows the turning Zerus mark, as a working agent does, not a plain arrow. */
function StateGlyph(props: { task: TeamTaskView; problem: boolean; size: number }) {
  return (
    <Show when={!props.problem && props.task.status === "in_progress"} fallback={<Icon name={statusIcon(props.task, props.problem)} size={props.size} />}>
      <span class="pt-working-mark"><ZerusMark size={props.size + 2} /></span>
    </Show>
  );
}
