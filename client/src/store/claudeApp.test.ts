import { createRoot, createSignal, type Signal } from "solid-js";
import { describe, expect, it } from "vitest";
import { createClaudeApp, type ClaudeAppSession, type ClaudeAppView, type ClaudeAppDeps } from "./claudeApp";

// WhiteWh fork: the Claude app sessions dialog's store
// (tray/src/ClaudeAppSessionsDialog.h; SessionsWindow.cpp 3688-3701). The read
// carries the dialog's context; Continue sends Qt's exact resume once; a
// session the app may still run asks first.

const session = (over: Partial<ClaudeAppSession> = {}): ClaudeAppSession => ({
  id: "local_aaa", conversation: "048ba331-3768-4b1c-9e2f-0123456789ab", cwd: "D:\\Work\\repo",
  title: "Fix the DX11 build", suggested_name: "fix-the-dx11-build", archived: false, running: false,
  running_process: false, history: true, folder_exists: true, last_activity: 1000, state: "stopped", in_folder: true,
  ...over,
});

const view = (over: Partial<ClaudeAppView> = {}): ClaudeAppView => ({
  sessions: [session()], truncated: false, available: true, folder: "D:\\Work\\repo", folder_enabled: true, ...over,
});

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

type Row = { key: string; computer: string; launchId?: string };

interface Fixture {
  dialog: ReturnType<typeof createClaudeApp>;
  calls: { command: string; args: unknown[] }[];
  notices: { tone: string; text: string; sticky: boolean }[];
  selected: string[];
  rows: Signal<readonly Row[]>;
  dispose(): void;
}

function setup(options: {
  list?: () => Promise<ClaudeAppView>;
  resume?: () => Promise<string>;
  problem?: (name: string) => string | undefined;
} = {}): Fixture {
  const calls: Fixture["calls"] = [];
  const notices: Fixture["notices"] = [];
  const selected: string[] = [];
  const rows = createSignal<readonly Row[]>([]);
  const bridge: ClaudeAppDeps["bridge"] = {
    claudeAppSessions: (host, folder) => {
      calls.push({ command: "list", args: [host, folder] });
      return (options.list ?? (() => Promise.resolve(view())))();
    },
    claudeAppResume: (host, id, name, force) => {
      calls.push({ command: "resume", args: [host, id, name, force] });
      return (options.resume ?? (() => Promise.resolve("11111111-2222-3333-4444-555555555555")))();
    },
  };
  let dispose!: () => void;
  const dialog = createRoot((end) => {
    dispose = end;
    return createClaudeApp({
      bridge,
      notify: (tone, text, sticky) => notices.push({ tone, text, sticky: sticky === true }),
      select: (key) => selected.push(key),
      rows: rows[0],
      problem: options.problem ?? (() => undefined),
    });
  });
  return { dialog, calls, notices, selected, rows, dispose };
}

