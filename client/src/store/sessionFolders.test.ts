import { beforeEach, describe, expect, it, vi } from "vitest";
import { folderEntries, folderRequest } from "./sessionFolders";
import { toSession } from "./catalog";
import type { Details } from "../types";
import * as bridge from "../bridge";
import { stillListed } from "./actions";
import { toast } from "./notices";

vi.mock("../bridge", () => ({ sessionFolderOpen: vi.fn() }));
vi.mock("./actions", () => ({ stillListed: vi.fn((s) => s) }));
vi.mock("./notices", () => ({ toast: vi.fn() }));
const session = (computer = "") => toSession(computer, "run:run-1", {
  name: "codex/sample-project/review", run_id: "run-1", conversation_id: "conversation-1", cwd: "/srv/parent", state: "running",
});
const available = { folder: true, shell: true, folder_hint: "Open session folder in file manager\n/srv/parent" };
const tick = async () => { for (let i = 0; i < 5; i++) await Promise.resolve(); };

beforeEach(() => { vi.clearAllMocks(); vi.mocked(bridge.sessionFolderOpen).mockResolvedValue(null); vi.mocked(stillListed).mockImplementation((s) => s); });

describe("session folder actions", () => {
  it("uses current details, preserving empty values, and never borrows a parent's folder for a child", () => {
    const row = session("peer");
    const details = { run_id: "run-1", conversation_id: "conversation-1", cwd: "/srv/current" } as Details;
    expect(folderRequest(row, details).directory).toBe("/srv/current");
    expect(folderRequest(row, { ...details, cwd: "" }).directory).toBe("");
    expect(folderRequest(row, undefined).directory).toBe("/srv/parent");
    expect(folderRequest(row, details, { id: "child" }).directory).toBe("");
    expect(folderRequest(row, details, { id: "child", cwd: "/srv/child" })).toEqual({
      target: { computer: "peer", name: row.name, run_id: "run-1", conversation_id: "conversation-1" },
      archive_id: null, agent_id: "child", directory: "/srv/child",
    });
  });
  it("opening the header or row menu never launches anything; click retains exact host, IDs, archive and child", async () => {
    const row = { ...session("peer"), archiveId: "archive-1", state: "archived" };
    const request = folderRequest(row, undefined, { id: "child", cwd: "/srv/child" });
    const entries = folderEntries(row, request, { ...available, folder: false });
    expect(entries.folder.disabled).toBe(true);
    expect(entries.shell.disabled).toBe(false);
    expect(bridge.sessionFolderOpen).not.toHaveBeenCalled();
    entries.shell.action?.(); await tick();
    expect(bridge.sessionFolderOpen).toHaveBeenCalledExactlyOnceWith(request, "shell");
    expect(request.archive_id).toBe("archive-1");
  });
  it("disables unknown paths or identity and reports an unavailable folder with Qt's hint", () => {
    const row = session();
    const request = folderRequest(row);
    expect(folderEntries(row, request).folder.disabled).toBe(true);
    expect(folderEntries(row, request).folder.hint).toBe("Session folder is unavailable");
    expect(folderEntries(row, { ...request, target: { ...request.target, run_id: "" } }, available).shell.disabled).toBe(true);
    expect(folderEntries(row, request, available).folder.disabled).toBe(false);
  });
  it("refuses a stale row and never retries an uncertain launcher result", async () => {
    const row = session(); const request = folderRequest(row);
    const entries = folderEntries(row, request, available);
    vi.mocked(stillListed).mockReturnValueOnce(undefined);
    entries.folder.action?.(); await tick();
    expect(bridge.sessionFolderOpen).not.toHaveBeenCalled();
    vi.mocked(bridge.sessionFolderOpen).mockRejectedValueOnce("No terminal answered");
    entries.shell.action?.(); await tick();
    expect(bridge.sessionFolderOpen).toHaveBeenCalledTimes(1);
    expect(toast).toHaveBeenCalledWith("Could not open folder shell", "No terminal answered", "error", true);
  });
});
