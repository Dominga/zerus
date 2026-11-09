import { comboFromEvent, matchSequence, type Sequence } from "../../store/shortcuts";
import { identityMatches, type FoundHit } from "../../store/content-search-model";
import type { Session } from "./types";

export const GoToKind = { Session: 0, Project: 1, Account: 2, Machine: 3, Section: 4, Command: 5, Message: 6 } as const;
export type GoToKind = (typeof GoToKind)[keyof typeof GoToKind];

export interface GoToItem {
  kind: GoToKind;
  id: string;
  host: string;
  title: string;
  detail: string | readonly string[];
  chord?: string;
  recency: number;
  /** Task 423: text the search matches without showing it: a session's name and IDs. */
  keys?: readonly string[];
}

export const goToKindLabel = (kind: GoToKind) => ["SESSION", "PROJECT", "ACCOUNT", "MACHINE", "SECTION", "COMMAND", "MESSAGE"][kind];
export const goToDetails = (detail: GoToItem["detail"]) => (Array.isArray(detail) ? detail : [detail]).filter(Boolean);

let recent: string[] = [];
export function rememberRecentSession(key: string | undefined) {
  if (!key) return;
  recent = [key, ...recent.filter((other) => other !== key)].slice(0, 20);
}

export const recentSessionIds = () => [...recent];

export function goToScore(query: string, text: string): number {
  if (!query) return 0;
  const q = query.toLowerCase(), t = text.toLowerCase();
  const none = Number.MIN_SAFE_INTEGER;
  let best = new Array<number>(t.length).fill(none);
  for (let i = 0; i < q.length; i++) {
    const next = new Array<number>(t.length).fill(none);
    let any = false;
    for (let j = 0; j < t.length; j++) {
      if (t[j] !== q[i]) continue;
      let points = 10;
      if (i === 0 && j === 0) points += 100;
      if (j === 0 || !/[\p{L}\p{N}]/u.test(t[j - 1])) points += 20;
      let base = none;
      if (i === 0) base = 0;
      else {
        for (let k = 0; k < j; k++) {
          if (best[k] === none) continue;
          const gap = j - k - 1;
          base = Math.max(base, best[k] + (gap === 0 ? 10 : -Math.min(gap, 8)));
        }
      }
      if (base === none) continue;
      next[j] = base + points;
      any = true;
    }
    if (!any) return -1;
    best = next;
  }
  return Math.max(...best);
}

export function rankGoTo(items: readonly GoToItem[], query: string): GoToItem[] {
  let q = query.trim();
  const commandsOnly = q.startsWith(">");
  if (commandsOnly) q = q.slice(1).trim();
  const entries = items.flatMap((item, order) => {
    if (commandsOnly && item.kind !== GoToKind.Command && item.kind !== GoToKind.Section) return [];
    // Task 423: names and IDs match as substrings (a pasted ID, part of one),
    // never by the fuzzy score, which is slow on long IDs.
    const shown = goToScore(q, `${item.title} ${goToDetails(item.detail).join(" ")}`);
    const lower = q.toLowerCase();
    const keyed = q && item.keys?.some((key) => key.toLowerCase().includes(lower)) ? 50 + 10 * q.length : -1;
    const score = Math.max(shown, keyed);
    return score < 0 ? [] : [{ item, score, order }];
  });
  entries.sort((a, b) => {
    if (q) return b.score - a.score || a.item.kind - b.item.kind || a.order - b.order;
    const ar = a.item.kind === GoToKind.Session && a.item.recency >= 0;
    const br = b.item.kind === GoToKind.Session && b.item.recency >= 0;
    if (ar !== br) return ar ? -1 : 1;
    return ar ? a.item.recency - b.item.recency : a.order - b.order;
  });
  return entries.map((entry) => entry.item);
}

export function visibleGoToRows(items: readonly GoToItem[], current: number, limit = 12, previousStart = 0): { start: number; rows: GoToItem[] } {
  const maxStart = Math.max(0, items.length - limit);
  const start = Math.max(0, Math.min(current < previousStart ? current : current >= previousStart + limit ? current - limit + 1 : previousStart, maxStart));
  return { start, rows: items.slice(start, start + limit) };
}

export type GoToKey = "up" | "down" | "pageUp" | "pageDown" | "activate" | "close" | "hold" | "none";
export interface GoToKeyState { typed: Sequence }

export function goToKey(event: Parameters<typeof comboFromEvent>[0] & { isComposing?: boolean; keyCode?: number }, state: GoToKeyState, closeKeys: Sequence[]): GoToKey {
  if (event.isComposing || event.keyCode === 229) return "none";
  const combo = comboFromEvent(event);
  if (combo) {
    const attempt = [...state.typed, combo];
    state.typed = [];
    let partial = false;
    for (const key of closeKeys) {
      const match = matchSequence(key, attempt);
      if (match === "full") return "close";
      if (match === "partial") partial = true;
    }
    if (partial && attempt.length < 4) {
      state.typed = attempt;
      return "hold";
    }
  }
  switch (event.key) {
    case "ArrowUp": return "up";
    case "ArrowDown": return "down";
    case "PageUp": return "pageUp";
    case "PageDown": return "pageDown";
    case "Enter": return "activate";
    case "Escape": return "close";
    default: return "none";
  }
}

/**
 * Task 423: the title-bar search opens in place. The palette's box from the
 * search field's rect: its left edge and top on the field's, at least the
 * field's width (and a readable minimum), kept inside the window with a margin.
 */
export function goToFrame(field: { left: number; top: number; width: number; height: number }, viewport: { width: number; height: number }, minimum = 560, margin = 8) {
  const room = Math.max(0, viewport.width - 2 * margin);
  const width = Math.min(room, Math.max(field.width, minimum));
  const left = Math.max(margin, Math.min(field.left, viewport.width - margin - width));
  const top = Math.max(0, Math.min(field.top, viewport.height - margin));
  return { left, top, width, height: field.height };
}

/** A message the palette lists: the live session it belongs to and the hit. */
export interface GoToMessage { item: GoToItem; row: Session; found: FoundHit }

/** Palette rows for the message hits whose session is on the list (archived hits stay in the panel's search). */
export function goToMessageRows(hits: readonly FoundHit[], row: (computer: string, hit: FoundHit["hit"]) => Session | undefined, limit = 50): GoToMessage[] {
  const rows: GoToMessage[] = [];
  for (const found of hits) {
    if (rows.length >= limit) break;
    if (found.hit.archive_id) continue;
    const session = row(found.computer, found.hit);
    if (!identityMatches(session, found.hit)) continue;
    rows.push({
      row: session, found,
      item: { kind: GoToKind.Message, id: `${session.key}\n${found.hit.message_id}`, host: session.computer, title: session.title,
        detail: [found.hit.snippet, found.machine].filter(Boolean), recency: -1 },
    });
  }
  return rows;
}
