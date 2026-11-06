import type { ComposerView, HeldMessageView } from "../../types";
import type { Session } from "./types";
import { draftKey } from "../../store/draftKeys";

/**
 * The message the selected session's composer holds while the desktop
 * service compacts before sending it (zerus-pabah): only the main composer
 * of exactly that live session, never a child's or another session's.
 */
export function heldMessage(view: Pick<ComposerView, "key" | "subagent" | "held"> | undefined, s: Session | undefined, child: boolean): HeldMessageView | undefined {
  if (!view?.held || !s || child || view.subagent || s.archiveId) return undefined;
  const key = draftKey({ computer: s.computer, key: s.catalogKey, name: s.name, archive_id: null, session_id: s.sessionId ?? null });
  return view.key === key ? view.held : undefined;
}

/** The local rows with the held message's own row left out: the held entry stands for it until it is sent. */
export function withoutHeld<T extends { id: string }>(rows: readonly T[], held: HeldMessageView | null | undefined): readonly T[] {
  return held ? rows.filter((row) => row.id !== held.request_id) : rows;
}
