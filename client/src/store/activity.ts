import { batch, createSignal } from "solid-js";
import type { ActivityEvent, ActivityOut, ComputerId } from "../types";

// The followed session's Activity: its events (oldest first, one per journal
// row), the latest inspection summary (`state`) and whether the subscription
// ended. Events arrive as `events` (append), `replace` (a new window, from
// DeepSeek and OpenCode or after a gap), `event` (replace by id, higher rev
// wins), `state` and `ended`. Only the selected session is followed.

export interface FollowTarget {
  /** The catalog row id (computer + "\n" + key). */
  id: string;
  computer: ComputerId;
  key: string;
  name: string;
  runId?: string;
  conversationId?: string;
}

export interface FollowRequest {
  computer: ComputerId;
  key: string;
  name: string;
  run_id?: string;
  conversation_id?: string;
  after?: number;
  tail?: number;
}

export interface ActivityPort {
  follow(request: FollowRequest): Promise<void>;
  unfollow(computer: ComputerId, key: string): Promise<void>;
}

/** The newest events a subscription starts with (the server allows 100). */
export const TAIL = 100;
/** The first older read reaches this many journal rows back; empty reads double it. */
const OLDER_SPAN = 200;
/** A read that brings nothing older within this time counts as empty. */
const OLDER_TIMEOUT_MS = 4000;

/** A stable identity for an event: its journal seq, else its own keys. */
export function eventKey(e: ActivityEvent): string {
  if (typeof e.seq === "number" && e.seq > 0) return `s${e.seq}`;
  const voice = e.voice && typeof e.voice.id === "string" ? e.voice.id : "";
  if (e.source === "hgs_voice" && (voice || e.message_id)) return `v${voice || e.message_id}`;
  if (e.activity_key) return `k${e.activity_key}`;
  if (e.source === "hgs_delivery" && e.message_id) return `r${e.message_id}`;
  return `t${e.type ?? ""}:${e.at ?? ""}:${(e.detail ?? "").slice(0, 48)}`;
}

/** An event's journal seq, when it has one (above 0). */
export const seqOf = (e: ActivityEvent) => (typeof e.seq === "number" && e.seq > 0 ? e.seq : undefined);

function payloadEvents(payload: unknown): { events: ActivityEvent[]; cursor?: number } {
  if (Array.isArray(payload)) return { events: payload as ActivityEvent[] };
  const p = (payload ?? {}) as { events?: unknown; cursor?: unknown };
  return { events: Array.isArray(p.events) ? (p.events as ActivityEvent[]) : [], cursor: typeof p.cursor === "number" ? p.cursor : undefined };
}

/** Orders events by journal seq; events without one keep their arrival place. */
function ordered(map: Map<string, ActivityEvent>): ActivityEvent[] {
  const list = [...map.values()];
  if (list.every((e) => seqOf(e) !== undefined)) list.sort((a, b) => seqOf(a)! - seqOf(b)!);
  return list;
}

