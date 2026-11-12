import { tr } from "../i18n";
import { accountOptions } from "../ui/provider-selection";
import { parseCatalog, type Catalog, type CatalogTemplate } from "../ui/teams/catalog";
import type { AccountsView, ComputerId, TeamList, TeamSummary } from "../types";
import type { Session } from "../ui/sessions/types";
import { AGENT_LABEL } from "../ui/lib/icons";
import type { AgentKind } from "../ui/sessions/types";
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

/**
 * A computer's agent accounts (Qt's account combos, `m_accounts[host]`): each
 * profile once, the native ones left to "Default account". Task 200: a member
 * on another computer is offered that computer's accounts.
 */
export function accountChoices(view: Pick<AccountsView, "accounts"> | undefined, computer: ComputerId = ""): AccountChoice[] {
  return accountOptions(view, { host: computer, includeNative: false }).map((option) => ({
    id: option.ref.profileId,
    label: option.label,
    provider: option.ref.provider,
    signedIn: option.signedIn,
  }));
}

/** The selected local, live session that may lead (Qt's coordinator); none for a peer's or an archived one. */
export function coordinatorOf(row: Pick<Session, "computer" | "name" | "model" | "status" | "state" | "accountId"> | undefined) {
  if (!row || row.computer !== "" || row.state === "archived" || !row.name) return undefined;
  return { name: row.name, agent: row.name.split("/")[0] || "claude", model: row.model || undefined,
    ...(row.accountId ? { account: row.accountId } : {}), busy: row.status === "working" };
}

/** Task #326 (N2): the lead pane names the session, agent, model and account. */
export function coordinatorLine(coordinator: { name: string; agent: string; model?: string; account?: string },
  accounts: readonly { id: string; label: string }[] = []): string {
  const agent = coordinator.agent === "claude" ? "Claude Code" : AGENT_LABEL[coordinator.agent as AgentKind] ?? coordinator.agent;
  const account = accounts.find((choice) => choice.id === coordinator.account)?.label || coordinator.account;
  return [coordinator.name, agent, coordinator.model, account].filter(Boolean).join(", ");
}

/**
 * Task 200 (Qt F5): the draft a session leads, which its New team form shows
 * again; none when its team is no draft or the session only takes part.
 */
export function draftLedBy(team: Pick<TeamSummary, "id" | "status" | "members"> | undefined, row: Pick<Session, "name"> | undefined): string {
  if (!team || !row || team.status !== "draft") return "";
  return team.members.some((member) => member.lead && member.session?.name === row.name) ? team.id : "";
}

/** Task 200: the drafts New team can open again, newest first. */
export function draftsOf(list: Pick<TeamList, "teams"> | undefined): Pick<TeamSummary, "id" | "name" | "folder" | "updated">[] {
  return (list?.teams ?? []).filter((team) => team.status === "draft").sort((a, b) => b.updated - a.updated);
}

/**
 * Task 200 (Qt teamNewForm, TeamNewTab::m_project): the selected session's
 * project, sent with Start planning as its name and id; the id also reads the
 * project's memory switch. The Inbox (no project) sends neither.
 */
export function projectOfRow(projectId: string, name: (id: string) => string | undefined): { project?: string; projectId?: string } {
  if (!projectId || projectId === "ungrouped") return {};
  return { project: name(projectId) || projectId, projectId };
}

/** Task 200 (Qt TeamTemplateList, F6): a template's member count and tasks, for its line. */
export const templateSeats = (template: Pick<CatalogTemplate, "roster">) => template.roster.reduce((n, seat) => n + Math.max(1, seat.count), 0);
export const templateTaskCount = (template: Pick<CatalogTemplate, "raw">) => (Array.isArray(template.raw.tasks) ? template.raw.tasks.length : 0);

/**
 * Task 200 (Qt TeamTemplateList::rebuild): the templates a search keeps (name,
 * id, description, and the roles it seats by id or name, in any case), only
 * the favourites when asked, favourites first and then by name.
 */
export function shownTemplates(catalog: Pick<Catalog, "templates" | "roles"> | undefined, query: string, favourites: readonly string[], favouritesOnly: boolean): CatalogTemplate[] {
  const needle = query.trim().toLocaleLowerCase();
  const has = (text: string) => text.toLocaleLowerCase().includes(needle);
  const roleName = (id: string) => catalog?.roles.find((role) => role.id === id)?.name ?? id;
  const favourite = new Set(favourites);
  return (catalog?.templates ?? [])
    .filter((template) => !needle || has(template.name) || has(template.id) || has(template.description)
      || template.roster.some((seat) => has(seat.role) || has(roleName(seat.role))))
    .filter((template) => !favouritesOnly || favourite.has(template.id))
    .map((template, index) => ({ template, index }))
    .sort((a, b) => Number(favourite.has(b.template.id)) - Number(favourite.has(a.template.id)) || a.template.name.localeCompare(b.template.name) || a.index - b.index)
    .map(({ template }) => template);
}

/** Task 200: a star given or taken back (Qt toggleFavourite), the list kept in order. */
export const toggledFavourite = (favourites: readonly string[], id: string): string[] =>
  favourites.includes(id) ? favourites.filter((found) => found !== id) : [...favourites, id];

/** The core's catalog view, parsed; none until it is read. */
export const catalogOf = (view: Record<string, unknown> | undefined): Catalog | undefined => (view ? parseCatalog(view) : undefined);
