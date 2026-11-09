import { For, Show, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import type { JSX } from "solid-js";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";
import type { TeamLaneView, TeamProfileView, TeamTaskView, TeamView } from "../../types";
import { defaultLaneName, laneEditable, laneGroups, laneMoveUpdate, laneName, laneRenameUpdates, laneTitle, nextLane } from "./lanes";
import { taskHeat } from "./heat";
import { TextInput } from "../lib/TextField";

// Task #226: Qt's lane sections (TeamWorkspace::laneSections): a phase whose
// tasks carry lanes shows one section per lane, the unassigned tasks last.
// A lane header's menu renames it and a task's menu moves it, both through
// `edit-draft` (the core's EditTasks), never through a browser confirm.

export interface LaneEdit {
  id: number;
  lane: TeamLaneView | null;
}

export function LanesView(props: {
  team: TeamView;
  includeCancelled: boolean;
  /** Task 342: Hide finished leaves accepted tasks out here too. */
  hideFinished?: boolean;
  profile?: TeamProfileView | null;
  onOpen?: (id: number) => void;
  onEdit?: (phase: number, updates: LaneEdit[]) => Promise<boolean>;
  /** Task 335: the phase's own common actions, with the Standard/Free toggle. */
  phaseActions?: (n: number) => JSX.Element;
}): JSX.Element {
  const [collapsed, setCollapsed] = createSignal<Set<string>>(new Set());
  const [menu, setMenu] = createSignal<{ phase: number; lane?: number; task?: number } | null>(null);
  const [editing, setEditing] = createSignal<{ phase: number; lane?: number; task?: TeamTaskView } | null>(null);
  const [notice, setNotice] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [version, setVersion] = createSignal(0);
  onCleanup(() => setMenu(null));

  // The team's own phases; a phase with no tasks in any lane is still shown
  // when it has tasks at all (lane 0). Empty phases are dropped. Task 342: a
  // finished (accepted) task leaves the lanes while Hide finished is on, as it
  // leaves every other plan mode.
  const sections = createMemo(() => {
    void version();
    const tasks = props.hideFinished ? props.team.tasks.filter((task) => task.status !== "accepted") : props.team.tasks;
    return props.team.phases
      .map((phase) => ({ phase, lanes: laneGroups(tasks, phase.n, props.includeCancelled) }))
      .filter((section) => section.lanes.length > 0);
  });
  const editable = (phase: number) => !!props.onEdit && laneEditable(props.team, phase);
  const key = (phase: number, lane: number) => `${props.team.id}/${phase}/${lane}`;
  const open = (phase: number, lane: number) => !collapsed().has(key(phase, lane));
  const toggle = (phase: number, lane: number) => {
    const next = new Set(collapsed());
    if (next.has(key(phase, lane))) next.delete(key(phase, lane));
    else next.add(key(phase, lane));
    setCollapsed(next);
  };
  const edit = async (phase: number, updates: LaneEdit[]) => {
    if (!props.onEdit || updates.length === 0) return;
    setBusy(true);
    setNotice("");
    try {
      if (!(await props.onEdit(phase, updates))) setNotice(tr("The lane was not changed."));
      else setVersion(version() + 1);
    } finally {
      setBusy(false);
      setEditing(null);
      setMenu(null);
    }
  };

  const dialog = () => {
    const state = editing();
    if (!state) return undefined;
    const phase = props.team.phases.find((candidate) => candidate.n === state.phase);
    const lane = state.lane ?? 0;
    const moving = state.task;
    const initial = moving
      ? defaultLaneName(nextLane(props.team.tasks, state.phase))
      : lane > 0
        ? (laneGroups(props.team.tasks, state.phase).find((group) => group.lane === lane)?.label ?? "")
        : "";
    return { phase, lane, moving, initial };
  };

  return (
    <div class="pm-lanes">
      <Show when={notice()}><p class="pm-lane-note danger" role="status">{notice()}</p></Show>
      <For each={sections()}>
        {(section) => (
          <section class="pm-lanes-phase" aria-label={tr("Phase %1", section.phase.n)}>
            <header class="pm-lanes-head">
              <span class="pt-pn">{section.phase.n}</span>
              <b>{section.phase.title || tr("Phase %1", section.phase.n)}</b>
              <span class="pm-count">{section.lanes.reduce((sum, lane) => sum + lane.tasks.length, 0)}</span>
              {/* Task 335: the phase's own Standard/Free toggle sits in its header, as in Qt. */}
              {props.phaseActions?.(section.phase.n)}
              <span class="pm-lane-tip">{tr("Lanes only shape the plan; hand-out still follows dependencies and phases")}</span>
            </header>
            <For each={section.lanes}>
              {(lane) => (
                <div class="pm-lane" classList={{ "pm-lane-zero": lane.lane === 0 }} data-lane={lane.lane}>
                  <div class="pm-lane-head">
                    <button type="button" class="pm-lane-toggle" aria-expanded={open(section.phase.n, lane.lane)}
                      title={lane.lane > 0 ? tr("Lanes only shape the plan; hand-out still follows dependencies and phases") : tr("These tasks fall back to their dependency layers")}
                      onClick={() => toggle(section.phase.n, lane.lane)}>
                      <Icon name={open(section.phase.n, lane.lane) ? "chevron-down" : "chevron-right"} size={11} />
                      <span class="pm-lane-name">{laneTitle(lane.lane, lane.label, lane.tasks.length)}</span>
                    </button>
                    <Show when={lane.lane > 0 && editable(section.phase.n)}>
                      <span class="pm-lane-actions">
                        <button type="button" class="icon-btn small" aria-label={tr("Lane actions")} aria-haspopup="menu" data-lane-menu={lane.lane}
                          onClick={() => setMenu(menu()?.phase === section.phase.n && menu()?.lane === lane.lane ? null : { phase: section.phase.n, lane: lane.lane })}>
                          <Icon name="ellipsis" size={14} />
                        </button>
                        <Show when={menu()?.phase === section.phase.n && menu()?.lane === lane.lane}>
                          <div class="pm-lane-menu" role="menu">
                            <button type="button" role="menuitem" data-lane-action="rename"
                              onClick={() => { setMenu(null); setEditing({ phase: section.phase.n, lane: lane.lane }); }}>
                              {tr("Rename lane…")}
                            </button>
                          </div>
                        </Show>
                      </span>
                    </Show>
                  </div>
                  <Show when={open(section.phase.n, lane.lane)}>
                    <div class="pm-lane-body">
                      <For each={lane.tasks}>
                        {(task) => (
                          <div class="pm-lane-task" classList={{ cancelled: task.status === "cancelled" }}>
                            <button type="button" class="pm-lane-open" onClick={() => props.onOpen?.(task.id)} title={task.title}>
                              <b>#{task.id} {task.title}</b>
                              <span class="dim">{task.status.replace(/_/g, " ")}{task.assignee ? ` — ${task.assignee}` : ""}</span>
                            </button>
                            <Show when={taskHeat(props.profile, task.id)}>
                              {(heat) => <span class="pm-lane-heat">{heat().label}</span>}
                            </Show>
                            <Show when={editable(section.phase.n)}>
                              <span class="pm-lane-actions">
                                <button type="button" class="icon-btn small" aria-label={tr("Task lane")} aria-haspopup="menu" data-task-lane={task.id}
                                  onClick={() => setMenu(menu()?.phase === section.phase.n && menu()?.task === task.id ? null : { phase: section.phase.n, task: task.id })}>
                                  <Icon name="ellipsis" size={14} />
                                </button>
                                <Show when={menu()?.phase === section.phase.n && menu()?.task === task.id}>
                                  <div class="pm-lane-menu" role="menu">
                                    <For each={laneGroups(props.team.tasks, section.phase.n).filter((group) => group.lane > 0)}>
                                      {(group) => (
                                        <button type="button" role="menuitem" data-task-lane-choice={group.lane}
                                          classList={{ on: task.lane?.n === group.lane }}
                                          onClick={() => void edit(section.phase.n, laneMoveUpdate(task, group.lane, group.label))}>
                                          {laneName(group.lane, group.label)}
                                        </button>
                                      )}
                                    </For>
                                    <button type="button" role="menuitem" data-task-lane-new="" onClick={() => { setMenu(null); setEditing({ phase: section.phase.n, task }); }}>
                                      {tr("New lane…")}
                                    </button>
                                  </div>
                                </Show>
                              </span>
                            </Show>
                          </div>
                        )}
                      </For>
                    </div>
                  </Show>
                </div>
              )}
            </For>
          </section>
        )}
      </For>
      <Show when={sections().length === 0}><p class="pm-empty">{tr("This plan has no tasks yet.")}</p></Show>
      <Show when={editing() && dialog()}>
        {(shown) => (
          <TextDialog
            title={shown().moving ? tr("New lane") : tr("Rename lane %1", shown().lane)}
            hint={tr("Lane name")}
            value={shown().moving ? defaultLaneName(nextLane(props.team.tasks, shown().phase?.n ?? 0)) : shown().initial}
            busy={busy()}
            close={() => setEditing(null)}
            submit={(text) => {
              const phase = shown().phase;
              if (!phase) return;
              if (shown().moving) {
                const lane = nextLane(props.team.tasks, phase.n);
                void edit(phase.n, laneMoveUpdate(shown().moving!, lane, text));
              } else {
                void edit(phase.n, laneRenameUpdates(props.team.tasks, phase.n, shown().lane, text));
              }
            }}
          />
        )}
      </Show>
    </div>
  );
}

/** Qt's QInputDialog::getText: one name, OK and Cancel (Cancel focused). */
function TextDialog(props: { title: string; hint: string; value: string; busy: boolean; close: () => void; submit: (text: string) => void }) {
  let dialog!: HTMLDialogElement;
  let field!: HTMLInputElement;
  const [text, setText] = createSignal(props.value);
  onMount(() => { dialog.showModal(); queueMicrotask(() => { field.focus(); field.select(); }); });
  onCleanup(() => { if (dialog.open) dialog.close(); });
  const ok = () => text().trim() !== "" && !props.busy;
  return (
    <dialog ref={dialog} class="dlg team-text-dialog" aria-labelledby="team-lane-title" onCancel={(event) => { event.preventDefault(); props.close(); }}>
      <div class="dlg-head"><h2 id="team-lane-title">{props.title}</h2></div>
      <div class="dlg-body">
        <label class="field-label" for="team-lane-name">{props.hint}</label>
        <TextInput ref={field} id="team-lane-name" class="field" value={text()} maxlength={40}
          onInput={(event) => setText(event.currentTarget.value)}
          onKeyDown={(event) => { if (event.key === "Enter" && ok()) { event.preventDefault(); props.submit(text()); } }} />
      </div>
      <div class="dlg-foot"><span class="spacer" />
        <button type="button" class="btn" onClick={props.close} autofocus>{tr("Cancel")}</button>
        <button type="button" class="btn primary" disabled={!ok()} onClick={() => props.submit(text())}>{tr("OK")}</button>
      </div>
    </dialog>
  );
}
