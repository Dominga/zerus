import type { ReadingAnchor } from "./activity-anchor";
import { eventKey, seqOf } from "../../store/activity";
import type { ActivityEvent } from "../../types";

/** A saved place is named by rows and journal seqs, never by an absolute
 * scroll offset: the transcript around it can change before it returns. */
export interface ActivityPosition {
  following: boolean;
  anchors: readonly ReadingAnchor[];
  /** Read immediately before the visible row if it has left the new tail. */
  after?: number;
  /** The top saved row's journal seq. If no saved row returns, Activity
   * places the nearest surviving row at or before it, else follows Latest. */
  seq?: number;
  expanded: Record<string, boolean>;
  more: Record<string, number>;
}

/** Each row key's journal seq, from its oldest event. */
function rowSeqs(events: readonly ActivityEvent[]) {
  const seqs = new Map<string, number>();
  for (const event of events) {
    const seq = seqOf(event), key = `item-${event.activity_display_key ?? eventKey(event)}`;
    const known = seqs.get(key);
    if (seq !== undefined && (known === undefined || seq < known)) seqs.set(key, seq);
  }
  return seqs;
}

/** The journal seq of the topmost saved row that names one. */
export function activityAnchorSeq(events: readonly ActivityEvent[], anchors: readonly ReadingAnchor[]): number | undefined {
  const seqs = rowSeqs(events);
  for (const anchor of anchors) {
    const seq = seqs.get(anchor.key);
    if (seq !== undefined) return seq;
  }
  return undefined;
}

/** The shown row whose journal seq is nearest at or before `seq`, if any. */
export function activityNearestRow(events: readonly ActivityEvent[], rows: readonly string[], seq: number | undefined): string | undefined {
  if (seq === undefined) return undefined;
  const seqs = rowSeqs(events);
  let nearest: string | undefined, best = -Infinity;
  for (const key of rows) {
    const rowSeq = seqs.get(key);
    if (rowSeq !== undefined && rowSeq <= seq && rowSeq > best) { nearest = key; best = rowSeq; }
  }
  return nearest;
}

/** Seek to the visible message, even if the old view had many older pages.
 * Starting at the old window's beginning could miss it in a bounded read. */
export function activityPositionCursor(events: readonly ActivityEvent[], anchors: readonly ReadingAnchor[]): number | undefined {
  const visible = new Set(anchors.map(anchor => anchor.key));
  const event = events.find(event => visible.has(`item-${event.activity_display_key ?? eventKey(event)}`) && seqOf(event) !== undefined);
  const seq = seqOf(event ?? events.find(event => seqOf(event) !== undefined) ?? {});
  return seq === undefined ? undefined : Math.max(0, seq - 1);
}

/** Window-local reading metadata only: no transcripts, measurements or DOM.
 * Evict the least recently visited position rather than growing with the catalog. */
export function createActivityPositions(limit = 64) {
  const positions = new Map<string, ActivityPosition>();
  return {
    get(key: string | undefined) {
      const position = key === undefined ? undefined : positions.get(key);
      if (position && key !== undefined) { positions.delete(key); positions.set(key, position); }
      return position;
    },
    set(key: string, position: ActivityPosition) {
      const anchors = position.anchors.slice(0, 8).map(anchor => ({ ...anchor }));
      const visible = new Set(anchors.map(anchor => anchor.key.slice("item-".length)));
      positions.delete(key);
      positions.set(key, { ...position, anchors,
        expanded: Object.fromEntries(Object.entries(position.expanded).filter(([id]) => visible.has(id))),
        more: Object.fromEntries(Object.entries(position.more).filter(([key]) => visible.has(key.slice(0, key.lastIndexOf("#")))).slice(0, 64)),
      });
      if (positions.size > limit) positions.delete(positions.keys().next().value!);
    },
  };
}

export const activityPositions = createActivityPositions();
