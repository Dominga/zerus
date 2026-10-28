import { describe, expect, it } from "vitest";
import { nextSort, projectColor, shownProjects, sortFolders } from "./projects-ui";
import { emptyProjects, type Folder, type Project } from "../../types";

const folder = (name: string, computer: string, path: string): Folder => ({ id: name, name, computer, machine: computer, path });
const project = (id: string, accessible: boolean, color: string | null = null): Project =>
  ({ id, name: id, color, accessible, is_default: false, palette: 3, folders: [] });

describe("projects page", () => {
  it("sorts folders by a column, case-insensitively, keeping their indices", () => {
    const folders = [folder("docs", "Windows lab", "C:\\docs"), folder("Api", "workstation", "/home/example/api"), folder("web", "studio", "/home/example/web")];
    expect(sortFolders(folders, { column: "name", ascending: true }).map((x) => x.index)).toEqual([1, 0, 2]);
    expect(sortFolders(folders, { column: "machine", ascending: true }).map((x) => x.folder.computer)).toEqual(["studio", "Windows lab", "workstation"]);
    expect(sortFolders(folders, { column: "path", ascending: false }).map((x) => x.index)).toEqual([0, 2, 1]);
    expect(nextSort({ column: "name", ascending: true }, "name")).toEqual({ column: "name", ascending: false });
    expect(nextSort({ column: "name", ascending: false }, "path")).toEqual({ column: "path", ascending: true });
  });

  it("keeps equal keys in their original order in both directions", () => {
    const folders = [folder("a", "m", "/x"), folder("b", "m", "/y"), folder("c", "n", "/z")];
    expect(sortFolders(folders, { column: "machine", ascending: false }).map((x) => x.index)).toEqual([2, 0, 1]);
    expect(sortFolders(folders, { column: "machine", ascending: true }).map((x) => x.index)).toEqual([0, 1, 2]);
  });

  it("lists remote projects only with All swarm projects and colours by the project", () => {
    const view = { ...emptyProjects(), loaded: true, projects: [project("a", true, "#3fb950"), project("b", false)] };
    expect(shownProjects(view, false).map((p) => p.id)).toEqual(["a"]);
    expect(shownProjects(view, true).map((p) => p.id)).toEqual(["a", "b"]);
    expect(projectColor(view.projects[0])).toBe("#3fb950");
    expect(projectColor(view.projects[1])).toBe("var(--z-entity-palette-3)");
  });
});
