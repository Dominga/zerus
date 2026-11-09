import { describe, expect, it } from "vitest";
import { foldNoticeTurns, noticeLine, sessionNoticeMode, withSessionNoticeMode } from "./leadNotices";
import { buildTranscript } from "./transcript";
import type { ActivityEvent } from "../../types";
import type { ActivityItem, TranscriptItem } from "./types";

// Task 244: a lead session shows the team's notices and its replies to them
// in full, folded (the default) or not at all; the owner's turns always show.
const event = (seq: number, type: string, detail: string, extra: Record<string, unknown> = {}) => ({ seq, type, detail, at: 1793050800 + seq, ...extra }) as unknown as ActivityEvent;
const INBOX = "[Zerus team \u00b7 from: Zerus \u00b7 inbox]\nReport for task #185 Side panel: done.";
const events = [
  event(1, "UserPromptSubmit", "Plan the next phase, please."),
  event(2, "Stop", "Planned phase 4."),
  event(3, "UserPromptSubmit", INBOX),
  event(4, "PreToolUse", "hgs team accept", { tool: "Bash" }),
  event(5, "PostToolUse", "accepted", { tool: "Bash" }),
  event(6, "Stop", "Accepted #185; the next task goes out."),
  event(7, "UserPromptSubmit", "[Zerus team] 1 new message for you: hgs team inbox"),
  event(8, "UserPromptSubmit", "And also check the light theme."),
  event(9, "Stop", "Done: both."),
  event(10, "UserPromptSubmit", "Message from claude/p/tester", { origin: "peer_message", sender: "claude/p/tester", report: "Are the fixtures in?" }),
  event(11, "Stop", "Yes, they are."),
  event(12, "UserPromptSubmit", "[Zerus team \u00b7 from: You \u00b7 task: #3 \u00b7 attempt: 2]\nYour turn ended without a report"),
  event(13, "Stop", "Reporting now."),
];
const shape = (items: readonly TranscriptItem[]) => items.map((item) => item.kind === "folded" ? `fold:${(item as ActivityItem).foldKind}` : item.kind);

describe("lead session notice modes (task 244)", () => {
  const items = buildTranscript(events);

  it("full shows everything as 208 does", () => {
    expect(foldNoticeTurns(items, "full")).toEqual(items);
  });

  it("collapsed folds each notice; the lead's reply always shows in full; the owner's turns stay whole", () => {
    // Owner, 2026-11-09: a folded reply hid the lead's answers to the owner,
    // whose messages reach the lead in the middle of a notice's turn.
    const shown = foldNoticeTurns(items, "collapsed");
    expect(shape(shown)).toEqual([
      "user", "agent",                        // the owner's turn
      "fold:notice", "tools", "agent",        // the inbox notice folded; the lead's tools and answer shown
      "system", "user", "agent",              // a doorbell with the owner's message in one turn: the owner's
      "fold:notice", "agent",                 // a member's message folded; its answer shown
      "system", "agent",                      // "from: You" is the owner's own team message
    ]);
    const notice = shown[2] as ActivityItem;
    expect(notice.title).toBe("Report to lead: Side panel");
    expect(notice.sender).toBe("Zerus");
    expect(notice.folded!.map((item) => item.id)).toEqual([items[2].id]);
  });

  it("silent hides the notices but never the lead's replies or the owner's turns", () => {
    expect(shape(foldNoticeTurns(items, "silent"))).toEqual(["user", "agent", "tools", "agent", "system", "user", "agent", "agent", "system", "agent"]);
  });

  it("keeps history before the first prompt and a notice left without a reply", () => {
    const tail = buildTranscript([event(1, "Stop", "Earlier reply."), event(2, "UserPromptSubmit", INBOX)]);
    expect(shape(foldNoticeTurns(tail, "collapsed"))).toEqual(["agent", "fold:notice"]);
    expect(shape(foldNoticeTurns(tail, "silent"))).toEqual(["agent"]);
  });

  it("names a peer message by its sender when it carries no team header", () => {
    const peer = buildTranscript([events[9]])[0] as ActivityItem;
    expect([peer.sender, noticeLine(peer)]).toEqual(["Message from claude/p/tester", "Are the fixtures in?"]);
  });

  it("remembers the mode per lead session over the default", () => {
    expect(sessionNoticeMode(undefined, "\nrun:1", undefined)).toBe("collapsed");
    expect(sessionNoticeMode("{}", "\nrun:1", "full")).toBe("full");
    const stored = withSessionNoticeMode("{}", "\nrun:1", "silent");
    expect(sessionNoticeMode(stored, "\nrun:1", "full")).toBe("silent");
    expect(sessionNoticeMode(stored, "\nrun:2", "full")).toBe("full");
    expect(sessionNoticeMode("not json", "\nrun:1", "bogus")).toBe("collapsed");
    expect(JSON.parse(withSessionNoticeMode(stored, "\nrun:1", "full"))).toEqual({ "\nrun:1": "full" });
  });
});
