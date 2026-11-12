import { describe, expect, it, vi } from "vitest";
import { createRoot } from "solid-js";
import { designerChoices, designerProjects, designerRow, isDesignerSession, sessionContext } from "./teamDesigner";
import { createCatalogEditor, removeConfirmation, type CatalogEditorPort } from "./catalogEditor";
import type { CatalogEditorView, Project, TeamConfirmation } from "../types";
import type { Session } from "../ui/sessions/types";

const session = (name: string, extra: Partial<Session> = {}): Session =>
  ({ key: `\nname:${name}`, catalogKey: `name:${name}`, computer: "", name, state: "running", cwdPath: "/work/orbit", cwdCanonical: "/work/orbit", ...extra }) as Session;
const project = (id: string, name: string, folders: { path: string; name: string; computer?: string; machine?: string }[]): Project =>
  ({ id, name, color: null, accessible: true, is_default: false, palette: 0, folders: folders.map((folder) => ({ id: "", key: folder.path, machine: folder.machine ?? folder.computer ?? "workstation", computer: folder.computer ?? folder.machine ?? "workstation", ...folder })) }) as Project;

describe("Team designer project (Qt showTeamDesigner)", () => {
  it("is the running local session named …/team-designer", () => {
    expect(isDesignerSession("", "claude/designer/team-designer")).toBe(true);
    expect(isDesignerSession("studio", "claude/designer/team-designer")).toBe(false);
    const rows = [session("claude/orbit/main"), session("claude/designer/team-designer", { state: "stopped" }), session("codex/designer/team-designer")];
    expect(designerRow(rows)?.name).toBe("codex/designer/team-designer");
  });

  it("lists this computer's folders and follows the selected session's project", () => {
    const projects = designerProjects([
      project("ungrouped", "Inbox", [{ path: "/tmp/x", name: "x" }]),
      project("p1", "Orbit", [{ path: "/work/orbit", name: "orbit" }, { path: "/remote/orbit", name: "orbit", computer: "studio" }]),
      project("p2", "Payments", [{ path: "/work/payments", name: "payments" }]),
    ], "workstation");
    expect(projects.map((entry) => entry.folder)).toEqual(["/work/orbit", "/work/payments"]);
    // The selected local session gives its group's name and its folder.
    const context = sessionContext(session("claude/orbit/main"), "Orbit", false, { folder: "", project: "" })!;
    expect(context).toEqual({ folder: "/work/orbit", project: "Orbit" });
    expect(designerChoices(projects, context).chosen).toBe(0);
    // Without a group, the name's second segment; a remote row gives nothing.
    expect(sessionContext(session("claude/payments/fix", { cwdPath: "/work/payments" }), undefined, false, context)?.project).toBe("payments");
    expect(sessionContext(session("claude/orbit/main", { computer: "studio" }), "Orbit", false, context)).toBeUndefined();
    // A running designer keeps the project of its original brief.
    expect(sessionContext(session("claude/payments/fix"), "Payments", true, context)).toBeUndefined();
    // A folder that no project lists becomes its own choice; no context: no choice.
    const own = designerChoices(projects, { folder: "/work/other", project: "other" });
    expect(own.choices[own.chosen]).toMatchObject({ folder: "/work/other", name: "other" });
    const empty = designerChoices(projects, { folder: "", project: "" });
    expect(empty.chosen).toBe(-1);
    expect(empty.choices).toEqual(projects);
  });
});

const view = (unsaved: number, version = 4): CatalogEditorView =>
  ({ version, builtin: false, roles: [], templates: [], routing: [], pipelines: [], planning_rules: "", unsaved, edited: {} });

function fakePort(answers: boolean[]): CatalogEditorPort & { asked: string[] } {
  const asked: string[] = [];
  return {
    asked,
    schema: vi.fn(async () => ({ roles: [], templates: [], routing: [], pipelines: [] })),
    load: vi.fn(async () => view(0, 5)),
    edit: vi.fn(async () => view(1)),
    revert: vi.fn(async () => view(0)),
    save: vi.fn(async () => ({ outcome: "uncertain" as const, detail: "hgs did not answer in time", request_id: "11111111-2222-4333-8444-555555555555" })),
    resetRules: vi.fn(async () => ({ outcome: "done" as const, receipt: { version: 5 }, request_id: "x" })),
    impact: vi.fn(async () => ({ name: "Backend", templates: ["Feature"], pipelines: ["Reviewed"], emptied: ["Reviewed"] })),
    exportFile: vi.fn(async () => "/home/example/zerus-team-catalog.json"),
    importFile: vi.fn(async () => view(3)),
    route: vi.fn(async () => ({})),
    ask: vi.fn(async (confirmation: TeamConfirmation) => { asked.push(confirmation.name); return answers.shift() ?? false; }),
  };
}

describe("catalog editor store", () => {
  it("names what a deleted role takes, as Qt asks", () => {
    const text = removeConfirmation("role", { name: "Backend", templates: ["Feature"], pipelines: ["Reviewed"], emptied: ["Reviewed"] }).text;
    expect(text).toContain("Backend");
    expect(text).toContain("Feature");
    expect(text).toContain("Reviewed (left with no stage)");
  });

  it("asks before Revert and Delete, and resends an unknown save with its request id", async () => {
    const port = fakePort([false, true]);
    await createRoot(async (dispose) => {
      const editor = createCatalogEditor(port);
      await editor.load();
      await editor.edit({ op: "set", kind: "role", id: "backend", field: "about", value: "API" });
      await editor.revert();
      expect(port.asked).toEqual(["teamCatalogRevertConfirm"]);
      expect(port.revert).not.toHaveBeenCalled();
      await editor.remove("role", "backend");
      expect(port.asked).toEqual(["teamCatalogRevertConfirm", "teamRoleDeleteConfirm"]);
      expect(port.edit).toHaveBeenLastCalledWith("", { op: "remove", kind: "role", id: "backend" });
      await editor.save();
      expect(editor.notice()?.error).toBe(true);
      await editor.save();
      expect(port.save).toHaveBeenLastCalledWith("", "11111111-2222-4333-8444-555555555555");
      await editor.exportFile();
      expect(editor.notice()?.text).toContain("zerus-team-catalog.json");
      dispose();
    });
  });
});
