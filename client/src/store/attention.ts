import type { Session } from "../ui/sessions/types";

// Shared by the session list and the Overview; pure, so tests can import it
// without the page state of store/ui.ts.

/** Needs attention: actionable, unread reply or review later (muted rows are listed, not counted). */
export const needsAttention = (s: Session) => s.status === "attention" || (s.unread && s.status !== "offline") || !!s.reviewLater;

/** Active = current work OR Scheduled OR Needs attention OR a local draft. */
export const isActive = (s: Session) =>
  s.status === "working" || s.status === "background" || s.status === "scheduled" || s.status === "waiting" || needsAttention(s) || s.draft;
