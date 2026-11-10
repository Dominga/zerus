import { batch } from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import type { ComputerId, TeamList, TeamSessionView, TeamSummary, TeamView, TeamWaitingView, TeamsOut } from "../types";
import type { Session } from "../ui/sessions/types";

// The Teams views the core reads and projects (client/core/src/teams), as the
// per-frame batches bring them (`batch.teams`): one entry per computer and
// key (`list`, `team:<id>`, `feed:<id>`...). A failed read keeps what the view
// showed and says why; an ended view is gone. What is followed is said once
// per change (src-tauri teams.rs `teams_follow`): the team list of the
// selected session's computer, to find its team, and that team.

export interface TeamsEntry {
  kind: string;
  payload: unknown;
  /** The last read failed; the view keeps what it showed. */
  failed?: string;
}

/** What one computer reads (core `TeamsFollow`): the list, Waiting for you, at most one team and the catalog. */
export interface TeamsFollow { list: boolean; waiting: boolean; team: string | null; catalog: boolean }

/** One view's need on a computer (task 47): the Team tab, Waiting for you, New team or Plan view, the catalog page. */
export interface TeamsNeed { computer: ComputerId; list?: boolean; waiting?: boolean; team?: string | null; catalog?: boolean }

export interface TeamsPort {
  follow(computer: ComputerId, follow: TeamsFollow): Promise<void>;
}

export const NOTHING: TeamsFollow = { list: false, waiting: false, team: null, catalog: false };

/**
 * Every view's needs as one follow per computer (task 47): the core keeps one
 * follow per computer, so a second call would replace the first. The list,
 * Waiting and the catalog are read when any view needs them; the one team is
 * the last need that names one (an explicitly opened team comes after the
 * selected session's).
 */
export function mergeFollows(needs: readonly TeamsNeed[]): Map<ComputerId, TeamsFollow> {
  const merged = new Map<ComputerId, TeamsFollow>();
  for (const need of needs) {
    const now = merged.get(need.computer) ?? NOTHING;
    merged.set(need.computer, {
      list: now.list || !!need.list,
      waiting: now.waiting || !!need.waiting,
      team: need.team ? need.team : now.team,
      catalog: now.catalog || !!need.catalog,
    });
  }
  return merged;
}
const entryId = (computer: ComputerId, key: string) => `${computer}\n${key}`;

/** Whether a member's session is this row: the same name on the team's computer, or on the computer it names. */
export function sessionIs(session: TeamSessionView | null | undefined, teamComputer: ComputerId, row: Pick<Session, "computer" | "name">): boolean {
  if (!session || session.name !== row.name) return false;
  return session.host === "" ? row.computer === teamComputer : session.host === row.computer;
}

/**
 * Task #9: the views a new read reconciles into the one shown, so unchanged
 * rows keep their objects and every <For> keeps their DOM; the key each
 * view's rows are matched by (rows without it merge by position). A team
 * update of one member's field then leaves every task card, phase row and
 * memo that does not read that field alone. The New team draft, the catalog
 * and the on-request views are replaced: their followers apply a new object.
 */
export const RECONCILED: Readonly<Record<string, string>> = {
  team: "id", graph: "id", list: "id", inbox: "id", waiting: "id", feed: "seq",
};

const isObject = (value: unknown): value is object => typeof value === "object" && value !== null && !Array.isArray(value);

