import { For, Show, createSignal } from "solid-js";
import type { JSX } from "solid-js";
import { tr } from "../../i18n";
import type { TeamProfileView, TeamTestsView, TeamView } from "../../types";
import type { PlacedNode } from "./plan";
import { KIND_STYLES, keyName, lossKey, styleOf, swatchBackground } from "./profile-palette";
import { taskHeat, type TaskHeat } from "./heat";
import { skin } from "../../skin/runtime";
import {
  boardColumns,
  formatSeconds,
  formatTokens,
  layerRows,
  runTime,
  stageChips,
  stageName,
  testStatus,
  testsRows,
  reportCounts,
  caseName,
  caseFailed,
  type PlanMode,
} from "./planModes";

const MODES: { mode: PlanMode; label: string }[] = [
  { mode: "graph", label: "Graph" },
  { mode: "board", label: "Board" },
  { mode: "layers", label: "Layers" },
  { mode: "profile", label: "Profile" },
  { mode: "tests", label: "Tests" },
];

export function PlanModeBar(props: {
  mode: PlanMode;
  includeCancelled: boolean;
  onMode: (mode: PlanMode) => void;
  onCancelled: (shown: boolean) => void;
  /** Task 82: the lateness heat on Graph, Board and Layers. */
  heat?: boolean;
  onHeat?: (on: boolean) => void;
}): JSX.Element {
  return (
    <div class="pm-modes" role="tablist" aria-label={tr("Plan view")}>
      <For each={MODES}>
        {(item) => (
          <button
            type="button"
            role="tab"
            class="pm-mode"
            aria-selected={props.mode === item.mode}
            classList={{ on: props.mode === item.mode }}
            onClick={() => props.onMode(item.mode)}
          >
            {modeLabel(item.mode)}
          </button>
        )}
      </For>
      <span class="grow" />
      <Show when={props.mode !== "profile" && props.mode !== "tests"}>
        <label class="pm-check">
          <input
            type="checkbox"
            checked={props.includeCancelled}
            onChange={(event) => props.onCancelled(event.currentTarget.checked)}
          />
          {tr("Show cancelled")}
        </label>
        <Show when={props.onHeat}>
          <label class="pm-check" title={tr("Tint each task by how late it is against the plan; its overrun shows beside it")}>
            <input type="checkbox" class="pm-heat-toggle" checked={!!props.heat} onChange={(event) => props.onHeat?.(event.currentTarget.checked)} />
            {tr("Lateness")}
          </label>
        </Show>
      </Show>
    </div>
  );
}

function modeLabel(mode: PlanMode): string {
  if (mode === "graph") return tr("Graph");
  if (mode === "board") return tr("Board");
  if (mode === "layers") return tr("Layers");
  if (mode === "profile") return tr("Profile");
  return tr("Tests");
}

export function stageLabel(kind: string): string {
  if (kind === "review") return tr("Review");
  if (kind === "test") return tr("Test");
  if (kind === "command") return tr("Command");
  if (kind === "implement") return tr("Implement");
  if (kind === "land") return tr("Land");
  return kind;
}

function lossLabel(kind: string): string {
  if (kind === "rework") return tr("Rework");
  if (kind === "member") return tr("Waited for a member");
  if (kind === "deps") return tr("Waited for dependencies");
  if (kind === "lead") return tr("Waited for the lead");
  if (kind === "person_wait") return tr("Waiting for a person's answer");
  return kind;
}

export function runStatusLabel(status: string): string {
  const kind = testStatus(status);
  if (kind === "passed") return tr("Passed");
  if (kind === "timed-out") return tr("Timed out");
  if (kind === "reused") return tr("Reused");
  if (kind === "queued") return tr("Queued");
  return tr("Failed");
}

function columnTitle(key: string): string {
  if (key === "todo") return tr("To do");
  if (key === "active") return tr("Active");
  if (key === "review") return tr("In review");
  if (key === "test") return tr("In test");
  if (key === "done") return tr("Done");
  return tr("Cancelled");
}

