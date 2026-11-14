import { invoke } from "@tauri-apps/api/core";
import { createRoot, createSignal, batch } from "solid-js";
import { inTauri } from "../bridge";
import type { TeamsOut } from "../types";

// The Teams views the desktop shows (M1): the team list, one followed team
// and the owner cards. Every value comes from the `teams` key of the per-frame
// batch; the store asks package 12's Tauri commands to follow, to read a page
// and to act, and never runs `hgs` or starts a timer of its own. An action has
// five outcomes; the person's confirmation is asked by the core (`confirm`),
// and an uncertain action only offers Check and Dismiss.

/** A member's session: the computer UUID and the session identity. */
export interface TeamSession { machine: string; session_id: string }
/** A member's bound session on a live team's list row. */
export interface TeamMemberSession { member: string; role?: string; machine: string; session_id: string }
export interface TeamSummary {
  id: string; project: string; name: string; status: string; lead: string; home: string; revision: number;
  /** The lead's bound session: how a session finds its team (package 5's list). */
  lead_session?: TeamSession | null;
  /** The bound sessions of a planning, running or paused team's members;
   * absent when none is bound or the command line does not send them. */
  member_sessions?: TeamMemberSession[] | null;
}
export interface TeamList {
  teams: TeamSummary[]; truncated: boolean; next: string | null;
  /** The cursor this page was read from; absent or null is the first page. */
  after?: string | null;
}
export interface TeamMember {
  id: string; role: string; agent: string; account: string; state: string; lead: boolean; session: TeamSession | null; model?: string | null;
  /** The person stopped this member (the store refuses a second Stop). */
  person_stopped?: boolean;
}
export interface TeamMemberCounts { total: number; not_created: number; active: number; paused: number; removed: number }
export interface TeamNodeCounts { waiting: number; pending: number; in_progress: number; reported: number; accepted: number; cancelled: number }
export interface TeamCounts { members: TeamMemberCounts; nodes: TeamNodeCounts; inbox: number; cards: number }
export interface TeamStage { node: number; index: number; role: string; state: string; assignee: string; implementer: string; kind?: "review" | "test" | "land" | null; target?: number | null; round?: number | null }
export interface TeamTaskRef { project: string; uuid: string }
export interface TeamCard {
  node: number; title: string; state: string; task: TeamTaskRef | null; role: string; pipeline: string;
  assignee: string; implementer: string; rank: string; round: number; held: boolean; admitted: boolean;
  integrated: boolean; decided_by: string; stages: TeamStage[];
  kind?: "task" | "stage" | null; rounds?: number | null;
  /** Who holds the attempt (the holder the store's decisions use): null when
   * nobody does, absent when an older hgs does not say. */
  holder?: string | null;
  /** The kind of the decision that has not settled, when one is pending. */
  pending_decision?: string | null;
  /** The pending decision has not started and a new Return, Reassign or
   * Cancel replaces it; absent (an older hgs) reads as false. */
  pending_replaceable?: boolean;
  /** The stop outcome of a stopped or archived team's row, as a stable
   * code plus the store's refusal reason as data; absent for a live
   * team's rows (zerus-63rru.52 round 2). */
  stop_outcome?: { code: string; reason?: string } | null;
}
export interface TeamGroup { state: string; cards: TeamCard[] }
export interface TeamBar {
  id: string; project: string; name: string; status: string; goal: string; lead: string;
  integration_branch: string; base_commit: string; revision: number; counts: TeamCounts;
}
export interface TeamView {
  bar: TeamBar; members: TeamMember[]; groups: TeamGroup[]; truncated: boolean; next: string | null; revision: number;
  /** The queue page's cursor; absent or null is the first page. */
  after?: string | null;
}

/** One service note beside a quoted report: a code and optional text. */
export interface ServiceNote { code: string; text?: string | null }

/** One open owner card that waits for the person. */
export interface WaitingItem {
  id: string;
  team: string;
  card: number;
  kind: string;
  node: number | null;
  text: string;
  state: string;
  by: string;
  created_ms: number;
  /** The decisions the card takes, in order, as the core read them; absent
   * from an older core: acknowledged alone. */
  decisions?: string[];
  /** The service's statements about a report the card quotes, apart from the
   * member's text; absent from an older core. */
  notes?: ServiceNote[];
}

/** The open owner cards of every team, oldest first, one bounded page. */
export interface WaitingView {
  items: WaitingItem[]; next: string | null;
  /** The cursor this page was read from; absent or null is the first page. */
  after?: string | null;
}

export interface TeamConfirmation { name: string; title: string; text: string; action: string }
export interface TeamCall {
  team: string;
  team_name?: string;
  action: { kind: string } & Record<string, unknown>;
  expected_revision?: number;
  confirmed?: boolean;
  request_id: string;
}
export type TeamOutcome =
  | { outcome: "done"; receipt: unknown; request_id: string }
  | { outcome: "failed"; detail: string; code?: string; request_id: string }
  | { outcome: "uncertain"; detail: string; request_id: string }
  | { outcome: "confirm"; confirmation: TeamConfirmation }
  | { outcome: "pending"; request_id: string };

