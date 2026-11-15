import { Show } from "solid-js";
import type { TeamSummary } from "../../store/teams";
import { tr } from "../../i18n";

// Adapted from WhiteWh's TeamStrip/TeamBar (12d02d58a, 065ebb036).
const TEAM_STATUS: Record<string, () => string> = {
  running: () => tr("Running"), planning: () => tr("Planning"), paused: () => tr("Paused"),
  stuck: () => tr("Stuck"), finished: () => tr("Finished"), stopped: () => tr("Stopped"), archived: () => tr("Archived"),
};
const count = (value: number, more: boolean) => `${value}${more ? "+" : ""}`;

function TeamBar(props: { label: string; done: number; total: number; approximate: boolean; muted?: boolean }) {
  const pct = () => !props.approximate && props.total > 0 ? Math.min(100, Math.round(props.done / props.total * 100)) : 0;
  return <span class="tbar" classList={{ muted: props.muted }}>
    <span class="tbar-label">{props.label}</span>
    <span class="tbar-track"><span class="tbar-fill" style={{ width: `${pct()}%` }} /></span>
  </span>;
}

/** The bounded list supplies everything; rendering never follows a team view. */
export function TeamStrip(props: { team: TeamSummary; onOpen: () => void }) {
  const p = () => props.team.progress!;
  const status = () => p().stuck ? tr("Stuck") : (TEAM_STATUS[props.team.status]?.() ?? props.team.status);
  const members = () => {
    const n = p().members;
    const label = n === 1 ? tr("%n member", n) : tr("%n members", n);
    return p().members_more ? label.replace(String(n), `${n}+`) : label;
  };
  const tasks = () => `${count(p().accepted, p().accepted_more)}/${count(p().tasks, p().tasks_more)}`;
  const phases = () => `${count(p().phases_done, p().phases_done_more)}/${count(p().phases, p().phases_more)}`;
  return <button type="button" class="sc-team-strip" data-team-strip aria-label={tr("Open the team %1", props.team.name)}
    title={`${props.team.name}\n${tr("Accepted tasks: %1", tasks())}${p().phases > 1 ? `\n${tr("Phases finished: %1", phases())}` : ""}`}
    onClick={(e) => { e.stopPropagation(); props.onOpen(); }} onDblClick={(e) => e.stopPropagation()}
    onKeyDown={(e) => {
      if (e.key !== "Enter" && e.key !== " ") return;
      e.preventDefault(); e.stopPropagation();
      if (!e.repeat) props.onOpen();
    }}>
    <TeamBar label={tasks()} done={p().accepted} total={p().tasks} approximate={p().tasks_more || p().accepted_more} />
    <Show when={p().phases > 1}><TeamBar label={phases()} done={p().phases_done} total={p().phases}
      approximate={p().phases_more || p().phases_done_more} muted /></Show>
    <span class="sc-team-status" classList={{ stuck: p().stuck }}>{status()}</span>
    <Show when={p().questions > 0}><span class="sc-team-answers" title={tr("Unanswered member questions")}>?{count(p().questions, p().questions_more)}</span></Show>
    <span class="grow" />
    <span class="sc-team-count">{members()}</span>
  </button>;
}
