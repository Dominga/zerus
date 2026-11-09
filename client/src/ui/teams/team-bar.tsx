import { For, Show, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";
import { memberRow } from "../../store/teams";
import { duration, progress, stateLabel, stateTone } from "./team-model";
import { TeamBarActions, TeamBarChips, TeamModeControl } from "./TeamActions";
import { toComposer } from "./MemberCard";
import { sendBackMessage } from "./actions";
import { openRow } from "./openRow";
import type { ComputerId, TeamView } from "../../types";

/**
 * Send back's message reaches the coordinator through its composer (the Tauri
 * client sends a session message only from a composer): the coordinator
 * becomes the selected session, keeps the Team tab, and the message waits in
 * its composer for the owner's Send.
 */
export function toCoordinator(computer: ComputerId, team: TeamView, text: string) {
  // A draft already typed stays; the message follows it (MemberCard toComposer).
  toComposer(computer, team.members.find((member) => member.lead), text);
}

/**
 * Task 40: the team bar. Task #427 rebuilt it as one row with fixed gaps: the
 * name shrinks with an ellipsis, the progress bar has a min and a max width,
 * the time never wraps, and below 1100 px the lower-priority actions (Resume
 * after limits, Team settings, Catalog) fold into a More menu; below 900 px
 * the team actions fold too, so nothing overlaps or wraps at any width.
 */
export function TeamBar(props: {
  computer: ComputerId;
  team: TeamView;
  state: string;
  problemTasks: ReadonlySet<number>;
  onPhase: (phase: number) => void;
  onSettings: () => void;
  onCatalog: () => void;
  /** Task #427: the strip's width (tests); the window's width otherwise. */
  width?: number;
}) {
  const [finishOpen, setFinishOpen] = createSignal(false);
  // U59: the owner's planning mode for the approval (none: as the lead planned).
  const [freeMode, setFreeMode] = createSignal(false);
  const counts = createMemo(() => progress(props.team, props.problemTasks));
  const [now, setNow] = createSignal(Date.now() / 1000);
  const clock = setInterval(() => setNow(Date.now() / 1000), 60_000);
  onCleanup(() => clearInterval(clock));
  // Task #427: one row; below 1100 px the lower-priority actions fold into a
  // More menu instead of overlapping, and below 900 px the team actions fold
  // too. The render tests pass the width; the window's width otherwise.
  const [viewport, setViewport] = createSignal(typeof window === "undefined" ? 1200 : window.innerWidth);
  onMount(() => {
    const resize = () => setViewport(window.innerWidth);
    window.addEventListener("resize", resize);
    onCleanup(() => window.removeEventListener("resize", resize));
  });
  const width = () => props.width ?? viewport();
  const compact = (): "none" | "low" | "high" => (width() < 900 ? "high" : width() < 1100 ? "low" : "none");
  // Task #438: one continuous segmented track: no gaps between the segments, a
  // minimum width for a non-zero one, rounded only at the track's two ends.
  const segments = () => [
    { key: "accepted", n: counts().accepted },
    { key: "submitted", n: counts().submitted },
    { key: "active", n: counts().active },
    { key: "problem", n: counts().problems },
  ].filter((segment) => segment.n > 0);
  const countsTip = () => [tr("Accepted %1", counts().accepted), tr("Submitted %1", counts().submitted),
    tr("In progress %1", counts().active), tr("Problems %1", counts().problems)].join(", ");
  return (
    <header class="team-bar" classList={{ "team-bar-compact": compact() !== "none" }} data-compact={compact()} data-strip>
      <Icon name="users" size={16} />
      <b class="team-name ell" title={props.team.goal || props.team.name}>{props.team.name}</b>
      <span class={`team-badge tone-${stateTone(props.state)}`} data-state={props.state}>{stateLabel(props.state)}</span>
      <TeamModeControl computer={props.computer} team={props.team} />
      <div class="team-progress" role="img" aria-label={countsTip()} title={countsTip()}>
        <For each={segments()}>{(segment) => <i class={`seg ${segment.key}`} style={{ "flex-grow": String(segment.n) }} />}</For>
      </div>
      <span class="mono team-count">{counts().accepted}/{counts().total}</span>
      <Show when={compact() === "none"}>
        <button type="button" class="btn ghost small" data-strip-action="settings" onClick={props.onSettings}>{tr("Team settings")}</button>
        <button type="button" class="btn ghost small" data-strip-action="catalog" onClick={props.onCatalog}>{tr("Catalog")}</button>
      </Show>
      <Show when={props.team.created > 0}><span class="dim team-age" data-strip-time title={tr("Since the team was created")}>{duration(now() - props.team.created)}</span></Show>
      <span class="grow" />
      <TeamBarChips team={props.team} onPhase={props.onPhase} onFinish={() => setFinishOpen(true)} onLead={() => {
        // Task 95: the lead's session shows what it waits on.
        const lead = props.team.members.find((member) => member.lead);
        if (lead) openRow(memberRow(props.computer, lead));
      }} />
      <Show when={compact() === "none"}>
        <Show when={props.state === "planning"}><span class="team-note">{tr("The plan waits for your approval")}</span></Show>
        <Show when={props.state === "done"}><span class="team-note done"><Icon name="check" size={13} />{tr("All tasks are decided")}</span></Show>
        <Show when={props.team.status === "planning"}>
          <label class="team-proposal-option" title={tr("One open phase that takes tasks as they come until you close it")}>
            <input type="checkbox" checked={freeMode()} onChange={(event) => setFreeMode(event.currentTarget.checked)} />{tr("Free planning")}
          </label>
        </Show>
      </Show>
      <TeamBarActions computer={props.computer} team={props.team} planMode={freeMode() ? "free" : ""}
        sendToCoordinator={(text) => toCoordinator(props.computer, props.team, sendBackMessage(text))}
        finishOpen={finishOpen} setFinishOpen={setFinishOpen}
        compact={compact()} onSettings={props.onSettings} onCatalog={props.onCatalog} />
    </header>
  );
}