export function createTeams(port: TeamsPort) {
  const [views, setViews] = createStore<Record<string, TeamsEntry | undefined>>({});
  const [errors, setErrors] = createStore<Record<string, string | undefined>>({});
  const followed = new Map<ComputerId, string>();

  function apply(outs: readonly TeamsOut[]) {
    batch(() => {
      for (const out of outs) {
        const id = entryId(out.computer, out.key);
        if (out.kind === "ended") { setViews(id, undefined); continue; }
        if (out.kind === "failed") {
          const message = typeof (out.payload as { message?: unknown } | null)?.message === "string" ? (out.payload as { message: string }).message : "";
          // The shown view stays the same object; only its failure changes.
          if (views[id]) setViews(id, "failed", message || out.kind);
          else setViews(id, { kind: "", payload: undefined, failed: message || out.kind });
          continue;
        }
        const key = RECONCILED[out.kind];
        const shown = views[id];
        if (key && shown && shown.kind === out.kind && isObject(shown.payload) && isObject(out.payload)) {
          setViews(id, "payload", reconcile(out.payload, { key }));
          if (shown.failed !== undefined) setViews(id, "failed", undefined);
          continue;
        }
        setViews(id, { kind: out.kind, payload: out.payload, failed: undefined });
      }
    });
  }

  const view = <T,>(computer: ComputerId, key: string, kind: string): T | undefined => {
    const entry = views[entryId(computer, key)];
    return entry && entry.kind === kind ? entry.payload as T : undefined;
  };

  /** Says what to read on a computer, once per change; nothing stops reading it. */
  function follow(computer: ComputerId, next: TeamsFollow) {
    const key = JSON.stringify(next);
    if ((followed.get(computer) ?? JSON.stringify(NOTHING)) === key) return;
    if (key === JSON.stringify(NOTHING)) followed.delete(computer); else followed.set(computer, key);
    setErrors(computer, undefined);
    let asked: Promise<void>;
    try { asked = port.follow(computer, next); } catch (reason) { asked = Promise.reject(reason); }
    void asked.catch((reason) => {
      // A refused follow is tried again with the next change.
      if (followed.get(computer) === key) followed.delete(computer);
      setErrors(computer, String(reason));
    });
  }

  const list = (computer: ComputerId) => view<TeamList>(computer, "list", "list");

  /** The team a session belongs to, on the session's computer; archived teams are left out. */
  function teamOf(row: Pick<Session, "computer" | "name"> | undefined): TeamSummary | undefined {
    if (!row) return undefined;
    return list(row.computer)?.teams.find((team) => team.status !== "archived"
      && team.members.some((member) => sessionIs(member.session, row.computer, row)));
  }

  return {
    apply,
    follow,
    list,
    teamOf,
    team: (computer: ComputerId, id: string) => view<TeamView>(computer, `team:${id}`, "team"),
    /** The dependency graph of the same team (`graph:${id}`). */
    graph: (computer: ComputerId, id: string) => view<import("../ui/teams/types").PlanGraph>(computer, `graph:${id}`, "graph"),
    /** Task 198: the watched team's feed, newest first (`feed:${id}`, core FeedView); Load older asks the core for the page before it. */
    feed: (computer: ComputerId, id: string) => view<import("../types").TeamFeedView>(computer, `feed:${id}`, "feed"),
    /** Task 198: what waits for the lead (`inbox:${id}`, core InboxView), oldest first. */
    inbox: (computer: ComputerId, id: string) => view<import("../types").TeamInboxView>(computer, `inbox:${id}`, "inbox"),
    /** The team's profile, read on request (`profile:${id}`). */
    profile: (computer: ComputerId, id: string) => view<import("../types").TeamProfileView>(computer, `profile:${id}`, "profile"),
    /** Task 42: an open draft as New team shows it (`new:<id>`, kind `new_team`). */
    newTeam: (computer: ComputerId, id: string) => view<import("../ui/teams/types").NewTeamView>(computer, `new:${id}`, "new_team"),
    /** The team catalog and the designer's draft, as the core projects it (`catalog`); parsed by ui/teams/catalog.ts. */
    catalog: (computer: ComputerId) => view<Record<string, unknown>>(computer, "catalog", "catalog"),
    /** Task 11: what waits for the owner across the computer's teams (`waiting`). */
    waiting: (computer: ComputerId) => view<TeamWaitingView>(computer, "waiting", "waiting"),
    /** Why the last read of a view failed, while it still shows what it had. */
    failure: (computer: ComputerId, key: string) => views[entryId(computer, key)]?.failed,
    /** Why the last follow of a computer was refused. */
    followError: (computer: ComputerId) => errors[computer],
    /** Every computer that is followed now (tests). */
    followed: () => new Map(followed),
  };
}

export type TeamsStore = ReturnType<typeof createTeams>;
