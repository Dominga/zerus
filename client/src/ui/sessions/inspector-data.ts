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

const whole = (v: unknown) => typeof v === "number" && Number.isFinite(v) ? Math.trunc(v) : 0;

/** One changed file of the Changes card: its path, added lines (accent) and deleted lines (danger). */
export interface ChangeRow { path: string; added: string; deleted: string }

/**
 * The Changes card (Qt updateChangesCard, SessionsWindow.cpp:7503-7540) from
 * one folder's `hgs git-status`: the header's "+A −D" (empty without
 * changes), a row per reported file (hgs lists at most 50), how many more
 * files a truncated list leaves out, and the empty text. `known` is a
 * status hgs read; `clean` is a known tree without changed files.
 */
export function changesView(status: unknown) {
  const git = object(status), changes = object(git.changes);
  const files = Array.isArray(changes.files) ? changes.files : [];
  const known = git.state === "ok";
  const clean = known && whole(git.changed_files) === 0;
  const rows = files.map((value): ChangeRow => {
    const file = object(value);
    const added = typeof file.added === "number" ? `+${whole(file.added)}` : "bin";
    return {
      path: text(file.path),
      added: file.untracked === true ? tr("new") : added,
      deleted: typeof file.deleted === "number" ? `−${whole(file.deleted)}` : "",
    };
  });
  return {
    aside: Object.keys(changes).length ? `+${whole(changes.added)} −${whole(changes.deleted)}` : "",
    rows,
    more: changes.truncated === true ? whole(changes.tracked_files) + whole(changes.untracked_files) - files.length : 0,
    empty: clean ? tr("Working tree is clean.") : known ? "" : tr("Git status not reported for this folder."),
    known,
    clean,
  };
}

/**
 * A changed file's path in two parts, so the row elides its middle as Qt's
 * InspectorValue(Qt::ElideMiddle) does: the head shrinks with an ellipsis,
 * the tail (the file name, or the last 20 characters of a long one) stays.
 */
export function pathParts(path: string): [string, string] {
  const chars = Array.from(path);
  const cut = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  const name = Array.from(path.slice(cut + 1));
  const tail = name.length > 40 ? 20 : name.length;
  return [chars.slice(0, chars.length - tail).join(""), chars.slice(chars.length - tail).join("")];
}

/** Commit changes… (Qt, SessionsWindow.cpp:1690-1693): the request after the trimmed draft, on its own line. */
export function withRequest(draft: string, request: string): string {
  const kept = draft.trim();
  return kept ? `${kept}\n${request}` : request;
}

/** Open diff's reply (Qt showDiff): the dialog's title, text and truncation note, or the notice shown instead. */
export type DiffOutcome = { notice: string } | { title: string; text: string; note: string };
export function diffOutcome(data: unknown, error: string): DiffOutcome {
  const reply = object(data), state = text(reply.state);
  if (error || state !== "ok") {
    return { notice: state === "too_large" ? tr("The diff is larger than 1 MiB; open it in Terminal.") : tr("The diff could not be read: %1", error || state) };
  }
  return {
    title: tr("Diff: %1", text(reply.path)),
    text: text(reply.diff),
    note: reply.truncated === true ? tr("Showing the first 256 KiB of %1 KiB.", Math.trunc(whole(reply.bytes) / 1024)) : "",
  };
}

/**
 * The working line (Qt updateWorkingLine, SessionsWindow.cpp:4600-4624):
 * while the selected live session works, what it does now: its plan's step
 * in progress (only the plan of the current run), else the tool's detail,
 * the tool, the activity's detail or summary, else "Working". Undefined
 * when the session does not work: offline, not running, its process
 * exited, the inspection failed, the activity is not busy, or it waits for
 * an approval or a question.
 */
export function workingLine(details: unknown, row: { online: boolean; state: string; processState: string }, inspectError = false): string | undefined {
  const value = object(details), phase = text(value.phase);
  if (!row.online || row.state !== "running" || row.processState === "exited" || inspectError || value.activity !== "busy" || phase === "approval" || phase === "question") return undefined;
  let line = "";
  const main = object(object(value.task_lists).main);
  if (text(main.run_id) === text(value.run_id)) {
    const step = (Array.isArray(main.items) ? main.items : []).map(object).find((item) => item.status === "in_progress");
    if (step) line = text(step.title);
  }
  for (const field of ["tool_detail", "current_tool", "activity_detail", "activity_summary"]) if (!line) line = text(value[field]);
  line = line.replace(/\s+/gu, " ").trim();
  return line || tr("Working");
}

/** The turn's time as the working line shows it (Qt WorkingLine::elapsed): "12s", "9m 05s" or "1h 05m". */
export function formatElapsed(seconds: number): string {
  const s = Math.max(0, Math.floor(Number.isFinite(seconds) ? seconds : 0));
  const pad = (n: number) => String(n).padStart(2, "0");
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${pad(s % 60)}s`;
  return `${Math.floor(s / 3600)}h ${pad(Math.floor(s / 60) % 60)}m`;
}
