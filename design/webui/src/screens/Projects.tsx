import { For, Show, createMemo, createSignal, onCleanup } from "solid-js";
import { Icon } from "../lib/icons";
import { machines } from "../mock/data";
import { swarmConflicts } from "../mock/setup-extra";
import type { ProjectFolder } from "../mock/types";
import { demoState, setMenu, toast, type MenuEntry } from "../state";
import { MachineBadge } from "./sessions/SessionCard";
import { machineOf } from "./sessions/model";
import { WorktreePanel } from "./setup/WorktreePanel";
import {
  org, projectById, selectedProject, setSelectedProject, selectedFolder, setSelectedFolder, moveProject, moveProjectTo,
  setDefaultProject, removeFolder, setFolderEdit, setLaunchPreset, setOrg, setSwarmTab, setSwarmPeer, resolvedConflicts,
  setCloneMachine, addFolder, setOverview, updateProject,
} from "./setup/store";
import { ProjectLabel, openSetup, leaf } from "./setup/ui";
import "../styles/setup.css";

// Projects page (ProjectsDialog.cpp): logical projects, their folder links on
// each computer and the folder's worktrees. Removing a folder link never
// touches files, so it needs no confirmation; deleting a project does.

/** Name is saved when editing finishes; an empty name reverts (1–80 characters, one line). */
function InlineProjectName(props: { project: { id: string; name: string } }) {
  let input!: HTMLInputElement;
  const commit = () => {
    const v = input.value.replace(/[\r\n]+/g, " ").trim();
    if (!v || v.length > 80) { input.value = props.project.name; return; }
    if (v !== props.project.name) updateProject(props.project.id, { name: v });
  };
  return (
    <input ref={input} id="proj-name" class="field proj-name" maxLength={80} value={props.project.name}
      onBlur={commit} onKeyDown={(e) => { if (e.key === "Enter") { commit(); input.blur(); } if (e.key === "Escape") { input.value = props.project.name; input.blur(); } }} />
  );
}

const LOCAL = machines.find((m) => m.thisComputer)!.alias;
const SPLIT_KEY = "zerus.projectFolderSplit";

function readSplit(): number {
  try { return Number(localStorage.getItem(SPLIT_KEY)) || 230; } catch { return 230; }
}

type SortCol = "name" | "machine" | "path";