function Card(props: { placed: PlacedNode; heat?: TaskHeat | null; onOpen?: (id: number) => void }): JSX.Element {
  const task = () => props.placed.node;
  return (
    <button
      type="button"
      class={`pm-card pm-tone-${tone(task().status)}`}
      style={props.heat ? { "--heat": String(props.heat.tint) } : undefined}
      data-task={task().id}
      title={props.heat?.tip || undefined}
      onClick={() => props.onOpen?.(task().id)}
    >
      <span class="pm-card-line">
        <span class="pm-id">#{task().id}</span>
        <b class="pm-title">{task().title}</b>
      </span>
      <span class="pm-card-meta">
        <span>{task().status || tr("pending")}</span>
        <Show when={task().assignee}><span>{task().assignee}</span></Show>
      </span>
      <Show when={props.placed.task?.origin || props.placed.node.origin}>
        {(origin) => <span class="pm-origin">{tr("Spawned from #%1", origin())}</span>}
      </Show>
      <HeatLine heat={props.heat} />
      <Show when={stageChips(props.placed.task?.pipeline_run).length > 0}>
        <span class="pm-stages">
          <For each={stageChips(props.placed.task?.pipeline_run)}>
            {(stage) => (
              <span class={`pm-stage pm-stage-${stage.state || "waiting"}`} title={stageLabel(stage.kind)}>
                {stageLabel(stage.kind)}
                <Show when={stage.attempt > 0}> {stage.attempt}</Show>
              </span>
            )}
          </For>
        </span>
      </Show>
    </button>
  );
}

function tone(status: string): string {
  if (status === "accepted" || status === "done") return "done";
  if (status === "cancelled" || status === "rejected") return "danger";
  if (status === "in_progress" || status === "working") return "working";
  if (status === "in_review" || status === "submitted") return "review";
  if (status === "in_test") return "test";
  return "muted";
}

export function BoardView(props: {
  phases: { nodes: PlacedNode[] }[];
  includeCancelled: boolean;
  profile?: TeamProfileView | null;
  onOpen?: (id: number) => void;
}): JSX.Element {
  const columns = () => boardColumns(props.phases, props.includeCancelled);
  return (
    <div class="pm-board">
      <For each={columns()}>
        {(column) => (
          <section class="pm-column" aria-label={columnTitle(column.key)}>
            <header class="pm-column-head">
              <b>{columnTitle(column.key)}</b>
              <span class="pm-count">{column.nodes.length}</span>
            </header>
            <div class="pm-column-body">
              <For each={column.nodes.slice(0, 200)}>
                {(placed) => (
                  <Card placed={placed} heat={taskHeat(props.profile, placed.node.id)} onOpen={props.onOpen} />
                )}
              </For>
              <Show when={column.nodes.length > 200}><p class="pm-more">{tr("%1 more not shown", column.nodes.length - 200)}</p></Show>
            </div>
          </section>
        )}
      </For>
    </div>
  );
}

function columnLabel(key: string): string {
  if (key === "todo") return "To do";
  if (key === "active") return "Active";
  if (key === "review") return "In review";
  if (key === "test") return "In test";
  if (key === "done") return "Done";
  return "Cancelled";
}

export function LayersView(props: {
  phases: { nodes: PlacedNode[] }[];
  includeCancelled: boolean;
  profile?: TeamProfileView | null;
  onOpen?: (id: number) => void;
}): JSX.Element {
  const rows = () => layerRows(props.phases, props.includeCancelled);
  return (
    <div class="pm-layers">
      <For each={rows()}>
        {(row) => (
          <section class="pm-layer" aria-label={tr("Layer %1", row.layer)}>
            <header class="pm-layer-head"><b>{tr("Layer %1", row.layer)}</b><span class="pm-count">{row.nodes.length}</span></header>
            <div class="pm-layer-body">
              <For each={row.nodes.slice(0, 200)}>
                {(placed) => <Card placed={placed} heat={taskHeat(props.profile, placed.node.id)} onOpen={props.onOpen} />}
              </For>
            </div>
          </section>
        )}
      </For>
    </div>
  );
}

/** Task #79: a kind's swatch from the shared profile palette (a wait shows its hatch). */
function Swatch(props: { kind?: string; strip?: boolean }): JSX.Element {
  const style = () => (props.kind ? styleOf(props.kind) : undefined);
  const dark = () => skin().appearance === "dark";
  return <span class="pm-swatch" classList={{ strip: props.kind === "build", none: !style() }}
    style={style() ? { background: swatchBackground(style()!, dark()), "border-color": dark() ? style()!.dark : style()!.light } : undefined} aria-hidden="true" />;
}

