import { GONE, notify, stillListed } from "../../store/actions";
import { markAllRead, markNeedsAttention, markRead, unreadReplies } from "../../store/marks";
import { needsAttention, type MenuEntry } from "../../store/ui";
import { snapshotOf, type ActionRow } from "./actions-ui";
import type { Session } from "./types";

// The mark entries of Qt's Sessions menu (SessionsWindow.cpp 5324-5328, 5416-5433,
// 362-365, 3389-3402), run against the local-first marks store.

export const NEW_REPLY = "A new reply or question arrived. Open the menu again to mark it as read.";
export const REVIEW_HINT = "Keep a reminder to review this session later";
export const MARK_ALL_HINT = "Mark current replies across all machines as read, including hidden sessions. Pending approvals, questions and errors stay highlighted. Sessions you marked as needing attention keep their reply unread.";
const CHIP_REVIEW = "Marked for later. Mark as read to clear this reminder.";
const CHIP_READ = "Mark this session's current notification as read. This does not answer a pending question.";

/** The Mark read chip of the composer toolbar (phase 4 places it): active for a live row that needs attention. */
export function markReadChip(s: Pick<Session, "state" | "status" | "unread" | "reviewLater">): { active: boolean; detail: string } {
  const active = s.state !== "archived" && needsAttention(s as Session);
  return { active, detail: s.reviewLater ? CHIP_REVIEW : CHIP_READ };
}

/** Mark as read, with Qt's refusal notice. */
export function runMarkRead(row: ActionRow): boolean {
  if (!stillListed(row)) return false;
  const ok = markRead(row);
  if (!ok) notify("error", NEW_REPLY, true);
  return ok;
}

/** Mark needs attention, with Qt's refusal notice. */
export function runMarkNeedsAttention(row: ActionRow): boolean {
  if (!stillListed(row)) return false;
  const ok = markNeedsAttention(row);
  if (!ok) notify("error", GONE, true);
  return ok;
}

/** The row's entry, or none for an archived row. Both are always enabled. */
export function markEntry(s: Session): Extract<MenuEntry, { label: string }> | undefined {
  if (s.state === "archived") return undefined;
  const row = snapshotOf(s);
  return markReadChip(s).active
    ? { label: "Mark as read", icon: "check-check", action: () => { runMarkRead(row); } }
    : { label: "Mark needs attention", icon: "bell", hint: REVIEW_HINT, action: () => { runMarkNeedsAttention(row); } };
}

/** The footer button's tooltip (Qt: the label, a newline, the hint). */
export const markAllTip = (e: { label: string; hint?: string }) => `${e.label}\n${e.hint ?? ""}`;

export const markAllLabel = (n: number) => (n > 0 ? `Mark all as read (${n})` : "Mark all as read");

/** Mark all as read: every current reply on every machine; a count shows its notice. */
export function runMarkAll(): number {
  const n = markAllRead();
  if (n > 0) notify("info", n === 1 ? "1 reply marked as read." : `${n} replies marked as read.`);
  return n;
}

export function markAllEntry(): Extract<MenuEntry, { label: string }> {
  const n = unreadReplies().size; // tracked: the marks store's version
  return { label: markAllLabel(n), icon: "read-all", disabled: n === 0, hint: MARK_ALL_HINT, action: () => { runMarkAll(); } };
}
