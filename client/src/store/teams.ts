import { createEffect, createMemo, createRoot, createSignal } from "solid-js";
import { teamsFollow } from "../bridge";
import { catalog } from "./catalog";
import { page, selected } from "./ui";
import { NOTHING, createTeams, sessionIs, type TeamsFollow } from "./teamsView";
import type { ComputerId, TeamMemberView, TeamWaitingItem, TeamWaitingView } from "../types";
import type { Session } from "../ui/sessions/types";

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

/** Task 11: how many views show Waiting for you (the page, the Team tab's banner); while any does, this computer's list is read. */
const [waitingShown, setWaitingShown] = createSignal(0);
/** Shows Waiting for you; the returned function hides it again. */
export function showWaiting(): () => void {
  setWaitingShown((n) => n + 1);
  let shown = true;
  return () => { if (shown) { shown = false; setWaitingShown((n) => n - 1); } };
}

/** Task 11: items answered here leave at once, before the next read drops them. */
const [answeredIds, setAnsweredIds] = createSignal<ReadonlySet<string>>(new Set());
export function answeredWaiting(item: Pick<TeamWaitingItem, "id">) { setAnsweredIds((ids) => new Set([...ids, item.id])); }

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
// the team the session belongs to. Another page, or no selection, reads nothing.
// Waiting for you, while shown, reads this computer's list as well.
createRoot(() => {
  let current = new Set<ComputerId>();
  createEffect(() => {
    const row = page() === "sessions" ? selected() : undefined;
    const computer = row && row.state !== "archived" ? row.computer : undefined;
    const wanted = new Map<ComputerId, TeamsFollow>();
    if (computer !== undefined) wanted.set(computer, { list: true, waiting: false, team: selectedTeam()?.id ?? null });
    if (waitingShown() > 0) wanted.set("", { ...(wanted.get("") ?? NOTHING), waiting: true });
    for (const gone of current) if (!wanted.has(gone)) teams.follow(gone, NOTHING);
    for (const [id, follow] of wanted) teams.follow(id, follow);
    current = new Set(wanted.keys());
  });
});
