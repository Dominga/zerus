import type { Session } from "../ui/sessions/types";

// Shared by the session list and the Overview; pure, so tests can import it
// without the page state of store/ui.ts.

/** What the attention rules read from a row. */
export type AttentionFacts = Pick<Session, "state" | "status" | "unread" | "reviewLater" | "attentionNeeded" | "childAttention">;

/**
 * Qt's SessionInfo::needsAttention (HgsClient.h:107-108) with this device's
 * marks: the core's `needs_attention` through store/marks.ts, what the menu
 * bar (menubar.rs, FleetState::attentionSessions) and Qt's machine cards
 * (DashboardPage.cpp:133) count. No children and no online gate. A
 * projection without the field reads its status and unread reply instead.
 */
export const attentionNeeded = (s: Pick<Session, "status" | "unread" | "attentionNeeded">) =>
  s.attentionNeeded ?? (s.status === "attention" || s.unread);

/**
 * Needs attention in the window's lists and counts: Qt's
 * SessionPresentation::needsAttention (SessionPresentation.h:279-293), that is
 * Review later, or, while the row's computer is `online`, attentionNeeded or
 * a subagent that asks (muted rows are listed, not counted). A computer that
 * is not online shows its last rows, which count only their Review later.
 */
export const needsAttention = (s: AttentionFacts, online: boolean) =>
  s.state !== "archived" && (!!s.reviewLater || (online && (attentionNeeded(s) || !!s.childAttention)));

/** Active = current work OR Scheduled OR Needs attention OR a local draft. */
export const isActive = (s: Session, online: boolean) =>
  s.status === "working" || s.status === "background" || s.status === "scheduled" || s.status === "waiting" || needsAttention(s, online) || s.draft;
