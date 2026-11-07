import { tr } from "../../i18n";
import { roleColor } from "../teams/roles";

// Task 208 (owner, 2026-11-07): what Zerus pastes into a session is not the
// owner's message. Qt's ActivityView.cpp teamMessage reads every team message
// by the literal header Zerus prepends ("[Zerus team \u00b7 from: Lead \u00b7 task: #3 \u00b7
// attempt: 2]"), inside a session's <zerus-message> envelope or not, and draws
// it as a plaque: kind, sender and receiver, task and attempt, three lines of
// the body and the header under it. The doorbells and task envelopes Zerus
// pastes without a sender ("[Zerus team] 1 new message…", "[Zerus task 12]")
// read as notices from Zerus. The owner's own messages never match: they carry
// no such header.

export interface SystemNotice {
  /** "Task from team", "Report to lead", "Planning brief", "All tasks decided", "Message from you", "Zerus notice". */
  kind: string;
  /** The sender as the header names it ("Lead", "Zerus"), or "Zerus team" for a header without one. */
  from: string;
  /** The receiver when the message names one (the member's role, "Lead"). */
  to: string;
  task: number;
  attempt: number;
  /** The task's title, when the message names it. */
  title: string;
  /** The text shown folded: the message without its header and Zerus's instructions. */
  body: string;
  /** The whole message, shown when opened. */
  full: string;
  /** The literal header line, shown under the body as Qt does. */
  header: string;
}

const ENVELOPE = /^<zerus-message[^>]*>\s*([\s\S]*?)\s*<\/zerus-message>$/;
const HEADER = /^\[Zerus team \u00b7 from: ([^\u00b7\]]+?)((?: \u00b7 [^\u00b7\]]+)*)\]/;
/** Zerus's own pastes without a sender: doorbells, build notices and task envelopes. */
const ZERUS = /^\[Zerus (team|task)\b[^\]\n]*\]/;

/** Qt's teamMessage, then Zerus's sender-less notices; undefined for anything else. */
export function systemNotice(raw: string): SystemNotice | undefined {
  let text = raw.trim();
  const envelope = ENVELOPE.exec(text);
  if (envelope) text = envelope[1].trim();
  const match = HEADER.exec(text);
  if (!match) {
    const zerus = ZERUS.exec(text);
    if (!zerus) return undefined;
    const rest = text.slice(zerus[0].length).trim();
    return { kind: tr("Zerus notice"), from: zerus[1] === "team" ? tr("Zerus team") : "Zerus", to: "", task: 0, attempt: 0, title: "", body: rest || text, full: text, header: zerus[0] };
  }
  const notice: SystemNotice = { kind: "", from: match[1].trim(), to: "", task: 0, attempt: 0, title: "", body: "", full: text, header: match[0] };
  let planning = false, decided = false;
  for (const raw of match[2].split("\u00b7")) {
    const part = raw.trim();
    if (part.startsWith("task: #")) notice.task = Number.parseInt(part.slice(7), 10) || 0;
    else if (part.startsWith("attempt: ")) notice.attempt = Number.parseInt(part.slice(9), 10) || 0;
    else if (part === "planning") planning = true;
    else if (part === "all tasks decided") decided = true;
  }
  let rest = text.slice(match[0].length).trim();
  const report = /^Report for task #\d+ ([^\n]*):\s*/.exec(rest);
  if (report) {
    notice.kind = tr("Report to lead"); notice.to = tr("Lead"); notice.title = report[1];
    rest = rest.slice(report[0].length);
    const decide = rest.lastIndexOf("\n\nDecide with ");
    notice.body = (decide >= 0 ? rest.slice(0, decide) : rest).trim();
  } else if (planning) {
    notice.kind = tr("Planning brief"); notice.to = tr("Lead");
    const plan = rest.indexOf("\n\nPlan the work");
    notice.body = (plan >= 0 ? rest.slice(0, plan) : rest).trim();
  } else if (decided) {
    notice.kind = tr("All tasks decided"); notice.to = tr("Lead");
    // A team name may hold a period ("v1.2 release"); the sentence after it starts with ". ".
    const team = /^Team: (.+?)\.\s+/.exec(rest);
    if (team) rest = rest.slice(team[0].length);
    notice.body = rest.trim();
  } else if (notice.from === "You") {
    notice.kind = tr("Message from you"); notice.body = rest;
  } else {
    notice.kind = tr("Task from team");
    const role = /Your role: ([^.\n]+)\./.exec(rest);
    if (role) notice.to = role[1].trim();
    const task = /(?:^|\n)Task #\d+: ([^\n]+)/.exec(rest);
    if (task) { notice.title = task[1].trim(); rest = rest.slice(task.index + task[0].length).trim(); }
    const finish = rest.indexOf("When you finish, report once");
    notice.body = (finish >= 0 ? rest.slice(0, finish) : rest).trim();
  }
  return notice;
}

/** The folded body: three lines and 320 characters at most, as Qt's plaque. */
export function foldedBody(notice: Pick<SystemNotice, "body" | "full">): { text: string; clipped: boolean } {
  const lines = notice.body.split("\n");
  if (lines.length > 3 || notice.body.length > 320) return { text: `${lines.slice(0, 3).join("\n").slice(0, 320).trim()} …`, clipped: true };
  return { text: notice.body, clipped: notice.body !== notice.full };
}

/** Task 208: the left bar of a system notice. Zerus's own notices are pink (Qt's
 * folder-tick colour); a member's carry its role's colour, as Qt's teamRoleColor. */
export function noticeBar(from: string): { dark: string; light: string } {
  const sender = from.trim().toLowerCase();
  if (sender === "you") return { dark: "#aab1ba", light: "#4d5c6b" };
  if (!sender || sender === "zerus" || sender.startsWith("zerus ")) return { dark: "#f08bb4", light: "#b83a72" };
  return roleColor(from);
}
