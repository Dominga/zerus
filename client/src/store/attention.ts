import type { Session } from "../ui/sessions/types";

// Shared by the session list and the Overview; pure, so tests can import it
// without the page state of store/ui.ts.

/** What the attention rules read from a row. */
export type AttentionFacts = Pick<Session, "state" | "status" | "unread" | "reviewLater" | "attentionNeeded" | "actionNeeded" | "childAttention">;

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

/**
 * Qt's `needsAction() && !attentionAcknowledged` with this device's marks: a
 * request, error or recovery that asks for the person and is not
 * acknowledged. No unread reply, Review later or children. A projection
 * without the field reads its status.
 */
export const actionNeeded = (s: Pick<Session, "status" | "actionNeeded">) => s.actionNeeded ?? s.status === "attention";

/**
 * A project header's "need a response" (SessionsWindowRows.cpp:400-401):
 * Review later, or, while the row's computer is `online`, a request not
 * acknowledged or a subagent that asks. The header counts an unread reply
 * as unread, not here.
 */
export const headerAttention = (s: AttentionFacts, online: boolean) =>
  s.state !== "archived" && (!!s.reviewLater || (online && (actionNeeded(s) || !!s.childAttention)));

/**
 * Qt's SessionPresentation::working (SessionPresentation.h:268-271): native
 * work or confirmed background jobs, only while the row's computer is
 * `online`. A computer that is not online shows its last rows, which never
 * count as working.
 */
export const isWorking = (s: Pick<Session, "status">, online: boolean) =>
  online && (s.status === "working" || s.status === "background");

/**
 * Active = current work OR Scheduled OR Needs attention OR a local draft
 * (SessionPresentation::active, SessionPresentation.h:308-312). Work,
 * Scheduled and Waiting count only while the row's computer is `online`; a
 * computer that is not online keeps its Review later and its drafts.
 */
export const isActive = (s: Session, online: boolean) =>
  isWorking(s, online) || (online && (s.status === "scheduled" || s.status === "waiting")) || needsAttention(s, online) || s.draft;