export interface TeamFollow { list: boolean; waiting: boolean; team: string | null }
export interface TeamCatalogRole { id: string; name: string; agent: string; account: { mode: string; account?: string }; model: string; effort: string; pipeline: string; context: string }
export interface TeamCatalogStage { kind: string; role: string }
export interface TeamCatalogPipeline { id: string; name: string; implementer: string; stages: TeamCatalogStage[]; max_rounds: number; accept: string }
export interface TeamCatalogSeat { role: string; count: number; optional: boolean }
export interface TeamCatalogSettings {
  queue: boolean; routing: string; plan_approval: string; phase_acceptance: string; integration: string; land_allowed: boolean;
  /** What the person lets the team decide without them (the contract's Delegations), by name. */
  delegations?: Record<string, boolean>;
}
export interface TeamCatalogLead { agent: string; account: { mode: string; account?: string }; model: string; effort: string; rules: string }
export interface TeamCatalogTemplate {
  id: string; name: string; description: string; lead: TeamCatalogLead;
  roster: TeamCatalogSeat[]; default_pipeline: string; settings: TeamCatalogSettings;
  /** The digest `catalog show` gives the template (opaque); a create sends it
   * back so the command line refuses a template that changed since it was
   * shown. Absent from an older command line: nothing can be bound. */
  digest?: string | null;
  /** Permissions in the template's delegations that this version does not
   * know: kept by the core's catalog read as a fact of the row, never dropped;
   * Create stays blocked while there are any. */
  unknown_permissions?: string[];
  /** Other fields of what the template shows for confirmation (its settings,
   * roster, lead, the pipelines it uses) that this version does not know. */
  unknown_fields?: string[];
}
export interface TeamCatalog { roles: TeamCatalogRole[]; pipelines: TeamCatalogPipeline[]; templates: TeamCatalogTemplate[]; planning_rules: string[] }
export interface TeamProjectFacts { git: boolean; head: string; branch: string; reason: string }

/** The data the views need from the platform; a test passes its own. */
export interface TeamsTransport {
  follow(follow: TeamFollow, refresh?: boolean): Promise<void>;
  hidden(hidden: boolean): Promise<void>;
  tasksChanged(): Promise<void>;
  page(key: string, cursor: string | null): Promise<void>;
  act(call: TeamCall): Promise<TeamOutcome>;
  actAgain(call: TeamCall): Promise<TeamOutcome>;
  dismiss(call: TeamCall): Promise<void>;
  catalog(): Promise<unknown>;
  saveTemplate(template: TeamCatalogTemplate): Promise<void>;
  project(path: string): Promise<unknown>;
  /** Turns this computer's Teams preview on or off (`hgs team preview`). */
  preview(on: boolean): Promise<unknown>;
}

/** The failure an action call itself can raise before any outcome: a refusal to run it. */
const errorText = (error: unknown) => (typeof error === "string" ? error : error instanceof Error ? error.message : String(error));

/** Package 12's Tauri commands; nothing else touches `hgs`. */
export function tauriTeams(): TeamsTransport {
  const live = () => {
    if (!inTauri()) throw new Error("Teams need a live hgs event stream.");
  };
  return {
    async follow(follow, refresh = false) { live(); await invoke("teams_follow", { list: follow.list, waiting: follow.waiting, team: follow.team, refresh }); },
    async hidden(hidden) { live(); await invoke("teams_hidden", { hidden }); },
    async tasksChanged() { live(); await invoke("teams_tasks_changed"); },
    async page(key, cursor) { live(); await invoke("teams_page", { key, cursor }); },
    async act(call) { live(); return invoke<TeamOutcome>("team_action", { call }); },
    async actAgain(call) { live(); return invoke<TeamOutcome>("team_action_again", { call }); },
    async dismiss(call) { live(); await invoke("team_intent_dismiss", { call }); },
    async catalog() { live(); return invoke<unknown>("teams_catalog"); },
    async saveTemplate(template) { live(); await invoke("teams_template_save", { template }); },
    async project(path) { live(); return invoke<unknown>("teams_project", { path }); },
    async preview(on) { live(); return invoke<unknown>("teams_preview", { on }); },
  };
}


