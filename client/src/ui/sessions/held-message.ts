import type { ComposerView, HeldMessageView } from "../../types";
import type { Session } from "./types";

/**
 * The message the selected session's composer holds while the desktop
 * service compacts before sending it (zerus-pabah). It follows the open
 * composer's own view, as the composer and its queue do, with no draft-key
 * comparison: a key or route difference must never hide it (zerus-i0ak2).
 * Only a main composer of a live session shows it, never a child's.
 */
export function heldMessage(held: HeldMessageView | null | undefined, view: Pick<ComposerView, "subagent"> | null | undefined, s: Session | undefined, child: boolean): HeldMessageView | undefined {
  if (!held || !view || !s || child || view.subagent || s.state === "archived") return undefined;
  return held;
}

/** The local rows with the held message's own row left out: the held entry stands for it until it is sent. */
export function withoutHeld<T extends { id: string }>(rows: readonly T[], held: HeldMessageView | null | undefined): readonly T[] {
  return held ? rows.filter((row) => row.id !== held.request_id) : rows;
}
