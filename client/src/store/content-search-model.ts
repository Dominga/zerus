import { batch, createSignal } from "solid-js";
import type { Session } from "../ui/sessions/types";
import type { ActivityEvent, ViewRow } from "../types";
import { tr, trNoop, type TranslationArgument } from "../i18n";

export const SEARCH_DELAY = 350;
export const SEARCH_HOSTS = 32;
export const SEARCH_HITS = 200;
export const SEARCH_ROWS = 500;
export interface SearchHit {
  name: string; run_id: string; conversation_id: string; archive_id: string; message_id: string;
  at: number; role: string; snippet: string; event: ActivityEvent; content_truncated: boolean;
  session: ViewRow | null; group: string;
}
/** `index_state` is the content index's coverage (core content_search.rs; empty without an index). */
export interface SearchReply { query: string; results: SearchHit[]; truncated: boolean; unavailable_sessions: number; contents_unavailable?: boolean; index_state?: string }
export interface SearchHost { computer: string; machine: string; online: boolean }
export interface FoundHit { computer: string; machine: string; hit: SearchHit }
export interface SearchPort {
  read(computer: string, query: string, generation: number): Promise<SearchReply>;
  cancel(through: number): void;
  later?(run: () => void, ms: number): () => void;
  firstGeneration?: number;
}

/**
 * Why a host's contents are partial (task 77): the index is unavailable or
 * still building, so hgs searched recent history; some sessions have no
 * readable history (no transcript, or one that was removed). A bare
 * "contents unavailable" stays for a reply that names no reason.
 */
export function coverageNotes(machine: string, reply: Pick<SearchReply, "unavailable_sessions" | "contents_unavailable" | "index_state">): string[] {
  const notes: string[] = [];
  if (reply.index_state === "unavailable") notes.push(tr("%1: content index unavailable, recent history searched instead", machine));
  else if (reply.index_state && reply.index_state !== "ready") notes.push(tr("%1: content index still building, recent history searched too", machine));
  if (reply.unavailable_sessions > 0) notes.push(tr("%1: history unavailable in %2 of the searched sessions", machine, reply.unavailable_sessions));
  if (!notes.length && reply.contents_unavailable) notes.push(`${machine}: contents unavailable`);
  return notes;
}

/** One query, one bounded result window. Old jobs still consume their lane
 * until they settle, so fast typing never grows concurrent CLI reads. */
export function createContentSearch(port: SearchPort) {
  const [hits, setHits] = createSignal<FoundHit[]>([]);
  const [errors, setErrors] = createSignal<string[]>([]);
  const [busy, setBusy] = createSignal(false);
  const [limited, setLimited] = createSignal(false);
  const [unavailable, setUnavailable] = createSignal(0);
  const later = port.later ?? ((run, ms) => { const timer = setTimeout(run, ms); return () => clearTimeout(timer); });
  let generation = port.firstGeneration ?? Date.now();
  let key = "", query = "", hosts: SearchHost[] = [], active = false;
  let queue: SearchHost[] = [];
  let running = 0, current = 0;
  let timer: (() => void) | undefined;
  const updateBusy = () => setBusy(!!timer || queue.length > 0 || current > 0);
  function drain() {
    while (running < 4 && queue.length) {
      const host = queue.shift()!, token = generation, text = query;
      running++; current++;
      void Promise.resolve().then(() => port.read(host.computer, text, token)).then((reply) => {
        if (token !== generation || reply.query !== query) return;
        const room = SEARCH_HITS - hits().length;
        batch(() => {
          setLimited((old) => old || reply.truncated || reply.results.length > room);
          setUnavailable((old) => Math.min(1_000_000, old + reply.unavailable_sessions));
          const notes = coverageNotes(host.machine, reply);
          if (notes.length) setErrors((old) => [...old, ...notes].slice(0, SEARCH_HOSTS));
          setHits((old) => [...old, ...reply.results.slice(0, room).map((hit) => ({ computer: host.computer, machine: host.machine, hit }))]);
        });
      }).catch((error) => {
        if (token === generation && String(error) !== "superseded") setErrors((old) => [...old, `${host.machine}: ${String(error).slice(0, 240)}`].slice(0, SEARCH_HOSTS));
      }).finally(() => { running--; if (token === generation) current--; drain(); updateBusy(); });
    }
    updateBusy();
  }
  function configure(nextQuery: string, nextHosts: SearchHost[], nextActive: boolean, force = false) {
    const signature = JSON.stringify([nextQuery.trim(), nextHosts, nextActive]);
    if (key === signature && !force) return;
    key = signature; port.cancel(generation++); timer?.(); timer = undefined;
    query = nextQuery.trim(); hosts = nextHosts.slice(0, SEARCH_HOSTS); active = nextActive;
    queue = []; current = 0;
    batch(() => { setHits([]); setErrors([]); setUnavailable(0); setLimited(nextHosts.length > SEARCH_HOSTS); setBusy(false); });
    if (!active || [...query].length < 2) return;
    if ([...query].length > 256 || /\p{Cc}/u.test(query)) { setErrors([tr("Search requires 2..256 printable characters.")]); return; }
    timer = later(() => {
      timer = undefined;
      setErrors(hosts.filter((h) => !h.online).map((h) => `${h.machine}: machine offline; contents unavailable`));
      queue = hosts.filter((h) => h.online); drain();
    }, SEARCH_DELAY);
    updateBusy();
  }
  return { hits, errors, busy, limited, unavailable, configure,
    refresh: () => configure(query, hosts, active, true),
    dispose: () => { timer?.(); timer = undefined; queue = []; current = 0; setBusy(false); setHits([]); port.cancel(generation++); },
  };
}

