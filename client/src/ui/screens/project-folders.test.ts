import { describe, expect, it } from "vitest";
import type { Folder } from "../../types";
import { folderRemovalCopy } from "./project-folders";

const folder: Folder = { id: "f1", key: "f1", name: "work", path: "C:\\work", computer: "this computer", machine: "this computer" };

describe("removing a project folder", () => {
  it("asks first, naming the folder, its path, machine and project, as Qt does", () => {
    const copy = folderRemovalCopy(folder, "project");
    expect(copy.title).toBe("Remove folder?");
    expect(copy.text).toBe("Remove “work” (C:\\work on this computer) from project? Files stay on disk.");
    expect(copy.action).toBe("Remove folder");
  });

  it("names an unnamed folder by its last path part", () => {
    expect(folderRemovalCopy({ ...folder, name: "" }, "project").text).toContain("Remove “work”");
  });
});
