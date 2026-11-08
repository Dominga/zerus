import { describe, expect, it } from "vitest";
import { ago, continueTip, stateText, statusText, titleOf, warning } from "./claude-app-ui";
import type { ClaudeAppSession, ClaudeAppView } from "../../store/claudeApp";

// WhiteWh fork: the Claude app sessions dialog's Qt texts
// (tray/src/ClaudeAppSessionsDialog.h 119-156, 168-185).

const row = (over: Partial<ClaudeAppSession> = {}): ClaudeAppSession => ({
  id: "local_aaa", conversation: "048ba331-3768-4b1c-9e2f-0123456789ab", cwd: "D:\\Work\\repo",
  title: "Fix the DX11 build", suggested_name: "fix-the-dx11-build", archived: false, running: false,
  running_process: false, history: true, folder_exists: true, last_activity: 1000, state: "stopped", in_folder: true,
  ...over,
});

const view = (over: Partial<ClaudeAppView> = {}): ClaudeAppView => ({
  sessions: [row()], truncated: false, available: true, folder: "D:\\Work\\repo", folder_enabled: true, ...over,
});

describe("Claude app session dialog texts", () => {
  it("keeps Qt's ago buckets and leaves future times at just now", () => {
    expect(ago(1000, 1000)).toBe("just now");
    expect(ago(1000, 1089)).toBe("just now");
    expect(ago(1000, 1090)).toBe("1 min ago");
    expect(ago(1000, 6399)).toBe("89 min ago");
    expect(ago(1000, 6400)).toBe("1 h ago");
    expect(ago(1000, 130_599)).toBe("35 h ago");
    expect(ago(1000, 130_600)).toBe("1 days ago");
    expect(ago(2000, 1000)).toBe("just now");
  });

  it("names an untitled session from its conversation", () => {
    expect(titleOf(row())).toBe("Fix the DX11 build");
    expect(titleOf(row({ title: "" }))).toBe("Untitled (048ba331)");
  });

  it("words every State column code", () => {
    expect(stateText("open_in_app")).toBe("Open in the app");
    expect(stateText("archived")).toBe("Archived");
    expect(stateText("stopped")).toBe("Stopped");
    expect(stateText("no_history")).toBe("No history");
    expect(stateText("folder_missing")).toBe("Folder missing");
  });

  it("has Qt's status line for each list", () => {
    expect(statusText(undefined, 0, false, "")).toBe("");
    expect(statusText(undefined, 0, false, "cannot start 'hgs'")).toBe("cannot start 'hgs'");
    expect(statusText(view({ available: false }), 0, false, "")).toBe("The Claude app keeps no sessions on this computer.");
    expect(statusText(view(), 0, true, "")).toBe("No Claude app session works in this folder.");
    expect(statusText(view(), 0, false, "")).toBe("No Claude app sessions.");
    expect(statusText(view(), 2, false, "")).toBe("");
    expect(statusText(view({ truncated: true }), 2, false, "")).toBe("The newest 1 sessions.");
  });

  it("explains Continue in Qt's order", () => {
    expect(continueTip(undefined, "x")).toBe("");
    expect(continueTip(row({ history: false }), "problem")).toBe("Claude Code has no history of this conversation here.");
    expect(continueTip(row({ folder_exists: false }), "problem")).toBe("Its folder is not on this computer.");
    expect(continueTip(row(), "Enter a session name.")).toBe("Enter a session name.");
    expect(continueTip(row(), undefined)).toBe("");
  });

  it("warns about a session the app may still run, with the surer reason", () => {
    expect(warning(row({ running_process: true }))).toEqual({
      title: "Still open in the Claude app?",
      text: "A Claude process of the app still continues this conversation. If both write to it, its history breaks. Stop the session in the Claude app first. Continue anyway?",
    });
    expect(warning(row()).text).toBe("This conversation changed in the last two minutes, so the Claude app may still run it. If both write to it, its history breaks. Continue anyway?");
  });
});
