// Adapted from WhiteWh's member-dialog choices to our revisioned operations.
import type { AccountsView } from "../../types";
import type { TeamMember } from "../../store/teams";
import { tr } from "../../i18n";
export const MEMBER_AGENTS = ["claude", "codex", "kimi", "dsh"] as const;
export type AccountChoice = { mode: "freest" } | { mode: "named"; account: string };
export interface MemberChoice { agent: string; model: string; effort: string; account: AccountChoice }
/** Current local catalogs alone grant account membership; remote/evidence
 * rows never become launch choices. The launch writer rechecks sign-in. */
export function memberAccounts(view: AccountsView, agent: string) {
  const found = new Map<string, { id: string; label: string; signedIn: boolean }>();
  for (const account of view.accounts) for (const p of account.members) {
    if (p.host !== "" || p.provider !== agent || !p.installed) continue;
    found.set(p.id, { id: p.id, label: p.label || p.id, signedIn: p.signed_in });
  }
  return [...found.values()].sort((a, b) => a.label.localeCompare(b.label));
}
export function swapBlocked(member: TeamMember): string | undefined {
  if (member.state === "removed") return tr("This member was removed.");
  if (member.open_work === undefined || member.open_work === null) return tr("Refresh the team to check this member's open work.");
  if (member.state !== "paused" || !member.person_stopped || member.open_work !== 0)
    return tr("Stop the member first; open work: %1", member.open_work);
  if (!member.session) return tr("This member has no confirmed session to replace.");
  return undefined;
}

/** Only fields the maintained native launch builders can deliver. */
export function supportedChoice(choice: MemberChoice): MemberChoice {
  return { ...choice, model: choice.agent === "dsh" ? "" : choice.model,
    effort: choice.agent === "claude" || choice.agent === "codex" ? choice.effort : "" };
}
