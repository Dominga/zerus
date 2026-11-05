import { tr } from "../i18n";
import { parseCatalog, type Catalog } from "../ui/teams/catalog";
import type { AccountsView, ComputerId } from "../types";
import type { Session } from "../ui/sessions/types";
import type { ComputerChoice } from "./teamsNew";

// Task 56: what New team offers, wherever it opens (the rail's page, a Team
// tab, Settings > Teams): every computer with its online state, this
// computer's accounts, the team catalog's roles and templates, and the
// selected local session as "This session". Pure: the page passes the
// window's catalog, accounts view, Teams catalog view and selection.

/** This computer first, then every computer the window knows (its catalog or the accounts hub), each once, with whether it is online. */
export function computerChoices(
  computers: Record<ComputerId, { status?: string }>,
  hosts: readonly { computer: ComputerId; name: string; label: string; online: boolean }[],
  online: (computer: ComputerId) => boolean,
): ComputerChoice[] {
  const choices: ComputerChoice[] = [{ alias: "", label: tr("this computer"), online: true }];
  const seen = new Set<string>([""]);
  for (const host of hosts) {
    if (seen.has(host.computer)) continue;
    seen.add(host.computer);
    choices.push({ alias: host.computer, label: host.label || host.name || host.computer, online: host.online && online(host.computer) });
  }
  for (const id of Object.keys(computers).sort()) {
    if (seen.has(id)) continue;
    seen.add(id);
    choices.push({ alias: id, label: id, online: online(id) });
  }
  return choices;
}

export interface AccountChoice { id: string; label: string; provider: string; signedIn: boolean }

/** This computer's agent accounts (Qt's account combos): each profile once, the native ones left to "Default account". */
export function accountChoices(view: Pick<AccountsView, "accounts"> | undefined): AccountChoice[] {
  const out: AccountChoice[] = [];
  const seen = new Set<string>();
  for (const account of view?.accounts ?? []) {
    for (const profile of account.members) {
      if (profile.host !== "" || profile.native || !profile.id || seen.has(`${profile.provider}\n${profile.id}`)) continue;
      seen.add(`${profile.provider}\n${profile.id}`);
      out.push({ id: profile.id, label: profile.label || account.label || profile.id, provider: profile.provider === "deepseek" ? "dsh" : profile.provider, signedIn: profile.signed_in });
    }
  }
  return out;
}

/** The selected local, live session that may lead (Qt's coordinator); none for a peer's or an archived one. */
export function coordinatorOf(row: Pick<Session, "computer" | "name" | "model" | "status" | "state"> | undefined) {
  if (!row || row.computer !== "" || row.state === "archived" || !row.name) return undefined;
  return { name: row.name, agent: row.name.split("/")[0] || "claude", model: row.model || undefined, busy: row.status === "working" };
}

/** The core's catalog view, parsed; none until it is read. */
export const catalogOf = (view: Record<string, unknown> | undefined): Catalog | undefined => (view ? parseCatalog(view) : undefined);
