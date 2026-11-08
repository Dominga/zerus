import { sessionUsageView } from "./session-usage-ui";
import { object, text } from "../screens/tasks-ui";
import { tr } from "../../i18n";
import type { Account, AccountsView, Profile, TeamView } from "../../types";
import { accountChoices, rowBinding, type BoundRow } from "./scope-filters";
import { profileTags } from "../screens/accounts-ui";
export function sessionPlan(value: unknown) {
  const lists = object(object(value).task_lists), main = object(lists.main);
  const items = Array.isArray(main.items) ? main.items : [];
  return items.slice(0, 100).map((v) => { const item = object(v); return { title: text(item.title), status: text(item.status) }; }).filter((v) => v.title);
}
export function sessionGoal(value: unknown) {
  const goal = object(object(value).goal);
  const status = text(goal.status);
  const caption = status === "active" ? tr("Pursuing goal") : status === "complete" ? tr("Goal complete")
    : status === "paused" ? tr("Goal paused") : status === "blocked" ? tr("Goal blocked")
    : status === "usage_limited" ? tr("Usage limit reached") : status === "budget_limited" ? tr("Goal budget limit reached") : tr("Goal");
  return { objective: text(goal.objective), completion: text(goal.completion_criterion), status, caption };
}
export function usageRows(value: unknown): [string, string][] {
  const view = sessionUsageView(object(value).session_usage);
  return [...view.rows.map((row): [string, string] => [row.label, row.value]), ...(view.context ? [[tr("Latest context"), view.context] as [string, string]] : [])];
}

/** The row's recorded binding grouped as the account chooser groups it (exact
 * profile id, or an unambiguous older home); an unknown profile finds none. */
export function sessionAccount(row: BoundRow, view: AccountsView): { account?: Account; profile?: Profile; tag?: string } {
  const binding = rowBinding(row);
  const choice = accountChoices(view.accounts, view.binding_evidence_complete ? view.binding_evidence ?? [] : [], []).find((c) => c.bindings.includes(binding));
  const account = choice && view.accounts.find((a) => a.key === choice.id);
  if (!account) return {};
  const profile = account.members.find((p) => p.host === row.computer && (row.accountId ? p.id === row.accountId : p.home === row.accountHome));
  return { account, profile, tag: profile && profileTags(account).get(profile.key) };
}

/** A path under the usual user directories as ~; the caller keeps the full path as its tooltip. */
export function homeShort(path: string): string {
  const home = /^(?:\/home\/[^/]+|\/Users\/[^/]+|\/root)(?=\/|$)/.exec(path) ?? /^[A-Za-z]:\\Users\\[^\\]+(?=\\|$)/.exec(path);
  return home ? `~${path.slice(home[0].length)}` : path;
}

/**
 * Task 206 (owner, 2026-11-07): the Compact skins' side panel (layout
 * `side-panel: sections`) is one scroll of collapsible sections in Qt's
 * InspectorCards order: Environment, Context, Team, Tasks open; Iron rules,
 * Worktrees, Changes and Project tasks folded; SocratiCode open.
 *
 * The remembered folds are one list of card ids (`workspace/inspectorClosed`):
 * a card open by default is listed when folded; one folded by default is
 * listed as `+id` when the owner opened it. */
export function cardOpen(id: string, openByDefault: boolean, listed: ReadonlySet<string>): boolean {
  return openByDefault ? !listed.has(id) : listed.has(`+${id}`);
}

/** The entry a toggle adds or removes for `id`. */
export function cardKey(id: string, openByDefault: boolean): string {
  return openByDefault ? id : `+${id}`;
}

/** The Context section (Qt updateContextCard): tokens used of the window, when the agent reports them. */
export function contextUsage(value: unknown): { used: number; limit: number; percent: number } | undefined {
  const context = object(object(object(value).session_usage).context);
  const used = context.used, limit = context.limit;
  if (typeof used !== "number" || !Number.isFinite(used)) return undefined;
  const window = typeof limit === "number" && Number.isFinite(limit) && limit > 0 ? limit : 0;
  return { used, limit: window, percent: window ? Math.round(100 * used / window) : -1 };
}

/** 1234 as "1k", as Qt's context card counts tokens. */
export const compactCount = (n: number) => n >= 1000 ? `${Math.round(n / 1000)}k` : `${Math.round(n)}`;

/** The Team section's card (Qt TeamPanel summary): the current phase, the
 * finished phases, the accepted tasks and the team's age in seconds. */
export function sideTeam(team: Pick<TeamView, "phases" | "tasks" | "created">, now: number) {
  const phases = team.phases.filter((phase) => phase.total > 0);
  const current = phases.find((phase) => phase.done < phase.total) ?? phases[phases.length - 1];
  const counted = team.tasks.filter((task) => task.status !== "cancelled");
  return {
    phase: current ? { n: current.n, done: current.done, total: current.total } : undefined,
    phases: { done: phases.filter((phase) => phase.done >= phase.total).length, total: phases.length },
    accepted: counted.filter((task) => task.status === "accepted").length,
    total: counted.length,
    age: team.created > 0 ? Math.max(0, now - team.created) : 0,
  };
}
