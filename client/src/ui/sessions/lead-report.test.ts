import { describe, expect, it } from "vitest";
import { lastReply, reportDraft } from "./lead-report";

// Qt parity (I4): SessionsWindow::sendReportToLead's reply and draft.

describe("Send report", () => {
  it("takes the main agent's newest reply with text", () => {
    expect(lastReply([
      { type: "AgentMessage", detail: "First reply" },
      { type: "Stop", detail: "  The API is wired; tests pass.\n" },
      { type: "AgentMessage", detail: "A subagent's note", agent_id: "agent-7" },
      { type: "Stop", detail: "   " },
      { type: "PostToolUse", detail: "cargo test" },
      { type: "UserPromptSubmit", detail: "Thanks" },
    ])).toBe("The API is wired; tests pass.");
    expect(lastReply([{ type: "AgentMessage", detail: "Only this" }, { type: "Stop" }])).toBe("Only this");
    expect(lastReply([{ type: "UserPromptSubmit", detail: "Hello" }, { type: "Stop", detail: "", agent_id: "" }])).toBe("");
    expect(lastReply([])).toBe("");
  });

  it("names the session and adds the reply after a blank line", () => {
    expect(reportDraft("Checkout dashboard", "The API is wired.")).toBe("Report from Checkout dashboard:\n\nThe API is wired.");
    expect(reportDraft("Checkout dashboard", "")).toBe("Report from Checkout dashboard:");
  });
});