export function ProjectsScreen() {
  const [allSwarm, setAllSwarm] = createSignal(false);
  const [sort, setSort] = createSignal<{ col: SortCol; asc: boolean }>({ col: "name", asc: true });
  const [problem, setProblem] = createSignal("");
  const [split, setSplit] = createSignal(readSplit());
  const [dragging, setDragging] = createSignal<string>();
  const [dropAt, setDropAt] = createSignal<number>();

  const project = createMemo(() => projectById(selectedProject()) ?? org.projects[0]);
  const folder = (): ProjectFolder | undefined => project()?.folders[selectedFolder()];
  const sortedFolders = createMemo(() => {
    const p = project();
    if (!p) return [];
    const { col, asc } = sort();
    const key = (f: ProjectFolder) => (col === "name" ? f.name || leaf(f.path) : col === "machine" ? f.machine : f.path).toLowerCase();
    return p.folders.map((f, i) => ({ f, i })).sort((a, b) => (key(a.f) < key(b.f) ? -1 : key(a.f) > key(b.f) ? 1 : 0) * (asc ? 1 : -1));
  });
  const unresolved = () => swarmConflicts.filter((c) => !resolvedConflicts[c.id]).length;
  const swarmStatus = () => (demoState() === "error" ? "Project sync unavailable: SSH request timed out" : "Project sync connected to 3 computer(s)");

  const say = (text: string) => { setProblem(text); setOrg("saved", ""); };
  const openFolder = () => {
    const f = folder();
    if (!f) return say("Select a folder to open.");
    if (demoState() === "loading") return say("Waiting for this computer's identity.");
    if (f.machine !== LOCAL) return say(`Open this folder from Zerus on ${f.machine}.`);
    setProblem("");
    toast("Open folder", `Opens ${f.path} in the file manager`);
  };
  const newSession = (f = folder()) => {
    if (!f) return;
    if (demoState() === "loading") return say("Waiting for this computer's identity.");
    if (machineOf(f.machine)?.connection === "offline") return say(`Reconnect ${f.machine} to start a session in this folder.`);
    setProblem("");
    setLaunchPreset({ project: project()!.id, machine: f.machine, path: f.path });
    openSetup("new-session");
  };
  const editFolder = (index = selectedFolder()) => { if (!project()?.folders[index]) return; setFolderEdit({ project: project()!.id, index }); openSetup("setup-folder"); };
  const addFolderDlg = () => { setFolderEdit({ project: project()!.id, index: -1 }); openSetup("setup-folder"); };
  const remove = (index = selectedFolder()) => {
    const p = project();
    if (!p?.folders[index]) return;
    removeFolder(p.id, index);
    setSelectedFolder(Math.max(0, index - 1));
    setProblem("");
  };
  const copyTo = (target: string, move: boolean) => {
    const p = project(); const f = folder();
    if (!p || !f) return;
    addFolder(target, { ...f });
    if (move) remove();
    toast(move ? "Folder moved" : "Folder copied", `${f.name || leaf(f.path)} → ${projectById(target)?.name}`);
  };

  const folderMenu = (e: MouseEvent, index: number) => {
    e.preventDefault();
    setSelectedFolder(index);
    const f = project()!.folders[index];
    const others = org.projects.filter((p) => p.id !== project()!.id);
    const items: MenuEntry[] = [
      { label: "New session…", icon: "plus", action: () => newSession(f) },
      { label: "Open folder", icon: "folder-open", disabled: f.machine !== LOCAL, action: openFolder },
      { label: "Copy folder path", icon: "copy", action: () => toast("Copied", f.path) },
      { separator: true },
      { heading: "Copy to" },
      ...others.map((p) => ({ label: p.name, icon: "folder", action: () => copyTo(p.id, false) })),
      { heading: "Move to" },
      ...others.map((p) => ({ label: p.name, icon: "arrow-right", action: () => copyTo(p.id, true) })),
      { separator: true },
      { label: "Edit folder…", icon: "pencil", action: () => editFolder(index) },
      { label: "Remove folder from project", icon: "minus", action: () => remove(index) },
      { label: "Add folder…", icon: "plus", action: addFolderDlg },
    ];
    setMenu({ x: e.clientX, y: e.clientY, items });
  };

  const listMenu = (e: MouseEvent, id?: string) => {
    e.preventDefault();
    if (id) setSelectedProject(id);
    const i = org.projects.findIndex((p) => p.id === id);
    setMenu({
      x: e.clientX, y: e.clientY, items: [
        { label: "New project…", icon: "plus", action: () => openSetup("setup-new-project") },
        ...(id ? [
          { separator: true } as const,
          { label: "Move project up", icon: "arrow-up", kbd: "Alt+↑", disabled: i <= 0, action: () => moveProject(id, -1) },
          { label: "Move project down", icon: "arrow-down", kbd: "Alt+↓", disabled: i >= org.projects.length - 1, action: () => moveProject(id, 1) },
        ] : []),
      ],
    });
  };

  const onListKey = (e: KeyboardEvent) => {
    const i = org.projects.findIndex((p) => p.id === selectedProject());
    if (e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) { e.preventDefault(); moveProject(selectedProject(), e.key === "ArrowUp" ? -1 : 1); return; }
    if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      e.preventDefault();
      const next = org.projects[Math.min(org.projects.length - 1, Math.max(0, i + (e.key === "ArrowUp" ? -1 : 1)))];
      if (next) { setSelectedProject(next.id); setSelectedFolder(0); (e.currentTarget as HTMLElement).querySelector<HTMLElement>(`[data-id="${next.id}"]`)?.focus(); }
    }
  };

  // Folder / worktree splitter; its height is a per-viewer convenience.
  const startSplit = (e: PointerEvent) => {
    const startY = e.clientY, start = split();
    const move = (ev: PointerEvent) => setSplit(Math.max(140, Math.min(window.innerHeight - 360, start + ev.clientY - startY)));
    const up = () => { removeEventListener("pointermove", move); removeEventListener("pointerup", up); try { localStorage.setItem(SPLIT_KEY, String(split())); } catch { /* storage unavailable */ } };
    addEventListener("pointermove", move);
    addEventListener("pointerup", up);
  };
  onCleanup(() => setMenu(undefined));

  const header = (col: SortCol, label: string) => (
    <th aria-sort={sort().col === col ? (sort().asc ? "ascending" : "descending") : "none"}>
      <button class="su-th" title="Click a column heading to sort folders." onClick={() => setSort((s) => ({ col, asc: s.col === col ? !s.asc : true }))}>
        {label}<Show when={sort().col === col}><Icon name={sort().asc ? "chevron-down" : "arrow-up"} size={11} /></Show>
      </button>
    </th>
  );

  return (
    <div class="su-page projects">
      <div class="proj-content">
        <header class="su-head">
          <h1>Projects</h1>
          <span class="grow" />
          <button class="btn" data-tip="View worktrees across projects and computers, and review cleanup." onClick={() => { setOverview("selected", []); openSetup("setup-worktrees"); }}>
            <Icon name="folder-git-2" size={14} />Worktrees…
          </button>
          <button class="btn" onClick={() => { setSwarmTab("computers"); openSetup("setup-swarm"); }}><Icon name="network" size={14} />Swarm…</button>
        </header>
        <p class="muted proj-hint">Organize sessions and folders across your computers. A folder can be used by several projects.</p>

        <div class="notice proj-guide">
          <Icon name="link" size={14} />
          <span class="grow">Session connections for win-lab do not share project catalogs yet. Preview the other catalog, match projects, then explicitly join to share their grouping.</span>
          <button class="btn small" onClick={() => { setSwarmTab("connect"); setSwarmPeer("win-lab"); openSetup("setup-swarm"); }}>Connect project catalogs…</button>
        </div>

        <div class="row proj-swarm">
          <label class="check" title="Include projects on computers without a configured connection here.">
            <input type="checkbox" checked={allSwarm()} onChange={(e) => setAllSwarm(e.currentTarget.checked)} />All swarm projects
          </label>
          <span class="muted" classList={{ danger: demoState() === "error" }}>{swarmStatus()}</span>
          <Show when={unresolved() > 0}>
            <button class="su-link warn" onClick={() => { setSwarmTab("conflicts"); openSetup("setup-swarm"); }}>{unresolved()} project conflict(s) need review in Swarm.</button>
          </Show>
        </div>

        <div class="proj-split">
          <aside class="proj-left">
            <ul class="proj-list" role="listbox" aria-label="Projects; drag to reorder, or use Alt+Up and Alt+Down" tabindex="-1"
              onKeyDown={onListKey} onContextMenu={(e) => { if (e.target === e.currentTarget) listMenu(e); }}>
              <For each={org.projects}>
                {(p, i) => (
                  <li role="option" aria-selected={p.id === selectedProject()} classList={{ "drop-before": dropAt() === i() && dragging() !== p.id }}
                    onDragOver={(e) => { e.preventDefault(); setDropAt(i()); }}
                    onDrop={(e) => { e.preventDefault(); const id = dragging(); if (id) moveProjectTo(id, i()); setDragging(undefined); setDropAt(undefined); }}>
                    <button class="proj-item" classList={{ on: p.id === selectedProject(), dragging: dragging() === p.id }} data-id={p.id}
                      tabindex={p.id === selectedProject() ? 0 : -1} draggable="true"
                      title={`${p.isDefault ? `${p.name}, default project for sessions started outside Zerus` : p.name}\nDrag to reorder, or use Alt+Up / Alt+Down.`}
                      onDragStart={(e) => { setDragging(p.id); e.dataTransfer?.setData("text/plain", p.id); }}
                      onDragEnd={() => { setDragging(undefined); setDropAt(undefined); }}
                      onClick={() => { setSelectedProject(p.id); setSelectedFolder(0); setProblem(""); }}
                      onContextMenu={(e) => listMenu(e, p.id)}>
                      <ProjectLabel project={p} />
                    </button>
                  </li>
                )}
              </For>
              <Show when={allSwarm()}>
                <li role="option" aria-selected="false">
                  <div class="proj-item remote" title="Studio renders (studio)">
                    <ProjectLabel project={{ id: "studio-renders", name: "Studio renders", colorIndex: 5, vivid: false, isDefault: false }} />
                    <MachineBadge alias="studio" />
                  </div>
                </li>
              </Show>
            </ul>
            <button class="btn" onClick={() => openSetup("setup-new-project")}><Icon name="plus" size={14} />New project…</button>
          </aside>

          <Show when={project()}>
            {(p) => (
              <section class="proj-right" aria-label="Project">
                <div class="form-row">
                  <label for="proj-name">Name</label>
                  <div class="row">
                    <InlineProjectName project={p()} />
                    <span class="dim proj-id">ID</span>
                    <span class="mono proj-id-val" title="Project ID">{p().id}</span>
                  </div>
                </div>
                <div class="form-row">
                  <label>Color</label>
                  <div class="row">
                    <ProjectLabel project={p()} class="proj-preview" />
                    <button class="btn" onClick={() => openSetup("setup-project-color")}><Icon name="palette" size={14} />Edit color…</button>
                  </div>
                </div>
                <label class="check proj-default">
                  <input type="checkbox" checked={p().isDefault} onClick={(e) => { if (!e.currentTarget.checked) { e.preventDefault(); return; } setDefaultProject(p().id); }} />
                  Default for sessions started outside Zerus
                </label>

                <div class="proj-folders" style={{ height: `${split()}px` }}>
                  <div class="row proj-fhead">
                    <h2 class="section-title">Folders</h2>
                    <span class="grow" />
                    <button class="btn small" onClick={() => { setCloneMachine(LOCAL); openSetup("setup-clone-computer"); }}><Icon name="download" size={12} />Clone repository…</button>
                    <button class="btn small" disabled={!folder()} onClick={() => newSession()}><Icon name="plus" size={12} />New session…</button>
                  </div>
                  <div class="su-tablewrap">
                    <table class="su-table proj-table">
                      <thead><tr>{header("name", "Name")}{header("machine", "Computer")}{header("path", "Folder")}</tr></thead>
                      <tbody>
                        <For each={sortedFolders()}>
                          {({ f, i }) => (
                            <tr classList={{ on: selectedFolder() === i }} tabindex="0" aria-selected={selectedFolder() === i}
                              onClick={() => { setSelectedFolder(i); setProblem(""); }} onDblClick={() => editFolder(i)} onContextMenu={(e) => folderMenu(e, i)}
                              onKeyDown={(e) => { if (e.key === "Enter") editFolder(i); if (e.key === "Delete") remove(i); }}>
                              <td class="ell" title={f.name || leaf(f.path)}>{f.name || leaf(f.path)}</td>
                              <td><MachineBadge alias={f.machine} /></td>
                              <td class="mono ell" title={f.path}>{f.path}</td>
                            </tr>
                          )}
                        </For>
                      </tbody>
                    </table>
                  </div>
                  <div class="row proj-factions">
                    <button class="btn small" onClick={addFolderDlg}>Add folder…</button>
                    <button class="btn small" disabled={!folder()} onClick={() => editFolder()}>Edit folder…</button>
                    <button class="btn small" disabled={!folder()} data-tip="Remove this folder from the project. Files stay in place." onClick={() => remove()}>Remove folder</button>
                    <span class="grow" />
                    <button class="btn small" disabled={!folder()} onClick={openFolder}><Icon name="folder-open" size={12} />Open folder</button>
                  </div>
                </div>
                <div class="su-hsplit" role="separator" aria-orientation="horizontal" title="Drag to resize folders and worktrees." onPointerDown={startSplit} />
                <WorktreePanel machine={folder()?.machine} path={folder()?.path} project={p().id} />
                <div class="row proj-foot">
                  <button class="btn danger" disabled={org.projects.length < 2} onClick={() => openSetup("setup-delete-project")}>
                    <Icon name="trash" size={14} />Delete project…
                  </button>
                </div>
              </section>
            )}
          </Show>
        </div>
        <div class="proj-status" role="status" classList={{ warn: !!problem() }}>
          {problem() || org.saved || (demoState() === "error" ? "Could not add 1 hgs project folder(s) of build-02: computer unavailable" : "")}
        </div>
      </div>
    </div>
  );
}
