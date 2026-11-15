import { tr } from "../../i18n";
import type { MemberView, TeamView } from "./types";

// The Team tab's small vocabulary: state labels and tones, a member's state,
// and the bar's progress. All of it is projected from the core's view; nothing
// here reads or decides.

export type Tone = "neutral" | "active" | "done" | "review" | "cancelled" | "muted" | "warn" | "danger" | "planning";
export type MemberStateColor = "info" | "warn" | "danger" | "muted" | "dim";

/** The colour family of a member's state. */
export function memberStateColor(kind: string): MemberStateColor {
  switch (kind) {
    case "active": return "info";
    case "paused": return "warn";
    case "removed": return "danger";
    case "not_created": return "dim";
    default: return "muted";
  }
}

export interface MemberStatus {
  state: string;
  label: string;
  tone: MemberStateColor;
  lead: boolean;
}

/** What a member's card says about it. */
export function memberStatus(member: MemberView): MemberStatus {
  const state = member.state || "not_created";
  return { state, label: tr(state === "not_created" ? "not created" : state), tone: memberStateColor(state), lead: member.lead };
}

/** The team's own status as a word and a tone. */
export function stateLabel(state: string): string {
  return tr(state === "not_created" ? "not created" : state);
}

export function stateTone(state: string): Tone {
  switch (state) {
    case "running": return "active";
    case "planning": return "planning";
    case "paused": return "warn";
    case "stopped": return "danger";
    case "done":
    case "finished": return "done";
    case "archived": return "muted";
    default: return "neutral";
  }
}

/** The counts the bar draws and the progress track sizes. */
export interface Progress {
  accepted: number;
  reported: number;
  active: number;
  waiting: number;
  total: number;
}

export function progress(view: TeamView): Progress {
  const nodes = view.bar.counts.nodes;
  return {
    accepted: nodes.accepted,
    reported: nodes.reported,
    active: nodes.in_progress,
    waiting: nodes.pending + nodes.waiting,
    total: nodes.waiting + nodes.pending + nodes.in_progress + nodes.reported + nodes.accepted,
  };
}

/** The members the column shows, in the core's order. */
export function shownMembers(view: TeamView): MemberView[] {
  return view.members;
}

/** A duration as a short word (the member cards' "since"). */
export function duration(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return "";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return tr("%1 m", Math.max(1, minutes));
  const hours = Math.floor(minutes / 60);
  return hours < 24 ? tr("%1 h", hours) : tr("%1 d", Math.floor(hours / 24));
}