/** Task 82: a card's lateness and, beside it, its overrun badge in its own colour. */
export function HeatLine(props: { heat?: TaskHeat | null; compact?: boolean }): JSX.Element {
  return (
    <Show when={props.heat && (props.heat.label || props.heat.badge)}>
      <span class="pm-heat-line">
        <Show when={props.heat!.label}>
          <span class="pm-late" classList={{ "pm-late-muted": props.heat!.muted }}>{props.heat!.label}</span>
        </Show>
        <Show when={props.compact ? props.heat!.badgeCompact : props.heat!.badge}>
          <span class="pm-overrun" style={{ "--over": String(props.heat!.over) }} title={props.heat!.badge}>
            {props.compact ? props.heat!.badgeCompact : props.heat!.badge}
          </span>
        </Show>
      </span>
    </Show>
  );
}

/** Task 82: what the heat shows: the lateness tint and its scale, and the overrun's own scale. */
export function HeatLegend(props: { profile?: TeamProfileView | null }): JSX.Element {
  return (
    <div class="pm-heat-legend" aria-label={tr("Heat")}>
      <span>{tr("Late against the plan: own lateness tints, inherited is muted (from #N)")}</span>
      <span class="pm-heat-scale">
        <span>0</span>
        <i class="pm-heat-ramp" />
        <span>{tr("2h or more late")}</span>
      </span>
      <span>{tr("Overrun, own work against the estimate:")}</span>
      <For each={[0, 0.5, 1]}>
        {(over) => <span class="pm-overrun" style={{ "--over": String(over) }}>{over >= 1 ? "x2+" : `x${(1 + over).toFixed(1)}`}</span>}
      </For>
      <Show when={!props.profile}><span class="pm-heat-note">{tr("Reading the profile…")}</span></Show>
      <Show when={props.profile?.task_rows_more}>
        <span class="pm-heat-note">{tr("Lateness of the first %n tasks", props.profile?.task_rows.length ?? 0)}</span>
      </Show>
    </div>
  );
}

/** Task #79: every span kind of the profile, its name in full; the items wrap. */
export function ProfileLegend(): JSX.Element {
  return (
    <div class="pm-legend" aria-label={tr("Span kinds")}>
      <span class="pm-legend-title">{tr("SPANS")}</span>
      <For each={KIND_STYLES}>{(style) => <span class="pm-legend-item" data-kind={style.key}><Swatch kind={style.key} /><span>{keyName(style.key)}</span></span>}</For>
    </div>
  );
}

export function ProfilePanel(props: { profile?: TeamProfileView | null; team?: TeamView; onOpenReport?: (id: number) => void }): JSX.Element {
  return (
    <div class="pm-profile">
      <Show when={props.team?.tasks.some((task) => task.archived)}>
        <h3>{tr("Archived tasks in this window")}</h3>
        <For each={props.team?.tasks.filter((task) => task.archived)}>{(task) => (
          <div class="pm-role-row">
            <button class="btn small" onClick={() => props.onOpenReport?.(task.id)}>{`#${task.id} ${task.title}`}</button>
            <span>{tr("Phase %1", task.phase)}</span>
            <span>{task.status}</span>
            <span class="grow" />
            <span>{task.worked_s == null ? tr("Time unknown") : formatSeconds(task.worked_s)}</span>
          </div>
        )}</For>
      </Show>
      <Show when={props.profile} fallback={<p class="pm-empty">{tr("Reading the profile…")}</p>}>
        {(profile) => (
          <>
            <div class="pm-tiles">
              <div class="pm-tile"><span>{tr("Elapsed")}</span><b>{formatSeconds(profile().elapsed_s ?? 0)}</b></div>
              <div class="pm-tile"><span>{tr("Worked")}</span><b>{formatSeconds(profile().worked_s ?? 0)}</b></div>
              <div class="pm-tile"><span>{tr("Accepted")}</span><b>{profile().accepted}/{profile().tasks}</b></div>
              <div class="pm-tile"><span>{tr("Tokens")}</span><b>{formatTokens(profile().tokens)}</b><Show when={profile().tokens_partial}><i class="pm-note">{tr("partial")}</i></Show></div>
            </div>
            <h3>{tr("Top losses")}</h3>
            <Show when={profile().losses.length > 0} fallback={<p class="pm-empty">{tr("No losses recorded")}</p>}>
              <For each={profile().losses.slice(0, 20)}>
                {(loss) => (
                  <div class="pm-loss-row" data-kind={lossKey(loss.kind) ?? ""}>
                    <Swatch kind={lossKey(loss.kind)} />
                    <span class="pm-loss-kind">{lossLabel(loss.kind)}</span>
                    <Show when={loss.task > 0}><span class="pm-id">#{loss.task}</span></Show>
                    <span class="grow" />
                    <span>{formatSeconds(loss.seconds)}</span>
                    <span class="pm-note">{formatTokens(loss.tokens)}</span>
                  </div>
                )}
              </For>
            </Show>
            <ProfileLegend />
            <h3>{tr("Roles")}</h3>
            <For each={profile().roles.slice(0, 50)}>
              {(role) => (
                <div class="pm-role-row">
                  <span>{role.key || role.kind}</span>
                  <span class="grow" />
                  <span>{role.accepted}/{role.attempts}</span>
                  <span class="pm-note">{role.returned} {tr("returned")}</span>
                  <span class="pm-tokens">{formatTokens(role.tokens_per_accepted)}</span>
                </div>
              )}
            </For>
          </>
        )}
      </Show>
    </div>
  );
}

