import { describe, expect, it } from "vitest";
import { agoText, answerRow, newestAnswers, permissionBy, permissionSummary } from "./permissionAnswers";
import type { TeamPermissionAnswer } from "../../types";

// Task 315 (205 F9): a member card's answered permissions, as Qt's U17 line
// "Permissions: 18 allowed, 2 denied, last: allowed by the lead, 3 min ago",
// with commas where Qt has middle dots.

const NOW = 1_000_000;
const answer = (over: Partial<TeamPermissionAnswer> = {}): TeamPermissionAnswer => ({
  question_id: "q", decision: "allow", by: "lead", reason: "", tool: "Bash", command: "cargo build", at: NOW - 180, ...over,
});

describe("member permission answers (task 315)", () => {
  it("summarise the counts and the last answer in one line", () => {
    const answers = [...Array.from({ length: 18 }, () => answer({ at: NOW - 900 })), answer({ decision: "deny", by: "owner" }), answer({ decision: "deny" }), answer()];
    expect(permissionSummary(answers, NOW)).toBe("Permissions: 19 allowed, 2 denied, last: allowed by the lead, 3 min ago");
    expect(permissionSummary([answer({ decision: "deny", by: "timeout", at: NOW - 7200 })], NOW))
      .toBe("Permissions: 0 allowed, 1 denied, last: denied by a timeout, 2 h ago");
    expect(permissionSummary([], NOW)).toBe("");
  });

  it("word who answered and how long ago as Qt does", () => {
    expect(["owner", "lead", "rule", "timeout", "other"].map(permissionBy)).toEqual(["you", "the lead", "a rule", "a timeout", "unknown"]);
    expect([agoText(0, NOW), agoText(NOW, NOW), agoText(NOW - 40, NOW), agoText(NOW - 600, NOW), agoText(NOW - 3 * 86400, NOW)])
      .toEqual(["just now", "just now", "40 s ago", "10 min ago", "3 d ago"]);
  });

  it("open to the newest five rows, the command or tool and the deny text", () => {
    const answers = Array.from({ length: 7 }, (_, n) => answer({ question_id: `q${n}`, at: NOW - 100 + n }));
    const { shown, more } = newestAnswers(answers);
    expect([shown.map((found) => found.question_id), more]).toEqual([["q6", "q5", "q4", "q3", "q2"], 2]);
    const denied = answerRow(answer({ decision: "deny", by: "owner", command: "", tool: "Write", reason: "Not in this folder" }), NOW);
    expect(denied).toEqual({ decision: "Denied by you, 3 min ago", denied: true, command: "Write", reason: "Not in this folder",
      tip: "Denied by you, 3 min ago\nWrite\nNot in this folder" });
    expect(answerRow(answer(), NOW)).toMatchObject({ decision: "Allowed by the lead, 3 min ago", denied: false, command: "cargo build" });
  });
});
