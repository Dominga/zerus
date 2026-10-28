import type { Folder, Project, ProjectsView } from "../../types";

// Pure rules of the Projects screen (ProjectsDialog.cpp).

export type SortColumn = "name" | "machine" | "path";
export interface Sort { column: SortColumn; ascending: boolean }

/** The folders table sorted by a column, case-insensitively; each row keeps its index in the project's folders. */
export function sortFolders(folders: readonly Folder[], sort: Sort): { folder: Folder; index: number }[] {
  const key = (f: Folder) => (sort.column === "name" ? f.name : sort.column === "machine" ? f.computer : f.path).toLowerCase();
  const direction = sort.ascending ? 1 : -1;
  return folders
    .map((folder, index) => ({ folder, index }))
    .sort((a, b) => {
      const ka = key(a.folder), kb = key(b.folder);
      return ka < kb ? -direction : ka > kb ? direction : a.index - b.index;
    });
}

/** A heading click: the same column flips, another one sorts ascending. */
export const nextSort = (sort: Sort, column: SortColumn): Sort => ({ column, ascending: sort.column === column ? !sort.ascending : true });

/** "All swarm projects" adds the projects of computers without a configured connection. */
export const shownProjects = (view: ProjectsView, all: boolean) => (all ? view.projects : view.projects.filter((p) => p.accessible));

/** The project's own colour, else its palette slot (this device's colours arrive with project editing). */
export const projectColor = (p: Project) => p.color ?? `var(--z-entity-palette-${p.palette})`;