export function TestsPanel(props: {
  tests?: TeamTestsView | null;
  onMore?: () => void;
}): JSX.Element {
  const rows = () => testsRows(props.tests);
  const [open, setOpen] = createSignal("");
  return (
    <div class="pm-tests">
      <Show when={props.tests} fallback={<p class="pm-empty">{tr("Reading the tests…")}</p>}>
        <Show when={rows().running.length > 0}>
          <h3>{tr("Running now")}</h3>
          <For each={rows().running}>{(item) => <TestQueueRow item={item} />}</For>
        </Show>
        <Show when={rows().waiting.length > 0}>
          <h3>{tr("Waiting")}</h3>
          <For each={rows().waiting}>{(item) => <TestQueueRow item={item} />}</For>
        </Show>
        <h3>{tr("Recent runs")}</h3>
        <Show when={rows().runs.length > 0} fallback={<p class="pm-empty">{tr("No test runs recorded")}</p>}>
          <For each={rows().runs.slice(0, 50)}>
            {(run) => (
              <>
                <div class={`pm-run pm-${testStatus(run.status)}`} data-run={run.id}
                  role={run.report ? "button" : undefined} tabIndex={run.report ? 0 : undefined}
                  onClick={() => { if (run.report) setOpen(open() === run.id ? "" : run.id); }}>
                  <span class="pm-run-status">{runStatusLabel(run.status)}</span>
                  <b class="pm-run-suite">{run.suite}</b>
                  {/* Task 88: how its tests went, from the provider's report. */}
                  <Show when={run.report}>{(report) => (
                    <span classList={{ "pm-note": true, "pm-run-bad": report().failed + report().errors > 0 }} data-run-counts>{reportCounts(report())}</span>
                  )}</Show>
                  <span class="grow" />
                  <span>{runTime(run.duration_ms)}</span>
                  <Show when={run.runner}><span class="pm-note">{run.runner}</span></Show>
                </div>
                <Show when={open() === run.id && run.report}>{(report) => (
                  <div class="pm-cases" data-run-cases={run.id}>
                    <Show when={report().note}><p class="pm-note">{report().note}</p></Show>
                    <For each={report().results}>{(test) => (
                      <div classList={{ "pm-case": true, "pm-case-bad": caseFailed(test) }} data-case-status={test.status}>
                        <span class="pm-case-name">{caseName(test)}</span>
                        <span class="grow" />
                        <span class="pm-note">{runTime(test.ms)}</span>
                        <Show when={test.message}><p class="pm-case-message">{test.message}</p></Show>
                      </div>
                    )}</For>
                    <Show when={report().passed + report().failed + report().skipped + report().errors > report().results.length}>
                      <p class="pm-note">{tr("%1 more tests in the record", report().passed + report().failed + report().skipped + report().errors - report().results.length)}</p>
                    </Show>
                  </div>
                )}</Show>
              </>
            )}
          </For>
        </Show>
        <Show when={props.tests?.next != null && props.onMore}>
          <button type="button" class="btn ghost small" onClick={() => props.onMore?.()}>{tr("Load older")}</button>
        </Show>
      </Show>
    </div>
  );
}

function TestQueueRow(props: { item: TeamTestsView["queue"][number] }): JSX.Element {
  return (
    <div class="pm-queue">
      <span class="pm-run-status">{props.item.state || tr("Queued")}</span>
      <b class="pm-run-suite">{props.item.suite}</b>
      <span class="grow" />
      <Show when={props.item.forced}><span class="pm-pill">{tr("Forced")}</span></Show>
      <Show when={props.item.reason}><span class="pm-note">{props.item.reason}</span></Show>
    </div>
  );
}
