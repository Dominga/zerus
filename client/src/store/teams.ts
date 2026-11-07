import { createEffect, createMemo, createRoot, createSignal } from "solid-js";
import { onTeamDecided, teamsFollow } from "../bridge";
import { decidedWaiting } from "../ui/teams/decisions";
import { catalog } from "./catalog";
import { page, selected } from "./ui";
import { NOTHING, createTeams, mergeFollows, sessionIs, type TeamsNeed } from "./teamsView";
import type { ComputerId, TeamMemberView, TeamWaitingItem, TeamWaitingView } from "../types";
import type { Session } from "../ui/sessions/types";
import { waitingNotices } from "./waitingNotices";

// The window's Teams views (store/teamsView.ts) and what it follows: the team
// list of the selected session's computer, to find its team, and that team.
export { sessionIs, type TeamsEntry, type TeamsFollow, type TeamsStore } from "./teamsView";

// The bridge is read when a follow is said, not when the module loads.
export const teams = createTeams({ follow: (computer, follow) => teamsFollow(computer, follow) });

/** The selected session's team, from its computer's team list. */
export const selectedTeam = createRoot(() => createMemo(() => {
  const row = selected();
  return row && row.state !== "archived" ? teams.teamOf(row) : undefined;
}));

/** The catalog row of a member's session, if the session list has it. */
export function memberRow(teamComputer: ComputerId, member: Pick<TeamMemberView, "session"> | undefined): Session | undefined {
  const session = member?.session;
  if (!session) return undefined;
  for (const id of catalog.order) {
    const row = catalog.rows[id];
    if (row && row.state !== "archived" && sessionIs(session, teamComputer, row)) return row;
  }
  return undefined;
}

/**
 * Task 47: the one owner of what the Teams views read. Each open view says
 * what it needs (Waiting for you, a team opened by New team or the Plan view,
 * the catalog page) and takes it back when it closes; the selected session's
 * team list and team are needed while the Sessions page shows it. The needs
 * are merged into one follow per computer (`mergeFollows`).
 */
const [shownNeeds, setShownNeeds] = createSignal<readonly TeamsNeed[]>([]);
function show(need: TeamsNeed): () => void {
  setShownNeeds((needs) => [...needs, need]);
  let shown = true;
  return () => { if (shown) { shown = false; setShownNeeds((needs) => needs.filter((found) => found !== need)); } };
}
/** Shows Waiting for you (this computer's list); the returned function hides it again. */
export const showWaiting = (): (() => void) => show({ computer: "", waiting: true });
/** Shows a computer's catalog page; the returned function hides it again. */
export const showCatalog = (computer: ComputerId = ""): (() => void) => show({ computer, catalog: true });
/** Shows one team outside the selected session (New team, the Plan view); the returned function hides it again. */
export const showTeam = (computer: ComputerId, team: string): (() => void) => show({ computer, team });

/** Task 11: items answered here leave at once, before the next read drops them. */
const [answeredIds, setAnsweredIds] = createSignal<ReadonlySet<string>>(new Set());
export function answeredWaiting(item: Pick<TeamWaitingItem, "id">) {
  waitingNotices().withdraw(item.id);
  setAnsweredIds((ids) => new Set([...ids, item.id]));
}

/**
 * Task 107: a decision hgs confirmed anywhere (a proposal card, the Plan
 * view, a task card) takes what it decided out of Waiting for you at once,
 * with its notification, before the next read; a refused or unknown outcome
 * keeps it.
 */
export function decidedTeamAction(call: Parameters<typeof decidedWaiting>[1] & { computer?: ComputerId }) {
  if (call.computer) return;   // Waiting for you lists this computer's teams
  const view = teams.waiting("");
  for (const id of decidedWaiting(view?.items ?? [], call)) answeredWaiting({ id });
}
onTeamDecided(decidedTeamAction);

/** What waits for the owner on this computer, without what was answered here. */
export const waiting = createRoot(() => createMemo((): TeamWaitingView | undefined => {
  const view = teams.waiting("");
  if (!view) return undefined;
  const answered = answeredIds();
  const items = view.items.filter((item) => !answered.has(item.id));
  return { ...view, items, total: Math.max(items.length, view.total - (view.items.length - items.length)) };
}));

// A read without an answered item forgets it: it is gone from the store too.
createRoot(() => createEffect(() => {
  const view = teams.waiting("");
  if (!view) return;
  const listed = new Set(view.items.map((item) => item.id));
  const answered = answeredIds();
  if ([...answered].some((id) => !listed.has(id))) setAnsweredIds(new Set([...answered].filter((id) => listed.has(id))));
}));

// The Sessions page reads the selected session's computer: its team list, and
// the team the session belongs to. Another page, or no selection, reads nothing
// of it. The views shown elsewhere add their needs; every computer gets one
// follow with all of them.
createRoot(() => {
  let current = new Set<ComputerId>();
  createEffect(() => {
    const row = page() === "sessions" ? selected() : undefined;
    const computer = row && row.state !== "archived" ? row.computer : undefined;
    // A lead's card needs its team strip wherever the team's lead row is
    // listed, so the Sessions page reads every computer's team list; only the
    // selected session's computer also follows its one team.
    const needs: TeamsNeed[] = page() === "sessions"
      ? (Object.keys(catalog.computers) as ComputerId[]).map((id) => ({
          computer: id,
          list: true,
          team: id === computer ? selectedTeam()?.id ?? null : null,
        }))
      : [];
    const wanted = mergeFollows([...needs, ...shownNeeds()]);
    for (const gone of current) if (!wanted.has(gone)) teams.follow(gone, NOTHING);
    for (const [id, follow] of wanted) teams.follow(id, follow);
    current = new Set(wanted.keys());
  });
});