describe("Claude app sessions dialog", () => {
  it("reads the app's list for the dialog's computer and folder", async () => {
    const t = setup();
    t.dialog.openDialog("", "D:\\Work\\repo");
    expect(t.dialog.busy()).toBe(true);
    expect(t.dialog.onlyFolder()).toBe(true);
    await flush();
    expect(t.calls).toEqual([{ command: "list", args: ["", "D:\\Work\\repo"] }]);
    expect(t.dialog.busy()).toBe(false);
    expect(t.dialog.data()?.sessions).toHaveLength(1);
    // Qt selects the first visible row and fills its suggested name.
    expect(t.dialog.selected()?.id).toBe("local_aaa");
    expect(t.dialog.name()).toBe("fix-the-dx11-build");
    expect(t.dialog.canContinue()).toBe(true);
    t.dispose();
  });

  it("starts with This folder only off when New session has no folder", async () => {
    const t = setup();
    t.dialog.openDialog("", "");
    await flush();
    expect(t.dialog.onlyFolder()).toBe(false);
    t.dispose();
  });

  it("filters by the core's folder match and keeps the name with its row", async () => {
    const t = setup({
      list: () => Promise.resolve(view({
        sessions: [session(), session({ id: "local_bbb", cwd: "D:\\Other", in_folder: false, suggested_name: "app-bbb", title: "" })],
      })),
    });
    t.dialog.openDialog("", "D:\\Work\\repo");
    await flush();
    expect(t.dialog.visible().map((row) => row.id)).toEqual(["local_aaa"]);
    t.dialog.setOnlyFolder(false);
    expect(t.dialog.visible().map((row) => row.id)).toEqual(["local_aaa", "local_bbb"]);
    t.dialog.select("local_bbb");
    expect(t.dialog.name()).toBe("app-bbb");
    // Filtering away the selected row selects the first visible one.
    t.dialog.setOnlyFolder(true);
    expect(t.dialog.selected()?.id).toBe("local_aaa");
    expect(t.dialog.name()).toBe("fix-the-dx11-build");
    t.dispose();
  });

  it("refuses Continue for no history, a missing folder or an invalid name", async () => {
    const t = setup({
      list: () => Promise.resolve(view({ sessions: [session({ history: false }), session({ id: "local_bbb", folder_exists: false })] })),
      problem: (name) => (name === "" ? "Enter a session name." : undefined),
    });
    t.dialog.openDialog("", "D:\\Work\\repo");
    await flush();
    expect(t.dialog.canContinue()).toBe(false);
    t.dialog.select("local_bbb");
    expect(t.dialog.canContinue()).toBe(false);
    t.dialog.setName("");
    expect(t.dialog.canContinue()).toBe(false);
    expect(t.dialog.begin()).toBe("none");
    expect(t.calls.filter((call) => call.command === "resume")).toEqual([]);
    t.dispose();
  });

  it("sends Qt's resume once, notices the launch and opens the session it listed", async () => {
    const t = setup();
    t.dialog.openDialog("studio", "D:\\Work\\repo");
    await flush();
    expect(t.dialog.begin()).toBe("sent");
    expect(t.dialog.begin(), "a second Continue while the call runs does nothing").toBe("none");
    expect(t.calls.filter((call) => call.command === "resume")).toEqual([
      { command: "resume", args: ["studio", "local_aaa", "fix-the-dx11-build", false] },
    ]);
    expect(t.notices[0]).toEqual({ tone: "info", text: "Continuing the Claude app session…", sticky: false });
    await flush();
    expect(t.notices[1].text).toBe("Session started. Loading activity…");
    // The launched row appears with its launch id: the window opens its Activity.
    t.rows[1]([{ key: "row-1", computer: "studio", launchId: "11111111-2222-3333-4444-555555555555" }]);
    await flush();
    expect(t.selected).toEqual(["row-1"]);
    t.dispose();
  });

  it("asks before continuing a session the app may still run, and force follows the answer", async () => {
    const t = setup({ list: () => Promise.resolve(view({ sessions: [session({ running: true, running_process: true })] })) });
    t.dialog.openDialog("", "D:\\Work\\repo");
    await flush();
    expect(t.dialog.begin()).toBe("asked");
    expect(t.dialog.asking()).toBe(true);
    expect(t.dialog.answer(false)).toBe(false);
    expect(t.dialog.asking()).toBe(false);
    expect(t.calls.filter((call) => call.command === "resume")).toEqual([]);
    expect(t.dialog.begin()).toBe("asked");
    expect(t.dialog.answer(true)).toBe(true);
    expect(t.calls.filter((call) => call.command === "resume")).toEqual([
      { command: "resume", args: ["", "local_aaa", "fix-the-dx11-build", true] },
    ]);
    t.dispose();
  });

  it("reports a failed resume and never replays it", async () => {
    const t = setup({ resume: () => Promise.reject("A Claude process still continues this conversation.") });
    t.dialog.openDialog("", "D:\\Work\\repo");
    await flush();
    expect(t.dialog.begin()).toBe("sent");
    await flush();
    expect(t.notices.at(-1)).toEqual({ tone: "error", text: "Could not start session fix-the-dx11-build: A Claude process still continues this conversation.", sticky: true });
    expect(t.dialog.error()).toContain("still continues");
    t.dispose();
  });

  it("shows a failed read and drops an answer for a closed dialog", async () => {
    let answer: ((view: ClaudeAppView) => void) | undefined;
    const t = setup({ list: () => new Promise<ClaudeAppView>((resolve) => { answer = resolve; }) });
    t.dialog.openDialog("", "D:\\Work\\repo");
    t.dialog.close();
    answer?.(view({ sessions: [session({ title: "late" })] }));
    await flush();
    expect(t.dialog.data()).toBeUndefined();
    const failed = setup({ list: () => Promise.reject("Invalid Claude app session list") });
    failed.dialog.openDialog("", "");
    await flush();
    expect(failed.dialog.error()).toBe("Invalid Claude app session list");
    expect(failed.dialog.data()).toBeUndefined();
    expect(failed.dialog.busy()).toBe(false);
    t.dispose();
    failed.dispose();
  });
});
