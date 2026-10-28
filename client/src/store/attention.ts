import type { Session } from "../ui/sessions/types";

// Shared by the session list and the Overview; pure, so tests can import it
// without the page state of store/ui.ts.

/**
 * Qt's SessionInfo::needsAttention (HgsClient.h:107-108) with this device's
 * marks: the core's `needs_attention` through store/marks.ts, what the menu
 * bar counts (menubar.rs, FleetState::attentionSessions). No children. A
 * projection without the field reads its status and unread reply instead.
 */
export const attentionNeeded = (s: Session) => s.attentionNeeded ?? (s.status === "attention" || s.unread);

/**
 * Needs attention in the window's lists and counts: Qt's
 * SessionPresentation::needsAttention (SessionPresentation.h:279-293), that is
 * Review later, or on a reachable computer attentionNeeded or a subagent that
 * asks (muted rows are listed, not counted).
 */
export const needsAttention = (s: Session) =>
  s.state !== "archived" && (!!s.reviewLater || (s.status !== "offline" && (attentionNeeded(s) || !!s.childAttention)));

/** Active = current work OR Scheduled OR Needs attention OR a local draft. */
export const isActive = (s: Session) =>
  s.status === "working" || s.status === "background" || s.status === "scheduled" || s.status === "waiting" || needsAttention(s) || s.draft;
