import { createRoot } from "solid-js";
import { describe, expect, it, vi } from "vitest";
import * as bridge from "../../bridge";
import { machineCards } from "../../store/machines";
import type { Folder, FolderOut, MachineMeta, Project } from "../../types";
import { createProjectFolders } from "./project-folders";
import { createFolderLaunch } from "../sessions/folder-launch";

vi.mock("../../bridge", () => ({ projectFolders: vi.fn(), projectFolderOpen: vi.fn() }));
vi.mock("../../store/organization", () => ({ grouping: () => ({ ready: true }), changeProjectFolder: vi.fn() }));
vi.mock("../../store/machines", () => ({ machineCards: {} }));
vi.mock("../../store/notices", () => ({ bus: { notify: vi.fn() } }));
const node = "10000000-0000-4000-8000-000000000001";
const local = { host: "workstation", platform: "windows", machine_id: node } as MachineMeta;
const folder: Folder = { id: "drive", key: "drive", machine: "workstation", computer: "workstation", machine_id: node, path: "C:\\Source", name: "Source" };
const project = { id: "alpha", accessible: true, folders: [folder] } as Project;

// N6 (Qt ProjectsDialog): the local computer's folders are edited and opened
// like a peer's: the local Windows read model no longer fences them.
describe("the Windows project folders", () => {
  it("offers this computer as a place and edits its folder through the organization", async () => {
    machineCards[""] = local;
    vi.mocked(bridge.projectFolders).mockResolvedValue([
      { ...folder, machine_name: "workstation" } as FolderOut,
    ]);
    await createRoot(async (dispose) => {
      const edits = createProjectFolders(() => [project], vi.fn());
      expect(edits.enabled("alpha", folder)).toBe(true);
      await edits.open("alpha", folder, {} as HTMLElement);
      expect(bridge.projectFolders).toHaveBeenCalledWith("alpha");
      expect(edits.prompt()?.places.map((place) => place.computer)).toContain("");
      await edits.openFolder("alpha", folder);
      expect(bridge.projectFolderOpen).toHaveBeenCalledWith("alpha", expect.objectContaining({ path: "C:\\Source" }));
      dispose();
    });
  });
  it("launches a session in a local drive or UNC folder, probing it", async () => {
    const localFolderExists = vi.fn(async () => true);
    const launch = createFolderLaunch({ cards: () => ({ "": local }), online: () => true,
      bridge: { localFolderExists, newSessionContext: vi.fn() } });
    for (const path of ["C:\\Source", "\\\\server\\share\\Source"]) {
      const row = { ...folder, path };
      expect(launch.folderLaunch(row).problem).toBe("");
      expect(launch.folderLaunch(row).local).toBe(true);
      expect(await launch.folderExists(row)).toBe(true);
    }
    expect(localFolderExists).toHaveBeenCalledTimes(2);
    expect(launch.folderLaunch({ ...folder, machine: "peer", machine_id: "other", path: "//server/share" }).problem).toBe("");
  });
  it("derives each folder Place style from its own computer metadata", async () => {
    const previous = { ...machineCards };
    machineCards[""] = { ...local, host: "local-linux", platform: "linux", machine_id: "local-node" };
    machineCards["windows-peer"] = { ...local, host: "windows-peer", machine_id: "remote-node" };
    try {
      await createRoot(async (dispose) => {
        const edits = createProjectFolders(() => [project], vi.fn());
        await edits.open("alpha", undefined, {} as HTMLElement);
        expect(edits.prompt()?.places.find((place) => place.computer === "windows-peer")?.style).toBe("windows");
        expect(edits.prompt()?.places.find((place) => place.computer === "")?.style).toBe("posix");
        dispose();
      });
    } finally {
      for (const key of Object.keys(machineCards)) delete machineCards[key];
      Object.assign(machineCards, previous);
    }
  });
});
