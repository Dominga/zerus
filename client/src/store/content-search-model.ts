import { batch, createSignal } from "solid-js";
import type { Session } from "../ui/sessions/types";
import type { ActivityEvent, ViewRow } from "../types";
import { tr, trNoop } from "../i18n";

export const SEARCH_DELAY = 350;
export const SEARCH_HOSTS = 32;
export const SEARCH_HITS = 200;
export const SEARCH_ROWS = 500;
export interface SearchHit {
  name: string; run_id: string; conversation_id: string; archive_id: string; message_id: string;
  at: number; role: string; snippet: string; event: ActivityEvent; content_truncated: boolean;
  session: ViewRow | null; group: string;
}
export interface SearchReply { query: string; results: SearchHit[]; truncated: boolean; unavailable_sessions: number; contents_unavailable?: boolean }
export interface SearchHost { computer: string; machine: string; online: boolean }
export interface FoundHit { computer: string; machine: string; hit: SearchHit }
export interface SearchPort {
  read(computer: string, query: string, generation: number): Promise<SearchReply>;
  cancel(through: number): void;
  later?(run: () => void, ms: number): () => void;
  firstGeneration?: number;
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
          if (reply.contents_unavailable || reply.unavailable_sessions) setErrors((old) => [...old, `${host.machine}: contents unavailable`].slice(0, SEARCH_HOSTS));
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

/** A session keeps its position and row identity as metadata and host replies
 * arrive. The newest matching message supplies its single excerpt. */
export function mergeSessionMatch<T extends { id: string; session: Session; found?: FoundHit; matched?: string }>(items: T[], positions: Map<string, number>, item: T, cap: number): boolean {
  const index = positions.get(item.session.key);
  if (index !== undefined) {
    const previous = items[index];
    // The identity that matched stays with the row a message hit refreshes.
    if (item.found && (!previous.found || item.found.hit.at > previous.found.hit.at)) items[index] = { ...item, id: previous.id, matched: item.matched ?? previous.matched };
    else if (!previous.matched && item.matched) items[index] = { ...previous, matched: item.matched };
    return true;
  }
  if (items.length >= cap) return false;
  positions.set(item.session.key, items.length); items.push(item); return true;
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
export function searchCount(messages: number, sessions: number): string {
  return `${messages} ${messages === 1 ? "message" : "messages"}, ${sessions} ${sessions === 1 ? "session" : "sessions"}`;
}