export function createActivity(port: ActivityPort) {
  const [target, setTarget] = createSignal<FollowTarget | undefined>();
  const [events, setEvents] = createSignal<readonly ActivityEvent[]>([]);
  const [summary, setSummary] = createSignal<Record<string, unknown> | undefined>();
  const [ended, setEnded] = createSignal(false);
  const [loaded, setLoaded] = createSignal(false);
  const [loadingOlder, setLoadingOlder] = createSignal(false);
  const [olderDone, setOlderDone] = createSignal(false);
  /** Why the last follow failed (unknown computer, too many followed sessions, ...). */
  const [error, setError] = createSignal<string | undefined>();

  let byKey = new Map<string, ActivityEvent>();
  let stateRev = -1;
  const revs = new Map<string, number>();
  let span = OLDER_SPAN;
  let olderFrom: number | undefined;
  let olderTimer: ReturnType<typeof setTimeout> | undefined;

  const oldestSeq = () => {
    let oldest: number | undefined;
    for (const e of byKey.values()) {
      const s = seqOf(e);
      if (s !== undefined && (oldest === undefined || s < oldest)) oldest = s;
    }
    return oldest;
  };

  const reset = () => {
    byKey = new Map();
    stateRev = -1;
    revs.clear();
    span = OLDER_SPAN;
    olderFrom = undefined;
    clearTimeout(olderTimer);
    batch(() => {
      setEvents([]);
      setSummary(undefined);
      setEnded(false);
      setLoaded(false);
      setLoadingOlder(false);
      setOlderDone(false);
      setError(undefined);
    });
  };

  /** The exact session a subscription follows; its key must stay the catalog key. */
  const request = (t: FollowTarget, after?: number): FollowRequest => ({
    computer: t.computer, key: t.key, name: t.name,
    run_id: t.runId ?? (t.key.startsWith("run:") ? t.key.slice(4) : undefined),
    conversation_id: t.conversationId, after, tail: TAIL,
  });

  /** Follows one session (the selected one); following another unfollows the previous. */
  function follow(next: FollowTarget | undefined) {
    const prev = target();
    if (prev?.id === next?.id) return;
    if (prev) void port.unfollow(prev.computer, prev.key).catch(() => {});
    reset();
    setTarget(next);
    if (next) void port.follow(request(next)).catch((reason) => batch(() => { setError(String(reason)); setLoaded(true); }));
  }

  /** The finish of an older read: it either moved the oldest event back or doubles the reach. */
  const settleOlder = () => {
    if (olderFrom === undefined) return;
    clearTimeout(olderTimer);
    const oldest = oldestSeq();
    const asked = Math.max(0, olderFrom - span);
    if (oldest === undefined || oldest >= olderFrom) {
      if (asked === 0) setOlderDone(true);
      span *= 2;
    }
    olderFrom = undefined;
    setLoadingOlder(false);
  };

  /** Reads events before the oldest one shown, resuming the subscription from further back. */
  function loadOlder() {
    const t = target();
    const oldest = oldestSeq();
    if (!t || loadingOlder() || olderDone() || oldest === undefined) return;
    if (oldest <= 1) { setOlderDone(true); return; }
    olderFrom = oldest;
    setLoadingOlder(true);
    const after = Math.max(0, oldest - span);
    olderTimer = setTimeout(settleOlder, OLDER_TIMEOUT_MS);
    void port.follow(request(t, after)).catch(settleOlder);
  }

  function applyActivity(outs: ActivityOut[]) {
    const t = target();
    if (!t) return;
    let changed = false;
    let older = false;
    batch(() => {
      for (const out of outs) {
        if (out.computer !== t.computer || out.key !== t.key) continue;
        switch (out.kind) {
          case "events": {
            const { events: list } = payloadEvents(out.payload);
            for (const e of list) byKey.set(eventKey(e), e);
            changed = true;
            older = true;
            setLoaded(true);
            break;
          }
          case "replace": {
            const { events: list } = payloadEvents(out.payload);
            byKey = new Map(list.map((e) => [eventKey(e), e]));
            changed = true;
            setLoaded(true);
            // A replaced window has no older pages to read by seq.
            if (oldestSeq() === undefined) setOlderDone(true);
            break;
          }
          case "event": {
            const p = (out.payload ?? {}) as { id?: string; rev?: number; event?: ActivityEvent };
            if (!p.event || !p.id) break;
            const rev = typeof p.rev === "number" ? p.rev : 0;
            if ((revs.get(p.id) ?? -1) >= rev) break;
            revs.set(p.id, rev);
            const existing = [...byKey.entries()].find(([, e]) => e.id === p.id || e.activity_key === p.id);
            byKey.set(existing ? existing[0] : `i${p.id}`, p.event);
            changed = true;
            break;
          }
          case "state": {
            const p = (out.payload ?? {}) as { value?: Record<string, unknown>; rev?: number };
            const rev = typeof p.rev === "number" ? p.rev : stateRev + 1;
            if (rev < stateRev) break;
            stateRev = rev;
            setSummary(p.value);
            break;
          }
          case "ended":
            setEnded(true);
            setLoaded(true);
            break;
        }
      }
      if (changed) setEvents(ordered(byKey));
    });
    if (older && olderFrom !== undefined) settleOlder();
  }

  /** The followed session's conversation: its inspection's, else the row's (Qt's m_details conversation_id). */
  const attachmentScope = () => {
    const t = target();
    if (!t) return undefined;
    const inspected = summary()?.conversation_id;
    const conversation = typeof inspected === "string" && inspected ? inspected : t.conversationId ?? "";
    return { computer: t.computer, name: t.name, conversation_id: conversation, archive_id: null };
  };

  return { target, events, summary, ended, loaded, loadingOlder, olderDone, error, follow, loadOlder, applyActivity, attachmentScope };
}

export type ActivityStore = ReturnType<typeof createActivity>;
