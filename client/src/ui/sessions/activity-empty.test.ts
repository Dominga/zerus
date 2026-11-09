import { describe, expect, it } from "vitest";
import { emptyActivity } from "./activity-empty";

// zerus-5wmh.13.8.3: Qt's empty Activity guidance (ActivityView.cpp:1145-1150).
describe("empty Activity guidance", () => {
  const native = { backend: "dsh", runtime_state: "live", activity: "idle", auth_required: false };

  it("asks a live native session that needs sign-in to sign in first", () => {
    expect(emptyActivity({ ...native, activity: "attention", auth_required: true }, true)).toEqual({
      title: "No messages yet", text: "After signing in, send a message below to start this session." });
  });

  it("invites the first message to a live, idle native session", () => {
    expect(emptyActivity(native, true)).toEqual({
      title: "Ready for your first message", text: "Send a message below. The agent's replies, tools and approvals will appear here." });
  });

  it("otherwise says activity will appear, or points an untracked session to its terminal", () => {
    const appear = { title: "Activity will appear here", text: "Messages, tools and approvals will appear as your agent works." };
    for (const summary of [undefined, {}, { ...native, activity: "busy" }, { ...native, runtime_state: "stopped" },
      { ...native, read_only: true }, { ...native, backend: "tmux" }, { ...native, read_only: true, auth_required: true }]) {
      expect(emptyActivity(summary, true)).toEqual(appear);
    }
    expect(emptyActivity(undefined, false)).toEqual({
      title: "Activity will appear here", text: "This session has no recorded activity. Open its terminal to see the agent." });
  });
});
