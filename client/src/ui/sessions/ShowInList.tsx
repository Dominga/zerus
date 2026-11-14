import { batch } from "solid-js";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";
import { selected, subagentOpen, search, setSearch, stateFilter, setStateFilter, setMachineFilter, setProjectFilter,
  setAccountFilter, setFolderFilter, machineFilter, projectFilter, accountFilter, folderFilter } from "../../store/ui";
import { groupOf, headerOf, setProjectCollapsed } from "../../store/organization";
import { archive, archiveRevealScope } from "../../store/archive";
import { setTeamCollapsed, teamCollapsed } from "../../store/teamCollapse";
import { requestListReveal, setArchiveReveal, setListRevealNotice } from "../../store/listReveal";
import { listedSessionIgnoringCollapse, rowsAllOpen } from "./model";
import { compileScopes } from "./scope-filters";

/** Reveals existing list data; it neither selects another conversation nor inspects one. */
export function showInSessionList() {
  const session = selected();
  if (!session) return;
  const child = subagentOpen();
  batch(() => {
    let adjusted = !!search().trim();
    setSearch("");
    const clearScopes = () => {
      setMachineFilter([]); setProjectFilter([]); setAccountFilter([]); setFolderFilter(undefined);
    };
    let project = groupOf(session.key);
    if (session.state === "archived") {
      const group = (session as typeof session & { group?: string }).group ?? project;
      project = group;
      if (!compileScopes({ machines: machineFilter(), projects: projectFilter(), accounts: accountFilter(), folder: folderFilter() })(session, group)) {
        clearScopes(); adjusted = true;
      }
      if (stateFilter() !== "archived") { setStateFilter("archived"); adjusted = true; }
      // A search-selected row can be outside the current bounded archive page.
      // Retain that exact row, not a scan or an archive identity reconstructed from its name.
      setArchiveReveal({ session, group, scope: archiveRevealScope() });
      const known = archive.row(session.key);
      setListRevealNotice(!known || !archive.includes(known.archivedAt)
        ? tr("Showing this session outside the current archive page.")
        : adjusted ? tr("Filters adjusted to show this session.") : undefined);
    } else {
      setArchiveReveal(undefined);
      if (!listedSessionIgnoringCollapse(session.key)) {
        clearScopes(); setStateFilter("all"); adjusted = true;
      }
      // The logical all-open projection preserves parent/container chains even
      // when the project's visible projection currently contains only its header.
      const parents = new Map<string, string>();
      let currentProject: string | undefined;
      for (const row of rowsAllOpen()) {
        if (row.type === "header") currentProject = row.project;
        if (row.type === "card" || row.type === "container") {
          if (row.parentKey) parents.set(row.id, row.parentKey);
          if (row.id === session.key && currentProject) project = currentProject;
        }
      }
      const seen = new Set<string>();
      for (let parent = parents.get(session.key); parent && !seen.has(parent) && seen.size < 8; parent = parents.get(parent)) {
        seen.add(parent); if (teamCollapsed(parent)) setTeamCollapsed(parent, false);
      }
      if (child && teamCollapsed(session.key)) setTeamCollapsed(session.key, false);
      setListRevealNotice(adjusted ? tr("Filters adjusted to show this session.") : undefined);
    }
    if (headerOf(project)?.collapsed) setProjectCollapsed(project, false);
    requestListReveal(session.key, child);
  });
}

export function ShowInListButton() {
  return <button type="button" class="icon-btn small dh-locate" aria-label={tr("Show in session list")}
    data-tip={tr("Show in session list")} onClick={showInSessionList}><Icon name="circle-dot" size={15} /></button>;
}