/** A request identity for one intent, made once before the first send. */
export function newRequestId(): string {
  const crypto = globalThis.crypto;
  if (crypto?.randomUUID) return crypto.randomUUID();
  const bytes = new Uint8Array(16);
  crypto?.getRandomValues?.(bytes);
  const hex = [...bytes].map((b) => b.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-4${hex.slice(13, 16)}-8${hex.slice(17, 20)}-${hex.slice(20, 32)}`;
}

/** The session asking for its team: this computer's machine UUID and the
 * session's launch or run. The row's native `sessionId` is not this: a team
 * binds the lead's launch id. */
export interface SessionIdentity { machine?: string | null; launchId?: string | null; runId?: string | null }

/** What the scanned list says about a session's team; an incomplete list says
 * `unknown`, never `none`. */
export interface TeamOfSession { state: "lead" | "none" | "unknown"; team?: string }

/** The preview switch's answer: a refusal keeps hgs's own reason, and an
 * unknown result changes nothing on screen except a notice. */
export type PreviewOutcome =
  | { outcome: "done" }
  | { outcome: "failed"; detail: string }
  | { outcome: "uncertain"; detail: string };

/** One action whose result the desktop does not know: it leaves only by Check
 * with its own request id or by Dismiss. */
export interface UnresolvedCall { call: TeamCall; detail: string; state: "uncertain" | "pending" }

/** A failed outcome's reason, for the view that asked for the action, or a
 * read's failure for its key.
 *
 * A view names itself when it sends: `teams.run(call, "new-team")` (any stable
 * string; "" is the store's own). It reads `teams.failureFor("new-team")` in a
 * memo or an effect and clears it with `teams.clearFailure("new-team")` when
 * the person has seen it or the view changes. A read failure never replaces
 * a view's reason: it is read with `teams.readFailureFor(key)` and goes when
 * that key reads again. `teams.failure()` is the newest reason of any kind. */
export interface TeamFailure { key: string; code: string; message: string; source: string }

/** The New team dialog's one create intent: its team and request ids, kept in
 * the store so that closing the dialog keeps them. */
export interface CreateIntent { team: string; request_id: string }

/** Unresolved calls, in-flight ones included, before a new action is refused. */
export const UNRESOLVED_MAX = 8;
/** List pages scanned to find a lead's session; a longer list says unknown. */
export const LIST_PAGES_MAX = 5;
/** A later list page older than this is read again before it decides an answer. */
export const LIST_PAGE_FRESH_MS = 60 * 1000;
/** A window hidden at least this long re-asks whether Teams exist when it returns. */
export const HIDDEN_RECHECK_MS = 10 * 60 * 1000;
/** How long a decision waits for its team's revision: the read in flight and
 * the team's status read, each bounded at 15 seconds, and a margin. */
export const REVISION_WAIT_MS = 35 * 1000;
/** Asking views whose refusal is kept; the oldest goes first. */
export const FAILURES_KEPT = 16;
/** Read keys whose failure is kept: `list`, `waiting` and a few teams. */
const READ_FAILURES_KEPT = 8;

const OFF_CODES = new Set(["teams_inactive", "unavailable"]);

interface ListPage { view: TeamList; at: number }
interface ListScan { views: TeamList[]; complete: boolean; missing: string | null }

/** The cursors of the chain from the first page, at most [`LIST_PAGES_MAX`],
 * and the cursor after it: null when the chain ends, or when a cursor repeats
 * (a cycle, which no page can complete). */
function chainOf(pages: ReadonlyMap<string, ListPage>): { keys: Set<string>; next: string | null; cycle: boolean } {
  const keys = new Set<string>();
  let at: string | null = "";
  while (at !== null && keys.size < LIST_PAGES_MAX && pages.has(at) && !keys.has(at)) { keys.add(at); at = pages.get(at)!.view.next; }
  const cycle = at !== null && keys.has(at);
  return { keys, next: cycle ? null : at, cycle };
}

/** The pages reachable from the first one, at most [`LIST_PAGES_MAX`]; a later
 * page older than `fresh` counts as missing, and a repeated cursor ends the
 * walk as an incomplete list. */
function scanPages(pages: ReadonlyMap<string, ListPage>, now: number, fresh = LIST_PAGE_FRESH_MS): ListScan {
  const views: TeamList[] = [];
  const seen = new Set<string>();
  let truncated = false;
  let key: string | null = "";
  while (key !== null && views.length < LIST_PAGES_MAX) {
    if (seen.has(key)) return { views, complete: false, missing: null };
    seen.add(key);
    const page = pages.get(key);
    if (!page || (key !== "" && now - page.at > fresh)) return { views, complete: false, missing: key === "" ? null : key };
    views.push(page.view);
    truncated ||= page.view.truncated;
    key = page.view.next;
  }
  return { views, complete: key === null && !truncated, missing: null };
}

export function createTeams(transport: TeamsTransport = tauriTeams()) {
  /** The list pages by the cursor they were read from ("" is the first),
   * replaced by every read of that page and pruned to the chain from the first. */
  const [listPagesBy, setListPagesBy] = createSignal<ReadonlyMap<string, ListPage>>(new Map());
  const [waitingView, setWaitingView] = createSignal<WaitingView>();
  /** The last first page of Waiting: the rail counts it, never a later page. */
  const [waitingFirst, setWaitingFirst] = createSignal<WaitingView>();
  const [team, setTeam] = createSignal<TeamView>();
  const [failure, setFailure] = createSignal<TeamFailure | null>(null);
  const [actionFailures, setActionFailures] = createSignal<ReadonlyMap<string, TeamFailure>>(new Map());
  const [readFailures, setReadFailures] = createSignal<ReadonlyMap<string, TeamFailure>>(new Map());
  /** undefined until the first list answer; false when Teams are off or hgs cannot run. */
  const [available, setAvailable] = createSignal<boolean>();
  const [off, setOff] = createSignal<"inactive" | "unavailable" | null>(null);
  const [followed, setFollowed] = createSignal<string>();
  const [visible, setVisible] = createSignal(true);
  const [catalog, setCatalog] = createSignal<TeamCatalog>();
  const [catalogError, setCatalogError] = createSignal("");
  const [newTeamOpen, setNewTeamOpen] = createSignal(false);
  const [confirmation, setConfirmation] = createSignal<{ call: TeamCall; confirmation: TeamConfirmation } | null>(null);
  /** Every call whose result is unknown, bounded and never evicted by another. */
  const [unresolved, setUnresolved] = createSignal<UnresolvedCall[]>([]);
  /** The request ids whose invoke is in flight: each holds an admitted slot. */
  const [sending, setSending] = createSignal<readonly string[]>([]);
  const [createIntent, setCreateIntent] = createSignal<CreateIntent | null>(null);
  /** Resolvers by team id, for the decisions that wait for that team's revision. */
  const pendingViews = new Map<string, Set<(view: TeamView | null) => void>>();
  /** List page cursors asked for and not yet answered, so one is never asked twice. */
  const requestedPages = new Set<string>();
  /** The list page this store last selected for the reader; null is the first. */
  let listSelected: string | null = null;
  /** A positive off answer stops everything until the application asks again. */
  let stopped = false;
  let probing = false;
  let hiddenAt: number | undefined;
  /** Every follow this store sends; a borrowed follow restores only its own. */
  let followGeneration = 0;
  let lastFollow: TeamFollow | undefined;

  const busy = () => sending().length > 0;
  const unresolvedOf = (requestId: string) => unresolved().find((entry) => entry.call.request_id === requestId);
  function putUnresolved(call: TeamCall, detail: string, state: UnresolvedCall["state"]) {
    setUnresolved([...unresolved().filter((entry) => entry.call.request_id !== call.request_id), { call, detail, state }]);
  }
  function settled(requestId: string) {
    batch(() => {
      setUnresolved(unresolved().filter((entry) => entry.call.request_id !== requestId));
      if (createIntent()?.request_id === requestId) setCreateIntent(null);
    });
  }

  // ----------------------------------------------------------- failures

  function failAction(source: string, code: string, message: string) {
    const reason: TeamFailure = { key: "action", code, message, source };
    const next = new Map(actionFailures());
    next.delete(source);
    next.set(source, reason);
    for (const oldest of next.keys()) { if (next.size <= FAILURES_KEPT) break; next.delete(oldest); }
    batch(() => { setActionFailures(next); setFailure(reason); });
  }
  function failRead(key: string, code: string, message: string) {
    const reason: TeamFailure = { key, code, message, source: "" };
    const next = new Map(readFailures());
    next.delete(key);
    next.set(key, reason);
    for (const oldest of next.keys()) { if (next.size <= READ_FAILURES_KEPT) break; next.delete(oldest); }
    batch(() => { setReadFailures(next); setFailure(reason); });
  }
  function readSucceeded(key: string) {
    if (!readFailures().has(key)) return;
    const next = new Map(readFailures());
    next.delete(key);
    batch(() => {
      setReadFailures(next);
      const newest = failure();
      if (newest && newest.source === "" && newest.key === key) setFailure(null);
    });
  }
  /** A view's own refusal; "" asks for the newest reason of any kind. */
  const failureFor = (source: string) => source === "" ? failure() : actionFailures().get(source) ?? null;
  /** The failure of one read (`list`, `waiting`, `team:<id>`) until it reads again. */
  const readFailureFor = (key: string) => readFailures().get(key) ?? null;
  /** Clears one view's reason, or every reason when no source is given. */
  function clearFailure(source?: string) {
    if (source === undefined) { batch(() => { setFailure(null); setActionFailures(new Map()); setReadFailures(new Map()); }); return; }
    if (!actionFailures().has(source) && failure()?.source !== source) return;
    const next = new Map(actionFailures());
    next.delete(source);
    batch(() => { setActionFailures(next); if (failure()?.source === source) setFailure(null); });
  }

  // ------------------------------------------------------------- follow

  const scope = (): TeamFollow => ({ list: true, waiting: true, team: followed() ?? null });
  const sameFollow = (a: TeamFollow | undefined, b: TeamFollow) => !!a && a.list === b.list && a.waiting === b.waiting && a.team === b.team;
  function follow(next: TeamFollow, refresh = false): Promise<void> {
    followGeneration++;
    lastFollow = next;
    return transport.follow(next, refresh);
  }

  /** A positive off answer hides the entry points, unfollows and stops the reads;
   * only the application's recheck starts them again. */
  function stopFor(code: string) {
    stopped = true;
    batch(() => {
      setAvailable(false); setOff(code === "unavailable" ? "unavailable" : "inactive");
      setListPagesBy(new Map()); setTeam(undefined); setWaitingView(undefined); setWaitingFirst(undefined);
    });
    requestedPages.clear();
    listSelected = null;
    for (const [id, waiters] of pendingViews) { pendingViews.delete(id); for (const settle of waiters) settle(null); }
    void follow({ list: false, waiting: false, team: null }).catch(() => {});
    void transport.hidden(true).catch(() => {});
  }

  // --------------------------------------------------------------- list

  /** Replaces a page by its cursor and keeps only the chain from the first
   * page. A later page is taken when its cursor is already in the chain (a
   * refresh, or a stale page read again) or is the chain's next cursor within
   * the bound; a page of no cursor in the chain, or one that left it, is not. */
  function applyList(view: TeamList) {
    const key = view.after ?? "";
    const pages = new Map(listPagesBy());
    if (key !== "") {
      const chain = chainOf(pages);
      const known = chain.keys.has(key);
      const tail = chain.next === key && chain.keys.size < LIST_PAGES_MAX;
      if (!known && !tail) { requestedPages.delete(key); return; }
    }
    pages.set(key, { view, at: Date.now() });
    const keep = chainOf(pages).keys;
    for (const known of [...pages.keys()]) if (!keep.has(known)) pages.delete(known);
    requestedPages.delete(key);
    setListPagesBy(pages);
  }

  function requestListPage(cursor: string | null) {
    if (stopped) return;
    if (cursor !== null) {
      if (requestedPages.has(cursor)) return;
      requestedPages.add(cursor);
    } else if (listSelected === null) return;
    const before = listSelected;
    listSelected = cursor;
    void transport.page("list", cursor).catch(() => {
      if (cursor !== null) requestedPages.delete(cursor);
      if (listSelected === cursor) listSelected = before;
    });
  }

  /** The teams of the scanned pages: at most [`LIST_PAGES_MAX`] pages. */
  const listed = () => scanPages(listPagesBy(), Date.now(), Number.POSITIVE_INFINITY).views.flatMap((page) => page.teams);

  /**
   * Whether a session leads a team, from the scanned list and nothing else: no
   * cache and no prefix guessing. Pages are asked for one at a time up to
   * [`LIST_PAGES_MAX`]; while a page is missing, or the list goes on past the
   * bound, or a page is truncated, the answer is `unknown`. A complete list,
   * an empty one included, answers `none`. The reader goes back to the first
   * page once a scan has its answer.
   */
  function teamOfSession(identity: SessionIdentity): TeamOfSession {
    const ids = [identity.launchId ? `launch_id:${identity.launchId}` : "", identity.runId ? `run_id:${identity.runId}` : ""].filter(Boolean);
    if (!ids.length) return { state: "none" };
    const machine = identity.machine ?? "";
    if (!machine) return { state: "unknown" };
    const scan = scanPages(listPagesBy(), Date.now());
    const found = scan.views.flatMap((page) => page.teams).find((summary) => {
      const lead = summary.lead_session;
      return !!lead && lead.machine === machine && ids.includes(lead.session_id);
    });
    if (!found && scan.missing !== null) { requestListPage(scan.missing); return { state: "unknown" }; }
    requestListPage(null);
    if (found) return { state: "lead", team: found.id };
    return scan.complete ? { state: "none" } : { state: "unknown" };
  }

  /**
   * A team's summary from the scanned list, for a view that opens a team by
   * its id: `reading` while a page is still to be read (it is asked for, one
   * at a time up to [`LIST_PAGES_MAX`]), `none` when the scanned list is
   * complete without it, `unknown` when the list goes on past the bound.
   */
  function summaryOf(id: string): { state: "found"; summary: TeamSummary } | { state: "reading" | "none" | "unknown" } {
    const scan = scanPages(listPagesBy(), Date.now());
    const summary = scan.views.flatMap((page) => page.teams).find((candidate) => candidate.id === id);
    if (!summary && scan.missing !== null) { requestListPage(scan.missing); return { state: "reading" }; }
    requestListPage(null);
    if (summary) return { state: "found", summary };
    if (!scan.views.length) return { state: "reading" };
    return { state: scan.complete ? "none" : "unknown" };
  }

  // ------------------------------------------------------------- apply

  function apply(entries: TeamsOut[]) {
    if (stopped) return;
    for (const entry of entries.slice(0, 8)) {
      if (stopped) return;
      const payload = entry.payload as { code: string; message: string };
      if (entry.kind === "failed") {
        failRead(entry.key, payload.code, payload.message);
        if (OFF_CODES.has(payload.code)) { stopFor(payload.code); return; }
      }
      if (entry.key === "waiting") {
        if (entry.kind === "waiting") {
          const view = entry.payload as WaitingView;
          batch(() => { setWaitingView(view); if (!view.after) setWaitingFirst(view); });
          readSucceeded("waiting");
        }
        continue;
      }
      if (entry.key === "list") {
        if (entry.kind === "list") {
          applyList(entry.payload as TeamList);
          readSucceeded("list");
          batch(() => { setAvailable(true); setOff(null); });
        } else if (entry.kind === "failed") {
          if (payload.code === "cursor_reset") { listSelected = null; requestedPages.clear(); }
          else if (available() === undefined) batch(() => { setAvailable(false); setOff("unavailable"); });
        } else if (entry.kind === "ended") {
          // The scope was retired: the reader forgot its page selection.
          listSelected = null; requestedPages.clear();
        }
        continue;
      }
      if (entry.key.startsWith("team:")) {
        const id = entry.key.slice("team:".length);
        if (entry.kind === "team") {
          const view = entry.payload as TeamView;
          // A view is shown only for the followed team; a late view after
          // unfollow is dropped. A decision waiting for this team's revision
          // takes it without showing it.
          if (view.bar.id === followed()) { batch(() => { setTeam(view); setAvailable(true); }); readSucceeded(entry.key); }
          settleViews(view.bar.id, view);
        } else if (entry.kind === "failed") {
          if (payload.code !== "cursor_reset") settleViews(id, null);
        } else if (entry.kind === "ended") {
          const shown = team();
          if (!shown || entry.key === `team:${shown.bar.id}` || entry.key === `team:${followed()}`) setTeam(undefined);
          settleViews(id, null);
        }
      }
    }
  }

  function settleViews(id: string, view: TeamView | null) {
    const waiters = pendingViews.get(id);
    if (!waiters) return;
    pendingViews.delete(id);
    for (const settle of waiters) settle(view);
  }

  // ------------------------------------------------- discovery and scope

  /** Asks whether Teams exist and follows the list, the waiting read and the
   * open tab's team. Only the application's availability owner calls it: the
   * start, opening Settings, the preview switch, an explicit "read again" and a long-hidden
   * window's return. */
  async function recheck() {
    if (probing) return;
    probing = true;
    stopped = false;
    // Keep the published list until a fresh read replaces it. Ask for the
    // answer again even when the followed scope and reply are unchanged.
    try {
      await transport.hidden(false);
      await follow(scope(), true);
    } catch (error) {
      stopped = true;
      batch(() => { failRead("list", "unavailable", errorText(error)); setAvailable(false); setOff("unavailable"); });
    } finally { probing = false; }
  }

  /** The application asks once at start (App's mount); it never polls by itself. */
  async function probe() { await recheck(); }

  /** Window visibility belongs to the application, not to a mounted tab. While
   * Teams are off the reads stay stopped; a window hidden ten minutes or more
   * asks again when it returns. Never raises from a visibility change. */
  async function hidden(hiddenNow: boolean) {
    setVisible(!hiddenNow);
    try {
      if (hiddenNow) { hiddenAt = Date.now(); await transport.hidden(true); return; }
      const was = hiddenAt;
      hiddenAt = undefined;
      if (was !== undefined && Date.now() - was >= HIDDEN_RECHECK_MS) { await recheck(); return; }
      if (stopped) return;
      await transport.hidden(false);
    } catch { /* a synthetic or failed helper: the reads stay as they are */ }
  }

  /** A Team tab follows its team beside the list and the waiting read. With
   * Teams off nothing is followed: only the application's recheck restarts. */
  async function openTeam(id: string) {
    setFollowed(id);
    if (stopped) return;
    await follow(scope());
  }
  async function closeTeam() {
    batch(() => { setFollowed(undefined); setTeam(undefined); });
    if (stopped) return;
    await follow(scope());
  }
  async function tasksChanged() { if (followed() && !stopped) await transport.tasksChanged(); }
  async function page(key: string, cursor: string | null) { if (!stopped) await transport.page(key, cursor); }

  // ------------------------------------------------------------- waiting

  /** Waiting opens on its first page. */
  async function openWaiting() {
    if (stopped) return;
    if (!sameFollow(lastFollow, scope())) await follow(scope());
    if (waitingView()?.after) await transport.page("waiting", null);
  }
  async function pageWaiting(cursor: string | null) { if (!stopped) await transport.page("waiting", cursor); }
  /** Back to Waiting's first page, when a later one is shown. */
  async function firstWaitingPage() { if (!stopped && waitingView()?.after) await transport.page("waiting", null); }
  /** The rail's count: the first page's cards and one more when a page follows. */
  const waitingCount = () => {
    const view = waitingFirst();
    return !view ? 0 : view.items.length + (view.next ? 1 : 0);
  };
  const waitingLater = () => !!waitingView()?.after;

  /**
   * The revision of a team a decision is made on. The resolver is registered
   * before the follow is sent, a read failure or an end settles it at once,
   * and [`REVISION_WAIT_MS`] settles a silent read. Afterwards the scope the
   * views want is followed again, unless another follow was sent meanwhile.
   */
  async function revisionOf(id: string): Promise<number | undefined> {
    const shown = team();
    if (shown?.bar.id === id && followed() === id) return shown.revision;
    if (stopped) return undefined;
    let settle!: (view: TeamView | null) => void;
    const answer = new Promise<number | undefined>((resolve) => { settle = (view) => resolve(view?.revision); });
    const waiters = pendingViews.get(id) ?? new Set();
    waiters.add(settle);
    pendingViews.set(id, waiters);
    const timer = setTimeout(() => { pendingViews.get(id)?.delete(settle); settle(null); }, REVISION_WAIT_MS);
    const borrowed: TeamFollow = { list: true, waiting: true, team: id };
    const sent = follow(borrowed);
    const generation = followGeneration;
    try {
      await sent;
    } catch {
      pendingViews.get(id)?.delete(settle);
      settle(null);
    }
    const revision = await answer;
    clearTimeout(timer);
    const left = pendingViews.get(id);
    if (left) { left.delete(settle); if (!left.size) pendingViews.delete(id); }
    if (generation === followGeneration && !stopped && !sameFollow(borrowed, scope())) await follow(scope()).catch(() => {});
    return revision;
  }
  async function decide(card: WaitingItem, decision: string, source = "waiting"): Promise<TeamOutcome | undefined> {
    const expected_revision = await revisionOf(card.team);
    if (expected_revision === undefined) return undefined;
    return run({ team: card.team, expected_revision, request_id: newRequestId(), action: { kind: "decide", card: card.card, decision } }, source);
  }

  // ------------------------------------------------------------ New team

  async function openNewTeam() {
    setNewTeamOpen(true);
    if (catalog()) return;
    await reloadCatalog();
  }
  /** Reads the catalog from the command line; on a failure the one shown stays
   * and the reason is told. True when the catalog was read. */
  async function reloadCatalog(): Promise<boolean> {
    try {
      const reply = (await transport.catalog()) as { catalog?: TeamCatalog } | TeamCatalog;
      const candidate = reply && "catalog" in reply && reply.catalog ? reply.catalog : (reply as TeamCatalog);
      if (!candidate || !Array.isArray(candidate.templates) || !Array.isArray(candidate.roles) || !Array.isArray(candidate.pipelines)) {
        throw new Error("The catalog answer has no templates. Nothing was created.");
      }
      setCatalog(candidate);
      setCatalogError("");
      return true;
    } catch (error) { setCatalogError(errorText(error)); return false; }
  }
  /** Closing keeps the create intent; a reason the person has seen goes. */
  function closeNewTeam() {
    setNewTeamOpen(false);
    if (!createIntent()) clearFailure("new-team");
  }
  /** The held create intent, or a new one when none is held: never two team ids. */
  function claimCreate(): CreateIntent {
    const held = createIntent();
    if (held) return held;
    const fresh = { team: newRequestId(), request_id: newRequestId() };
    setCreateIntent(fresh);
    return fresh;
  }

  /** Saves a template, then reads the catalog again: only the command line's
   * read gives the saved template its digest, which a create binds. */
  async function saveTemplate(template: TeamCatalogTemplate) {
    await transport.saveTemplate(template);
    await reloadCatalog();
  }

  /** A refusal carries hgs's own `code: message`; anything else is unknown,
   * and so is the store's `storage_failed`, whose result is not known. */
  const isRefusal = (detail: string) => /^[a-z][a-z0-9_]*(: |$)/.test(detail.trim()) && !/^storage_failed(: |$)/.test(detail.trim());

  /**
   * Turns the preview on or off, then asks discovery again so the entry points
   * appear or disappear without a restart. A refusal keeps its reason; an
   * unknown result re-asks nothing, because the screen must not move on a guess.
   */
  async function preview(on: boolean): Promise<PreviewOutcome> {
    try {
      await transport.preview(on);
      await recheck();
      return { outcome: "done" };
    } catch (error) {
      const detail = errorText(error);
      return isRefusal(detail) ? { outcome: "failed", detail } : { outcome: "uncertain", detail };
    }
  }

  async function project(path: string): Promise<TeamProjectFacts> {
    const reply = (await transport.project(path)) as { git?: boolean; head?: string; branch?: string; state?: string };
    const git = reply.git ?? reply.state === "ok";
    return { git, head: reply.head ?? "", branch: reply.branch ?? "", reason: git ? "" : "not a Git repository" };
  }

  // ------------------------------------------------------------- actions

  /** The admitted slots: unresolved calls and in-flight ones, each id once. */
  function occupied(except?: string) {
    const ids = new Set(unresolved().map((entry) => entry.call.request_id));
    for (const id of sending()) ids.add(id);
    if (except !== undefined) ids.delete(except);
    return ids.size;
  }
  const refuseFull = (call: TeamCall, source: string): TeamOutcome => {
    const detail = "There are too many unresolved actions. Check or dismiss them first.";
    failAction(source, "unresolved_full", detail);
    // Nothing was sent: a create intent refused here has no other trace.
    if (!unresolvedOf(call.request_id) && createIntent()?.request_id === call.request_id) setCreateIntent(null);
    return { outcome: "failed", detail, code: "unresolved_full", request_id: call.request_id };
  };

  /** Sends one intent; `source` names the view that asked for its reason. The
   * slot is reserved before the invoke is awaited, so overlapping calls never
   * pass the bound, and one request id is never in flight twice. */
  async function run(call: TeamCall, source = ""): Promise<TeamOutcome> {
    if (sending().includes(call.request_id)) return { outcome: "pending", request_id: call.request_id };
    if (occupied(call.request_id) >= UNRESOLVED_MAX) return refuseFull(call, source);
    return send(call, source, () => transport.act(call));
  }
  /** Check: the same request id and content, sent again because the person asked. */
  async function runAgain(call: TeamCall, source = ""): Promise<TeamOutcome> {
    if (sending().includes(call.request_id)) return { outcome: "pending", request_id: call.request_id };
    if (!unresolvedOf(call.request_id) && occupied() >= UNRESOLVED_MAX) return refuseFull(call, source);
    return send(call, source, () => transport.actAgain(call));
  }
  async function send(call: TeamCall, source: string, act: () => Promise<TeamOutcome>): Promise<TeamOutcome> {
    setSending([...sending(), call.request_id]);
    try {
      const outcome = await act();
      return accept(call, outcome, source);
    } catch (error) {
      const detail = errorText(error);
      putUnresolved(call, detail, "uncertain");
      return { outcome: "uncertain", detail, request_id: call.request_id };
    } finally {
      setSending(sending().filter((id) => id !== call.request_id));
    }
  }
  function accept(call: TeamCall, outcome: TeamOutcome, source: string): TeamOutcome {
    switch (outcome?.outcome) {
      case "confirm": setConfirmation({ call: { ...call, confirmed: false }, confirmation: outcome.confirmation }); return outcome;
      case "pending": putUnresolved(call, "The action waits for the team.", "pending"); return outcome;
      case "done": settled(call.request_id); return outcome;
      case "failed":
        settled(call.request_id);
        failAction(source, outcome.code ?? "", outcome.detail);
        return outcome;
      case "uncertain": putUnresolved(call, outcome.detail, "uncertain"); return outcome;
      default: {
        // An answer of no known shape is an unknown result, never a success.
        const detail = "The action's answer could not be read. Check it or dismiss it.";
        putUnresolved(call, detail, "uncertain");
        return { outcome: "uncertain", detail, request_id: call.request_id };
      }
    }
  }
  async function confirm(source = "") {
    const asking = confirmation();
    if (!asking) return;
    setConfirmation(null);
    await run({ ...asking.call, confirmed: true }, source);
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  function cancelConfirmation() { setConfirmation(null); }
  async function check(entry?: UnresolvedCall, source = "") {
    const asking = entry ?? unresolved()[0];
    if (!asking) return;
    await runAgain(asking.call, source);
  }
  /** Dismiss settles the entry only when the core took it; a refusal or a
   * failed call keeps the entry and names the reason. */
  async function dismiss(entry?: UnresolvedCall, source = ""): Promise<boolean> {
    const asking = entry ?? unresolved()[0];
    if (!asking) return false;
    if (sending().includes(asking.call.request_id)) return false;
    try {
      await transport.dismiss(asking.call);
      settled(asking.call.request_id);
      return true;
    } catch (error) {
      failAction(source, "dismiss_failed", errorText(error));
      return false;
    }
  }
  /** The first unresolved call, for a view that shows one at a time. */
  const uncertain = () => {
    const first = unresolved()[0];
    return first ? { call: first.call, detail: first.detail } : null;
  };

  /** The first list page, for the views that show the list. */
  const list = () => listPagesBy().get("")?.view;
  /** The summaries of the scanned pages (bounded), for lookups by id or project. */
  const listPages = listed;
  /** The next page after the kept chain: null when it is complete or its
   * cursors repeat. */
  const listNext = () => chainOf(listPagesBy()).next;

  return {
    list, listed, listPages, listNext, team, failure, available, off, followed, visible, catalog, catalogError, newTeamOpen, confirmation, unresolved, uncertain, busy, sending,
    waitingView, waitingCount, waitingLater, apply, probe, recheck, hidden, openTeam, closeTeam, tasksChanged, page, openWaiting, pageWaiting, firstWaitingPage, decide, revisionOf,
    teamOfSession, summaryOf, unresolvedOf, failureFor, readFailureFor, preview, openNewTeam, closeNewTeam, reloadCatalog, createIntent, claimCreate, saveTemplate, project, run, runAgain, confirm, cancelConfirmation, check, dismiss, clearFailure,
  };
}

/** The application's Teams store. */
export const teams = createRoot(() => createTeams());
