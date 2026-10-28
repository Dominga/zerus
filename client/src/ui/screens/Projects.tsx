import { For, Show, createMemo, createSignal, onCleanup } from "solid-js";
import { Icon } from "../lib/icons";
import { MachineLabel } from "../components/MachineLabel";
import type { Project } from "../../types";
import { projectsPage } from "../../store/projects";
import { setMenu, type MenuEntry } from "../../store/ui";
import { NEW_SESSION, PROJECT_EDITING, WORKTREE_FLOWS } from "./reasons";
import { copy } from "../../store/actions";
import { nextSort, projectColor, shownProjects, sortFolders, type Sort, type SortColumn } from "./projects-ui";

// Projects (ProjectsDialog.cpp), ported from design/webui/src/screens/Projects.tsx:
// the shared project catalog of `hgs swarm get`, its folders on each computer
// and the swarm's status. Editing (names, colours, defaults, folders, order,
// deleting, the swarm connection) arrives with project editing (3c), New
// session with phase 5, and the Worktrees panel with phase 5.

function ProjectLabel(props: { project: Project; class?: string }) {
  return (
    <span class={`su-plabel entity ${props.class ?? ""}`} style={{ "--entity": projectColor(props.project) }}>
      <Icon name={props.project.id === "ungrouped" ? "layers" : "folder"} size={14} stroke={1.8} />
      <span class="ell">{props.project.name}</span>
      <Show when={props.project.is_default}><span class="su-pldefault">Default</span></Show>
    </span>
  );
}

const [selectedProject, setSelectedProject] = createSignal<string | undefined>();
const [selectedFolder, setSelectedFolder] = createSignal(0);
const [allSwarm, setAllSwarm] = createSignal(false);

const disabled = (label: string, icon: string, hint: string): MenuEntry => ({ label, icon, disabled: true, hint });

