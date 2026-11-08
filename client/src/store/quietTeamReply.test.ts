import { describe, expect, it } from "vitest";
import { quietTeamReply, toSession } from "./catalog";

// Task 244: a lead's reply to a turn only the team's pastes opened is not
// unread unless the lead shows its team notices in full; the owner's turns
// and members' sessions keep their unread mark.
describe("a lead's replies to team notices (task 244)", () => {
  const values = (mode?: string, sessions: Record<string, string> = {}) => ({ "teams/leadNotices": mode, "teams/leadNoticeSessions": JSON.stringify(sessions) });

  it("are quiet in Collapsed (the default) and Hidden, loud in Full", () => {
    const row = { team_lead: true, reply_team_notice: true };
    expect(quietTeamReply("", "run:1", row, values())).toBe(true);
    expect(quietTeamReply("", "run:1", row, values("silent"))).toBe(true);
    expect(quietTeamReply("", "run:1", row, values("full"))).toBe(false);
    // The lead's own choice wins over the default.
    expect(quietTeamReply("", "run:1", row, values("full", { "\nrun:1": "collapsed" }))).toBe(true);
    expect(quietTeamReply("arch", "run:1", row, values("collapsed", { "arch\nrun:1": "full" }))).toBe(false);
  });

  it("never touch the owner's turns or a member's session", () => {
    expect(quietTeamReply("", "run:1", { team_lead: true, reply_team_notice: false }, values())).toBe(false);
    expect(quietTeamReply("", "run:1", { team_lead: false, reply_team_notice: true }, values())).toBe(false);
  });

  it("leave a quiet reply read on the card", () => {
    const row = { name: "claude/p/lead", agent: "claude", unread: true, team_lead: true, reply_team_notice: true, state: "running", status: "ready" };
    expect(toSession("", "run:1", row).unread).toBe(false);
    expect(toSession("", "run:2", { ...row, reply_team_notice: false }).unread).toBe(true);
  });

  it("stay read and out of Needs attention on a core row with shared read state (task 315)", () => {
    // The core sends the finished-reply candidate and the host's read mark; the
    // marks overlay recomputed the reply as unread and lost the quiet rule.
    const row = { name: "claude/p/lead", agent: "claude", unread: true, team_lead: true, reply_team_notice: true, state: "running",
      status: "ready", conversation_id: "c", reply_id: "r1", run_id: "run", shared_read: true, unread_candidate: true, needs_attention: true };
    const quiet = toSession("", "run:3", row);
    expect([quiet.unread, quiet.unreadReply, quiet.attentionNeeded]).toEqual([false, false, false]);
    const loud = toSession("", "run:4", { ...row, reply_team_notice: false });
    expect([loud.unread, loud.unreadReply, loud.attentionNeeded]).toEqual([true, true, true]);
  });
});
