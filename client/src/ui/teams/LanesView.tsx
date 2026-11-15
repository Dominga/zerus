// Collapsible sections and task buttons adapted from WhiteWh's LanesView
// (f49f902a3). The upstream view uses our current member and queue facts only.
import { For, Show, createEffect, createMemo, createSignal, on } from "solid-js";
import { tr } from "../../i18n";
import type { TeamView } from "../../store/teams";
import { Icon } from "../lib/icons";
import { RoleChip, StageChip, TeamAgent } from "./TeamChips";
import { memberStatus } from "./team-model";
import { taskState } from "./planTasks";
import { laneGroups, type MemberLane } from "./lanes";
import "../styles/team-lanes.css";

export function LanesView(props: { view: TeamView; selected: number | null; onOpen: (node: number) => void }) {
  const lanes = createMemo(() => new Map(laneGroups(props.view).map((lane) => [lane.key, lane])));
  const keys = createMemo(() => [...lanes().keys()]);
  const [collapsed, setCollapsed] = createSignal<ReadonlySet<string>>(new Set());
  const teamId = createMemo(() => props.view.bar.id);
  createEffect(on(teamId, () => setCollapsed(new Set<string>()), { defer: true }));
  // Keep only this roster's keys; member churn must not grow a per-team cache.
  createEffect(on(keys, (current) => {
    const kept = new Set(current);
    setCollapsed((previous) => new Set([...previous].filter((key) => kept.has(key))));
  }));
  const toggle = (key: string) => setCollapsed((current) => {
    const next = new Set(current);
    if (next.has(key)) next.delete(key); else next.add(key);
    return next;
  });
  return <div class="team-lanes" data-team-lanes role="region" aria-label={tr("Tasks by member")}>
    <p class="dim team-lanes-note">{tr("Tasks on this queue page, grouped by member.")}</p>
    <Show when={props.view.after || props.view.next || props.view.truncated}><p class="notice" role="note" data-lanes-partial>{tr("This view shows only the current queue page.")}</p></Show>
    <For each={keys()}>{(key) => <LaneSection lane={lanes().get(key)!} collapsed={collapsed().has(key)} selected={props.selected} onToggle={() => toggle(key)} onOpen={props.onOpen} />}</For>
  </div>;
}

/** Primitive lane/card keys retain the scroll container and a focused card
 * across refreshed snapshots. The maps contain only this queue page. */
function LaneSection(props: { lane: MemberLane; collapsed: boolean; selected: number | null; onToggle: () => void; onOpen: (node: number) => void }) {
  const entries = createMemo(() => new Map(props.lane.tasks.map((entry) => [entry.task.node, entry])));
  const keys = createMemo(() => [...entries().keys()]);
  const name = () => props.lane.member?.id ?? tr("Unassigned");
  return <section class="team-lane" data-member-lane={props.lane.member?.id ?? ""} data-lane-key={props.lane.key}>
    <button type="button" class="team-lane-head" aria-expanded={!props.collapsed} onClick={props.onToggle}>
      <Icon name={props.collapsed ? "chevron-right" : "chevron-down"} size={13} />
      <Show when={props.lane.member} fallback={<Icon name="inbox" size={16} />}>{(member) => <TeamAgent agent={member().agent} />}</Show>
      <b class="team-lane-name" title={name()}>{name()}</b>
      <Show when={!props.lane.member && props.lane.tasks.length > 0}><span class="dim">{tr("No verified member on this page")}</span></Show>
      <Show when={props.lane.member}>{(member) => <><RoleChip role={member().role} lead={member().lead} /><span class="dim">{memberStatus(member()).label}</span></>}</Show>
      <span class="dim team-lane-count">{tr("Tasks on this page: %1", props.lane.tasks.length)}</span>
    </button>
    <Show when={!props.collapsed}>
      <div class="team-lane-body">
        <For each={keys()} fallback={<p class="dim team-lane-empty">{tr("No tasks on this page.")}</p>}>{(node) => {
          const entry = () => entries().get(node)!;
          const label = () => {
            const { task, stage, state } = entry();
            // The parent task's pipeline hold is not the active stage's state.
            const held = task.held && (task.kind === "stage" || !(stage?.holder || stage?.assignee));
            return taskState({ ...task, state, held });
          };
          return <button type="button" class="team-lane-task" classList={{ "is-selected": props.selected === node, cancelled: entry().state === "cancelled" }}
            data-lane-task={node} data-state={entry().state} aria-pressed={props.selected === node}
            title={entry().task.title} onClick={() => props.onOpen(entry().task.node)}>
            <b>#{node} {entry().task.title || tr("Task %1", node)}</b>
            <span class="team-lane-task-meta"><span>{label()}</span>
              <Show when={entry().stage}>{(stage) => <StageChip stage={stage()} rounds={entry().task.rounds} on={entry().task} />}</Show>
              <Show when={entry().task.phase && entry().task.phase! > 0}><span>{tr("Phase %1", entry().task.phase!)}</span></Show>
            </span>
          </button>;
        }}</For>
      </div>
    </Show>
  </section>;
}
