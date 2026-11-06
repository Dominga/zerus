import { describe, expect, it } from "vitest";
import { createRoot } from "solid-js";
import { createFileDialog, createMappingDialog, fileScope, problem, type FilePort, type MappingPort } from "./wsl-mapping";
import type { WslFields, WslOpened } from "../../bridge";

// Task 73: Map Windows path… and Open mapped Windows original… keep Qt's
// order: fields, preview, then use or open; any change needs a new preview;
// a refusal from client/core is shown and nothing is used or opened.

const target = (saved: Partial<WslFields> = {}): WslOpened => ({ handle: "h1", target: { alias: "ubuntu", machine_id: "workstation",
  distribution: "Ubuntu", user: "dev", environment: "wsl:Ubuntu:dev", saved: { windows_root: "", mount_root: "", workspace_id: "", ...saved } } });
const text = (source: string) => ({ source });
const fill = (state: { setField(name: keyof WslFields, value: string): void }) => {
  state.setField("windows_root", "C:\\Projects"); state.setField("mount_root", "/mnt"); state.setField("workspace_id", "orbit");
};

describe("Map Windows path", () => {
  function fixture(over: Partial<MappingPort> = {}) {
    const calls: unknown[][] = [];
    const port: MappingPort = {
      open: async (alias) => { calls.push(["open", alias]); return target(); },
      preview: async (handle, source, fields) => { calls.push(["preview", handle, source, fields]); return { windows: source, wsl: "/mnt/c/Projects/app", canonical: "c:/projects/app" }; },
      use: async (handle, source, fields, remember) => { calls.push(["use", handle, source, fields, remember]); return "/mnt/c/Projects/app"; },
      close: async (handle) => { calls.push(["close", handle]); },
      ...over,
    };
    return { state: createRoot(() => createMappingDialog(port)), calls };
  }

  it("previews only with every field, then uses the previewed mapping after a recheck", async () => {
    const { state, calls } = fixture();
    await state.open("ubuntu");
    expect(state.canPreview()).toBe(false);
    state.setSource("C:\\Projects\\app"); fill(state);
    expect(state.canPreview()).toBe(true);
    expect(state.canUse()).toBe(false);
    await state.preview();
    expect(state.notice()).toContain("WSL: /mnt/c/Projects/app");
    expect(state.canUse()).toBe(true);
    state.setRemember(true);
    expect(await state.use()).toBe("/mnt/c/Projects/app");
    expect(calls.at(-1)).toEqual(["use", "h1", "C:\\Projects\\app", { windows_root: "C:\\Projects", mount_root: "/mnt", workspace_id: "orbit" }, true]);
  });

  it("a change after the preview needs a new one; a refusal is shown and nothing is used", async () => {
    const { state } = fixture({ use: async () => { throw text("The WSL connection changed. Close this preview and select the machine again."); } });
    await state.open("ubuntu");
    state.setSource("C:\\Projects\\app"); fill(state);
    await state.preview();
    state.setField("mount_root", "/mnt/wsl");
    expect(state.canUse()).toBe(false);
    expect(state.notice()).toBe("The selection changed. Preview its exact mapping before using it.");
    await state.preview();
    expect(await state.use()).toBeUndefined();
    expect(state.notice()).toBe("The WSL connection changed. Close this preview and select the machine again.");
    expect(state.canUse()).toBe(false);
  });

  it("an ssh computer is refused when the dialog opens; saved fields come back", async () => {
    const refused = fixture({ open: async () => { throw text("This action requires an explicitly configured local WSL connection."); } });
    await expect(refused.state.open("studio")).rejects.toEqual(text("This action requires an explicitly configured local WSL connection."));
    expect(problem(text("This action requires an explicitly configured local WSL connection."))).toBe("This action requires an explicitly configured local WSL connection.");
    const saved = fixture({ open: async () => target({ windows_root: "C:\\Projects", mount_root: "/mnt", workspace_id: "orbit" }) });
    await saved.state.open("ubuntu");
    expect(saved.state.fields().workspace_id).toBe("orbit");
    expect(saved.state.close()).toBe(true);
    expect(saved.calls.at(-1)).toEqual(["close", "h1"]);
  });
});

describe("Open mapped Windows original", () => {
  const scope = { session: "claude/app/main", run_id: "run-1", conversation_id: "conv-1", path: "/mnt/c/Projects/app/notes.md" };
  function fixture(over: Partial<FilePort> = {}) {
    const calls: unknown[][] = [];
    const port: FilePort = {
      open: async (alias, s) => { calls.push(["open", alias, s]); return target(); },
      preview: async (handle, fields) => { calls.push(["preview", handle, fields]); return { wsl: scope.path, windows: "C:\\Projects\\app\\notes.md", sha256: "b".repeat(64) }; },
      openOriginal: async (handle, fields, remember) => { calls.push(["openOriginal", handle, fields, remember]); return { source: "Opened the verified Windows original: %1", args: [{ kind: "verbatim", value: "C:\\Projects\\app\\notes.md" }] }; },
      close: async (handle) => { calls.push(["close", handle]); },
      ...over,
    };
    return { state: createRoot(() => createFileDialog(port)), calls };
  }

  it("opens only after a verified preview of the same fields", async () => {
    const { state, calls } = fixture();
    await state.open("ubuntu", scope);
    expect(calls[0]).toEqual(["open", "ubuntu", scope]);
    expect(state.canOpen()).toBe(false);
    fill(state);
    await state.preview();
    expect(state.notice()).toContain("SHA-256: " + "b".repeat(64));
    expect(state.canOpen()).toBe(true);
    expect(await state.openOriginal()).toBe("Opened the verified Windows original: C:\\Projects\\app\\notes.md");
  });

  it("a changed field or a refused proof opens nothing", async () => {
    const { state } = fixture({ openOriginal: async () => { throw text("The file proof no longer matches this preview. Nothing was opened."); } });
    await state.open("ubuntu", scope);
    fill(state);
    await state.preview();
    state.setField("workspace_id", "other");
    expect(state.canOpen()).toBe(false);
    expect(await state.openOriginal()).toBeUndefined();
    await state.preview();
    expect(await state.openOriginal()).toBeUndefined();
    expect(state.notice()).toBe("The file proof no longer matches this preview. Nothing was opened.");
    expect(state.canOpen()).toBe(false);
  });

  it("an unusable connection is shown and the fields stay closed", async () => {
    const { state } = fixture({ open: async () => { throw text("Choose an enabled, explicitly configured local WSL connection. SSH and default-distribution fallbacks are not allowed."); } });
    await state.open("studio", scope);
    expect(state.loaded()).toBe(false);
    expect(state.canPreview()).toBe(false);
    expect(state.notice()).toMatch(/^Choose an enabled/);
  });

  it("is offered for a known run and conversation and an absolute WSL path", () => {
    const session = { name: "claude/app/main", runId: "run-1", conversationId: "conv-1", archiveId: "a-1" };
    expect(fileScope(session, "/mnt/c/x.md", "child")).toEqual({ session: "claude/app/main", run_id: "run-1", conversation_id: "conv-1", path: "/mnt/c/x.md", archive_id: "a-1", agent_id: "child" });
    expect(fileScope({ ...session, runId: undefined }, "/mnt/c/x.md")).toBeUndefined();
    expect(fileScope(session, "relative/x.md")).toBeUndefined();
    expect(fileScope(undefined, "/x")).toBeUndefined();
  });
});
