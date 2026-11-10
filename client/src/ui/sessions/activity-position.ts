import type { ReadingAnchor } from "./activity-anchor";
import { eventKey, seqOf } from "../../store/activity";
import type { ActivityEvent } from "../../types";

export interface ActivityPosition {
  following: boolean;
  top: number;
  anchors: readonly ReadingAnchor[];
  /** Read immediately before the visible row if it has left the new tail. */
  after?: number;
  expanded: Record<string, boolean>;
  more: Record<string, number>;
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
