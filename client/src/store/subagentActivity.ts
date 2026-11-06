import { batch, createEffect, createMemo, createSignal, on, onCleanup, untrack } from "solid-js";
import type { SubagentInspection } from "../bridge";
import type { ActivitySource } from "./activitySource";
import type { ActivityEvent, ComputerId } from "../types";
import { tr } from "../i18n";

export interface SubagentActivityTarget {
  /** The parent row id, used by the Activity's file-reference dialog. */
  id: string;
  computer: ComputerId;
  name: string;
  runId: string;
  conversationId: string;
  archiveId: string | null;
  agentId: string;
}

export interface SubagentActivityDeps {
  read(target: SubagentActivityTarget): Promise<SubagentInspection | null>;
  /** The window shows and the Sessions page is current (reactive). */
  showing(): boolean;
  /** The computer is online (reactive). */
  online?(computer: ComputerId): boolean;
  later?(callback: () => void, ms: number): () => void;
}

export const SUBAGENT_POLL_MS = 2500;
/** Above the native read's own bound (15 s to a peer plus the 2 s drain), so only a lost answer reaches it. */
export const SUBAGENT_READ_DEADLINE_MS = 20_000;
const same = (a: SubagentActivityTarget | undefined, b: SubagentActivityTarget | undefined) =>
  a === b || (!!a && !!b && a.id === b.id && a.computer === b.computer && a.name === b.name && a.runId === b.runId
    && a.conversationId === b.conversationId && a.archiveId === b.archiveId && a.agentId === b.agentId);

/**
 * One replacement page of one exact child; no live parent state or growing
 * history. The Activity shows "Reading activity…" until `loaded`, so every way
 * of not getting a page while it shows ends loaded, with its reason in `error`.
 */
export function createSubagentActivity(deps: SubagentActivityDeps) {
  const later = deps.later ?? ((callback: () => void, ms: number) => { const timer = setTimeout(callback, ms); return () => clearTimeout(timer); });
  const [target, setTarget] = createSignal<SubagentActivityTarget | undefined>();
  const [events, setEvents] = createSignal<readonly ActivityEvent[]>([]);
  const [summary, setSummary] = createSignal<Record<string, unknown> | undefined>();
  const [loaded, setLoaded] = createSignal(false);
  const [error, setError] = createSignal<string | undefined>();
  const reachable = createMemo(() => { const t = target(); return !!t && (deps.online?.(t.computer) ?? true); });
  let generation = 0, reading = false, showing = false;
  let cancel: (() => void) | undefined;

  const stopTimer = () => { cancel?.(); cancel = undefined; };
  const schedule = () => {
    stopTimer();
    cancel = later(() => { cancel = undefined; read(); }, SUBAGENT_POLL_MS);
  };
  function drop() {
    generation++;
    reading = false;
    stopTimer();
    batch(() => { setEvents([]); setSummary(undefined); setLoaded(false); setError(undefined); });
  }
  function read() {
    const t = untrack(target);
    if (!t || !showing || reading) return;
    if (!untrack(reachable)) {
      // Nothing is read until it returns, and what was read stays.
      batch(() => { setError(tr("Could not refresh activity: the computer is offline")); setLoaded(true); });
      return;
    }
    const mine = generation;
    reading = true;
    stopTimer();
    cancel = later(() => {
      cancel = undefined;
      generation++; // Its answer, should one still come, is no longer wanted.
      reading = false;
      batch(() => { setError(tr("Could not refresh activity: the read did not finish")); setLoaded(true); });
      schedule();
    }, SUBAGENT_READ_DEADLINE_MS);
    void deps.read(t).then((answer) => {
      if (mine !== generation) return;
      reading = false;
      if (!answer || answer.agent_id !== t.agentId || answer.parent_conversation_id !== t.conversationId || answer.run_id !== t.runId) {
        setError(tr("Could not refresh activity: the subagent changed or is unavailable"));
        setLoaded(true);
      } else {
        batch(() => { setEvents(answer.events); setSummary(answer); setError(undefined); setLoaded(true); });
      }
      schedule();
    }).catch((reason: unknown) => {
      if (mine !== generation) return;
      reading = false;
      batch(() => { setError(tr("Could not refresh activity: %1", String(reason))); setLoaded(true); });
      schedule();
    });
  }
  function follow(next: SubagentActivityTarget | undefined) {
    if (same(untrack(target), next)) return;
    drop();
    setTarget(next);
    read();
  }
  createEffect(on(deps.showing, (show) => {
    showing = show;
    if (!show) drop();
    else read();
  }));
  createEffect(on(reachable, read, { defer: true }));
  onCleanup(() => { generation++; stopTimer(); });

  return {
    target, events, summary, loaded, error,
    // target().id is the parent's row, as the file dialog needs it. The Activity
    // resets its layout and sent-message projection when this changes, so each
    // child's page stays its own and no child inherits its parent's.
    displayKey: () => { const t = target(); return t ? JSON.stringify([t.id, t.agentId]) : undefined; },
    attachmentScope: () => {
      const t = target();
      return t ? { computer: t.computer, name: t.name, conversation_id: t.conversationId, archive_id: t.archiveId, agent_id: t.agentId } : undefined;
    },
    loadingOlder: () => false,
    olderDone: () => true,
    ended: () => false,
    loadOlder: () => {},
    readOnly: true as const,
    follow,
  } satisfies ActivitySource & { follow(next: SubagentActivityTarget | undefined): void };
}