export function identityMatches(row: Session | undefined, hit: FoundHit["hit"]): row is Session {
  return !!row && row.name === hit.name && (!hit.run_id || row.runId === hit.run_id)
    && (!hit.conversation_id || row.conversationId === hit.conversation_id) && (row.archiveId ?? "") === hit.archive_id;
}
export function metadataMatches(row: Session, query: string): boolean {
  const needle = query.toLowerCase();
  return [row.name, row.title, row.machine, row.model, row.effort, row.cwdPath, row.branch, row.worktree,
    row.conversationId, row.runId, row.archiveId, ...(row.subagents ?? []).flatMap((s) => [s.id, s.conversationId])]
    .some((text) => text?.toLowerCase().includes(needle));
}

/**
 * The first identity of `row` that contains `query`, as "Label: ID", in Qt's
 * order (SessionSearch.cpp:239-268): session, run and archive IDs, then each
 * child's agent ID and its own conversation ID. Undefined when only other
 * metadata matched.
 */
export function matchedIdentity(row: Session, query: string): string | undefined {
  const needle = query.toLowerCase();
  const ids: [string | undefined, string][] = [
    [row.conversationId, trNoop("Session ID")], [row.runId, trNoop("Run ID")], [row.archiveId, trNoop("Archive ID")],
    ...(row.subagents ?? []).flatMap((s): [string | undefined, string][] => [[s.id, trNoop("Agent ID")], [s.conversationId, trNoop("Child session ID")]]),
  ];
  const found = ids.find(([id]) => !!id && id.toLowerCase().includes(needle));
  return found ? `${tr(found[1])}: ${found[0]}` : undefined;
}

/** At most this many message rows per session (zerus-5wmh.13.17.1). */
export const SESSION_MESSAGES = 20;

/**
 * One row per matching message (SessionSearch.cpp:276-282, zerus-5wmh.13.17.1).
 * A session's rows stay together: its metadata row when its metadata or IDs
 * matched, then its message rows newest first, at most `perSession`; sessions
 * keep the order they were first seen in, so a later host reply only adds
 * rows to its sessions' groups and every row keeps its id. A message listed
 * twice shows once, and a session's message rows carry its matched identity
 * for their tooltip. `limited` says a cap left rows out.
 */
export function searchRows<T extends { id: string; session: Session; found?: FoundHit; matched?: string }>(
  entries: readonly T[], caps: { rows: number; perSession: number } = { rows: SEARCH_ROWS, perSession: SESSION_MESSAGES },
): { rows: T[]; limited: boolean } {
  const groups = new Map<string, { metadata?: T; messages: T[]; ids: Set<string> }>();
  for (const entry of entries) {
    let group = groups.get(entry.session.key);
    if (!group) { group = { messages: [], ids: new Set() }; groups.set(entry.session.key, group); }
    if (!entry.found) { group.metadata ??= entry; continue; }
    if (group.ids.has(entry.id)) continue;
    group.ids.add(entry.id);
    group.messages.push(entry);
  }
  const rows: T[] = [];
  let limited = false;
  for (const group of groups.values()) {
    const messages = [...group.messages].sort((a, b) => b.found!.hit.at - a.found!.hit.at || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
    if (messages.length > caps.perSession) { limited = true; messages.length = caps.perSession; }
    const matched = group.metadata?.matched;
    for (const row of [...(group.metadata ? [group.metadata] : []), ...messages.map((m) => (matched && !m.matched ? { ...m, matched } : m))]) {
      if (rows.length >= caps.rows) return { rows, limited: true };
      rows.push(row);
    }
  }
  return { rows, limited };
}
/**
 * A result row's tooltip, as Qt's (SessionSearch.cpp:257-262): the session's
 * name, title and machine, its IDs, the identity that matched when not
 * already listed, and the message excerpt.
 */
export function searchTooltip(item: { session: Session; found?: FoundHit; matched?: string }): string {
  const s = item.session;
  const lines = [s.name, s.title !== s.name ? s.title : "", s.machine,
    s.conversationId ? tr("Session ID: %1", s.conversationId) : "", s.runId ? tr("Run ID: %1", s.runId) : "",
    s.archiveId ? tr("Archive ID: %1", s.archiveId) : ""].filter(Boolean);
  if (item.matched && !lines.includes(item.matched)) lines.push(item.matched);
  if (item.found?.hit.snippet) lines.push(item.found.hit.snippet);
  return lines.join("\n");
}
type Translate = (source: string, ...args: TranslationArgument[]) => string;
const MESSAGES = trNoop("%n message(s)"), SESSIONS = trNoop("%n session(s)"), CREATIONS = trNoop("%n creation(s)");

/** One count in the interface language with its plural form; English keeps "1 message" and "2 messages". */
function counted(translate: Translate, count: number, plural: string, one: string, many: string): string {
  const text = translate(plural, count);
  return text === plural.replace("%n", String(count)) ? `${count} ${count === 1 ? one : many}` : text;
}

/** "3 messages, 2 sessions" and, when any match, "1 creation"; each count translated with its plural form (zerus-5wmh.13.17.3.1). */
export function searchCount(messages: number, sessions: number, creations = 0, translate: Translate = tr): string {
  const parts = [counted(translate, messages, MESSAGES, "message", "messages"), counted(translate, sessions, SESSIONS, "session", "sessions")];
  if (creations) parts.push(counted(translate, creations, CREATIONS, "creation", "creations"));
  return parts.join(", ");
}