export function ProjectsScreen() {
  const view = projectsPage;
  const [sort, setSort] = createSignal<Sort>({ column: "name", ascending: true });
  const projects = createMemo(() => shownProjects(view(), allSwarm()));
  const project = () => projects().find((p) => p.id === selectedProject()) ?? projects().find((p) => p.accessible);
  const folders = createMemo(() => sortFolders(project()?.folders ?? [], sort()));
  onCleanup(() => setMenu(undefined));
  const choose = (p: Project) => { setSelectedProject(p.id); setSelectedFolder(0); };

  const folderMenu = (e: MouseEvent, index: number) => {
    e.preventDefault();
    setSelectedFolder(index);
    const others = projects().filter((p) => p.accessible && p.id !== project()?.id);
    setMenu({ x: e.clientX, y: e.clientY, items: [
      disabled("New session…", "plus", NEW_SESSION),
      disabled("Open folder", "folder-open", PROJECT_EDITING),
      // ProjectsDialog.cpp:565-566: always enabled, copies the path the row shows.
      { label: "Copy folder path", icon: "copy", hint: project()?.folders[index]?.path, action: () => { const path = project()?.folders[index]?.path; if (path) void copy(path); } },
      { separator: true },
      { heading: "Copy to" },
      ...others.map((p) => disabled(p.name, "folder", PROJECT_EDITING)),
      { heading: "Move to" },
      ...others.map((p) => disabled(p.name, "arrow-right", PROJECT_EDITING)),
      { separator: true },
      disabled("Edit folder…", "pencil", PROJECT_EDITING),
      disabled("Remove folder from project", "minus", PROJECT_EDITING),
      disabled("Add folder…", "plus", PROJECT_EDITING),
    ] });
  };

  const listMenu = (e: MouseEvent, id?: string) => {
    e.preventDefault();
    const p = projects().find((x) => x.id === id);
    if (p) choose(p);
    setMenu({ x: e.clientX, y: e.clientY, items: [
      disabled("New project…", "plus", PROJECT_EDITING),
      ...(p ? [{ separator: true } as const, disabled("Move project up", "arrow-up", PROJECT_EDITING), disabled("Move project down", "arrow-down", PROJECT_EDITING)] : []),
    ] });
  };

  const onListKey = (e: KeyboardEvent) => {
    if (e.altKey || (e.key !== "ArrowUp" && e.key !== "ArrowDown")) return;
    e.preventDefault();
    const list = projects().filter((p) => p.accessible);
    const i = list.findIndex((p) => p.id === project()?.id);
    const next = list[Math.min(list.length - 1, Math.max(0, i + (e.key === "ArrowUp" ? -1 : 1)))];
    if (!next) return;
    choose(next);
    (e.currentTarget as HTMLElement).querySelector<HTMLElement>(`[data-id="${CSS.escape(next.id)}"]`)?.focus();
  };

  const header = (column: SortColumn, label: string) => (
    <th aria-sort={sort().column === column ? (sort().ascending ? "ascending" : "descending") : "none"}>
      <button class="su-th" title="Click a column heading to sort folders." onClick={() => setSort((s) => nextSort(s, column))}>
        {label}<Show when={sort().column === column}><Icon name={sort().ascending ? "arrow-up" : "arrow-down"} size={11} /></Show>
      </button>
    </th>
  );

  return (
    <div class="su-page projects">
      <div class="proj-content">
        <header class="su-head">
          <h1>Projects</h1>
          <span class="grow" />
          <button class="btn" disabled title={WORKTREE_FLOWS}><Icon name="folder-git-2" size={14} />Worktrees…</button>
          <button class="btn" disabled title={PROJECT_EDITING}><Icon name="network" size={14} />Swarm…</button>
        </header>
        <p class="muted proj-hint">Organize sessions and folders across your computers. A folder can be used by several projects.</p>

        <Show when={view().notice}>{(notice) => (
          <div class="notice proj-guide">
            <Icon name="link" size={14} />
            <span class="grow">{notice()}</span>
            <button class="btn small" disabled title={PROJECT_EDITING}>Connect project catalogs…</button>
          </div>
        )}</Show>

        <div class="row proj-swarm">
          <label class="check" title="Include projects on computers without a configured connection here.">
            <input type="checkbox" checked={allSwarm()} onChange={(e) => setAllSwarm(e.currentTarget.checked)} />All swarm projects
          </label>
          <span class="muted" classList={{ danger: view().status_error }}>{view().status}</span>
        </div>

        <div class="proj-split">
          <aside class="proj-left">
            <ul class="proj-list" role="listbox" aria-label="Projects" tabindex="-1" onKeyDown={onListKey}
              onContextMenu={(e) => { if (e.target === e.currentTarget) listMenu(e); }}>
              <For each={projects()}>{(p) => (
                <li role="option" aria-selected={p.id === project()?.id}>
                  <Show when={p.accessible} fallback={<div class="proj-item remote" title={p.name}><ProjectLabel project={p} /></div>}>
                    <button class="proj-item" classList={{ on: p.id === project()?.id }} data-id={p.id} tabindex={p.id === project()?.id ? 0 : -1}
                      title={p.is_default ? `${p.name}, default project for sessions started outside Zerus` : p.name}
                      onClick={() => choose(p)} onContextMenu={(e) => listMenu(e, p.id)}>
                      <ProjectLabel project={p} />
                    </button>
                  </Show>
                </li>
              )}</For>
            </ul>
            <button class="btn" disabled title={PROJECT_EDITING}><Icon name="plus" size={14} />New project…</button>
          </aside>

          <Show when={project()}>{(p) => (
            <section class="proj-right" aria-label="Project">
              <div class="form-row">
                <label for="proj-name">Name</label>
                <div class="row">
                  <input id="proj-name" class="field proj-name" value={p().name} readOnly title={PROJECT_EDITING} />
                  <span class="dim proj-id">ID</span>
                  <span class="mono proj-id-val" title="Project ID">{p().id}</span>
                </div>
              </div>
              <div class="form-row">
                <label>Color</label>
                <div class="row">
                  <ProjectLabel project={p()} class="proj-preview" />
                  <button class="btn" disabled title={PROJECT_EDITING}><Icon name="palette" size={14} />Edit color…</button>
                </div>
              </div>
              <label class="check proj-default" title={PROJECT_EDITING}>
                <input type="checkbox" checked={p().is_default} disabled />Default for sessions started outside Zerus
              </label>

              <div class="proj-folders">
                <div class="row proj-fhead">
                  <h2 class="section-title">Folders</h2>
                  <span class="grow" />
                  <button class="btn small" disabled title={PROJECT_EDITING}><Icon name="download" size={12} />Clone repository…</button>
                  <button class="btn small" disabled title={NEW_SESSION}><Icon name="plus" size={12} />New session…</button>
                </div>
                <div class="su-tablewrap">
                  <table class="su-table proj-table">
                    <thead><tr>{header("name", "Name")}{header("machine", "Computer")}{header("path", "Folder")}</tr></thead>
                    <tbody>
                      <For each={folders()}>{(row) => (
                        <tr classList={{ on: selectedFolder() === row.index }} tabindex="0" aria-selected={selectedFolder() === row.index}
                          onClick={() => setSelectedFolder(row.index)} onContextMenu={(e) => folderMenu(e, row.index)}>
                          <td class="ell" title={row.folder.name}>{row.folder.name}</td>
                          <td><MachineLabel alias={row.folder.computer} /></td>
                          <td class="mono ell" title={row.folder.path}>{row.folder.path}</td>
                        </tr>
                      )}</For>
                    </tbody>
                  </table>
                </div>
                <div class="row proj-factions">
                  <button class="btn small" disabled title={PROJECT_EDITING}>Add folder…</button>
                  <button class="btn small" disabled title={PROJECT_EDITING}>Edit folder…</button>
                  <button class="btn small" disabled title={PROJECT_EDITING}>Remove folder</button>
                  <span class="grow" />
                  <button class="btn small" disabled title={PROJECT_EDITING}><Icon name="folder-open" size={12} />Open folder</button>
                </div>
              </div>
              <div class="row proj-foot">
                <button class="btn danger" disabled title={PROJECT_EDITING}><Icon name="trash" size={14} />Delete project…</button>
              </div>
            </section>
          )}</Show>
        </div>
      </div>
    </div>
  );
}
