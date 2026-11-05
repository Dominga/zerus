import type { Project } from "../types";
import type { Session } from "../ui/sessions/types";

// Task #44: the Team designer page's project, as Qt's SessionsWindow::
// showTeamDesigner and TeamDesignerPage::setProjects/setContext choose it.
// The designer is the running local session whose name ends in
// "/team-designer" (Teams::isDesignerSession). The page lists this computer's
// project folders; the selected local session supplies its project (its
// group's name, or the name's second segment) and folder, unless a running
// designer keeps the project named in its original brief.

export interface DesignerProject { id: string; name: string; folder: string; folderName: string }
export interface DesignerContext { folder: string; project: string }

export const isDesignerSession = (computer: string, name: string) => computer === "" && name.endsWith("/team-designer");

/** The running designer's row, if the session list has it. */
export function designerRow(rows: Iterable<Session>): Session | undefined {
  for (const row of rows) if (row.state === "running" && isDesignerSession(row.computer, row.name)) return row;
  return undefined;
}

/** This computer's project folders, as Qt lists them (the ungrouped inbox left out). */
export function designerProjects(projects: readonly Project[]): DesignerProject[] {
  const out: DesignerProject[] = [];
  for (const project of projects) {
    if (project.id === "ungrouped") continue;
    for (const folder of project.folders) {
      if (folder.computer === "" && folder.path) out.push({ id: project.id, name: project.name, folder: folder.path, folderName: folder.name });
    }
  }
  return out;
}

/**
 * The context a selected session gives: its folder and project, only for a
 * local row, and only while no designer runs or no folder is chosen yet.
 */
export function sessionContext(row: Session | undefined, groupName: string | undefined, running: boolean, current: DesignerContext): DesignerContext | undefined {
  if (!row || row.computer !== "" || row.state === "archived") return undefined;
  if (running && current.folder) return undefined;
  const folder = row.cwdPath || row.cwdCanonical;
  if (!folder) return undefined;
  return { folder, project: groupName || row.name.split("/")[1] || "" };
}

/**
 * The choices and the chosen one: the listed folder that matches the context
 * (by folder, then project id or name); a context folder that is not listed
 * becomes its own choice; with no context, the default project's folder,
 * else the first.
 */
export function designerChoices(projects: readonly DesignerProject[], defaultProject: string, context: DesignerContext): { choices: DesignerProject[]; chosen: number } {
  const choices = [...projects];
  if (context.folder) {
    const index = choices.findIndex((choice) => choice.folder === context.folder && (!context.project || choice.id === context.project || choice.name === context.project));
    if (index >= 0) return { choices, chosen: index };
    choices.push({ id: context.project, name: context.project, folder: context.folder, folderName: "" });
    return { choices, chosen: choices.length - 1 };
  }
  const fallback = choices.findIndex((choice) => choice.id === defaultProject);
  return { choices, chosen: fallback >= 0 ? fallback : choices.length ? 0 : -1 };
}
