import { tr } from "../../i18n";
import type { ActivityItem, TranscriptItem } from "./types";

// Task 244 (owner, 2026-11-07): in a team lead's session the owner chooses how
// the team's notices to the lead (everything Zerus pastes: inbox, starved,
// look, stalled and report notices, member messages, doorbells) and the
// lead's answers to them show in the owner's view:
//   full       as today (208's system-notice look);
//   collapsed  each notice one folded line like a tool call; the lead's reply
//              shows in full (owner, 2026-11-09) (the default);
//   silent     the notices do not show; the lead's reply still does (owner, 2026-11-09).
// A turn belongs to the prompts that opened it: it is a notice turn only when
// every prompt that opened it is a team notice; one owner message among them
// makes it the owner's, shown in full. Display only: the lead receives
// everything, and nothing that waits for the owner is hidden.

export type NoticeMode = "full" | "collapsed" | "silent";
export const NOTICE_MODES: readonly NoticeMode[] = ["full", "collapsed", "silent"];
export const DEFAULT_NOTICE_MODE: NoticeMode = "collapsed";

export const noticeMode = (value: unknown): NoticeMode | undefined =>
  NOTICE_MODES.includes(value as NoticeMode) ? value as NoticeMode : undefined;

/** A lead session's mode: its own choice, else the default, else collapsed. */
export function sessionNoticeMode(sessions: unknown, sessionKey: string, fallback: unknown): NoticeMode {
  let chosen: unknown;
  if (typeof sessions === "string" && sessions.length <= 65536) {
    try { const parsed: unknown = JSON.parse(sessions); if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) chosen = (parsed as Record<string, unknown>)[sessionKey]; } catch { /* the default */ }
  }
  return noticeMode(chosen) ?? noticeMode(fallback) ?? DEFAULT_NOTICE_MODE;
}

/** The settings value with `sessionKey` set to `mode` (at most 256 sessions kept, newest last). */
export function withSessionNoticeMode(sessions: unknown, sessionKey: string, mode: NoticeMode): string {
  let map: Record<string, NoticeMode> = {};
  if (typeof sessions === "string") {
    try { const parsed: unknown = JSON.parse(sessions); if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) map = Object.fromEntries(Object.entries(parsed).filter(([, value]) => noticeMode(value))) as Record<string, NoticeMode>; } catch { /* start again */ }
  }
  delete map[sessionKey];
  map[sessionKey] = mode;
  const entries = Object.entries(map);
  return JSON.stringify(Object.fromEntries(entries.slice(-256)));
}

/** A prompt that opens a turn: the owner's message (or recording) or a team notice. */
// A team message "from: You" is the owner's own (sent from the Team tab).
const isOwner = (item: TranscriptItem) => item.kind === "user" || item.kind === "media"
  || (item.kind === "system" && (item as ActivityItem).notice?.from === "You");
const isNotice = (item: TranscriptItem) => item.kind === "system" && !!(item as ActivityItem).teamNotice;
const isPrompt = (item: TranscriptItem) => isOwner(item) || isNotice(item);

const firstLine = (text: string | undefined) => (text ?? "").split("\n").map((line) => line.trim()).find(Boolean) ?? "";

/** The folded notice's line after its sender: "Report to lead, #185: Side panel", as Qt's tool-call row reads. */
export function noticeLine(item: ActivityItem): string {
  const notice = item.notice;
  if (!notice) return firstLine(item.body);
  const head = [notice.kind, notice.task ? `#${notice.task}` : ""].filter(Boolean).join(", ");
  const body = firstLine(notice.title || notice.body);
  return body ? `${head}: ${body}` : head;
}

/** The items as the mode shows them. Full returns them unchanged. */
export function foldNoticeTurns(items: readonly TranscriptItem[], mode: NoticeMode): TranscriptItem[] {
  if (mode === "full") return [...items];
  const out: TranscriptItem[] = [];
  let i = 0;
  // History before the first prompt stays as it is.
  while (i < items.length && !isPrompt(items[i])) out.push(items[i++]);
  while (i < items.length) {
    const prompts: TranscriptItem[] = [];
    while (i < items.length && isPrompt(items[i])) prompts.push(items[i++]);
    const body: TranscriptItem[] = [];
    while (i < items.length && !isPrompt(items[i])) body.push(items[i++]);
    if (prompts.some(isOwner)) { out.push(...prompts, ...body); continue; }
    if (mode === "silent") { out.push(...body); continue; }
    for (const prompt of prompts) {
      const notice = prompt as ActivityItem;
      out.push({ id: `fold-${notice.id}`, kind: "folded", at: notice.at, title: noticeLine(notice), sender: notice.sender, folded: [notice], foldKind: "notice" });
    }
    // Owner, 2026-11-09: the lead's reply is never folded. The owner's messages
    // reach the lead in the middle of a turn a notice opened, so a folded reply
    // hid the lead's answers to the owner. Only the notice itself folds.
    out.push(...body);
  }
  return out;
}
