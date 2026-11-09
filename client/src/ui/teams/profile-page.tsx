import { Show, createSignal } from "solid-js";
import type { JSX } from "solid-js";
import type { TeamProfileView, TeamTimelineView, TeamView } from "../../types";
import { ComparePage, MembersPage, ProfileDetails, ProfileModeTabs, ProfileSummary, TokensPage, type ProfileMode } from "./plan-panels";
import { ProfileTime } from "./TimeView";

/**
 * Task #339: Qt's TeamProfile::timePage order. The totals come first, the
 * member lanes follow with the legend and the estimate row directly under
 * them, and the Top losses come after the lanes. The page used to put the
 * totals and Top losses between the lanes and the estimate row, which left the
 * legend far below the lanes it explains.
 *
 * Task #462 (owner, package 168): Qt's four profile modes are back as a
 * segmented switch: Time, Tokens, Members and Compare (TeamProfile.cpp:884).
 */
export function ProfilePage(props: {
  team: TeamView;
  profile?: TeamProfileView | null;
  loadTimeline?: (phase: number | null) => Promise<TeamTimelineView>;
  statusName: (status: string) => string;
  onOpen?: (task: number) => void;
  onOpenReport?: (task: number) => void;
}): JSX.Element {
  const [mode, setMode] = createSignal<ProfileMode>("time");
  return (
    <div class="pm-profile-page">
      <ProfileModeTabs mode={mode()} onMode={setMode} />
      <Show when={mode() === "time"}>
        <ProfileSummary profile={props.profile} team={props.team.id} />
        <Show when={props.loadTimeline}>{(load) => (
          <ProfileTime team={props.team} load={load()} profile={props.profile} statusName={props.statusName} onOpen={props.onOpen} />
        )}</Show>
        <ProfileDetails profile={props.profile} team={props.team} onOpenReport={props.onOpenReport} />
      </Show>
      <Show when={mode() === "tokens"}><TokensPage profile={props.profile} /></Show>
      <Show when={mode() === "members" && props.loadTimeline}>{(load) => (
        <MembersPage profile={props.profile} team={props.team} load={load()} />
      )}</Show>
      <Show when={mode() === "compare" && props.loadTimeline}>{(load) => (
        <ComparePage team={props.team} load={load()} />
      )}</Show>
    </div>
  );
}
