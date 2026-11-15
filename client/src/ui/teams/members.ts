import { swapBlocked } from "./member-edit";
import { tr } from "../../i18n";
import type { MemberView } from "./types";

// The member column's vocabulary: the account a member runs on and the menu
// of the member operations M1 has (pause, resume, stop, archive).

export const accountLabel = (member: MemberView) => member.account || (member.account_choice?.mode === "freest" ? tr("The freest account") : tr("the default account"));

export interface MemberMenuItem {
  id: "pause" | "resume" | "stop" | "archive" | "swap";
  text: string;
  enabled: boolean;
}

/** The member operations the core has, by the member's current state. */
export function memberMenu(member: MemberView): MemberMenuItem[] {
  const active = member.state === "active";
  const paused = member.state === "paused";
  return [
    { id: "pause", text: tr("Pause"), enabled: active },
    { id: "resume", text: tr("Resume"), enabled: paused },
    { id: "stop", text: tr("Stop"), enabled: active || paused },
    { id: "swap", text: tr("Swap…"), enabled: !swapBlocked(member) },
    { id: "archive", text: tr("Remove"), enabled: active || paused },
  ];
}
