import { For, Show, createSignal, untrack } from "solid-js";
import { Icon } from "../../lib/icons";
import { machines } from "../../mock/data";
import { PROJECT_SWATCHES } from "../../mock/setup-extra";
import { toast } from "../../state";
import {
  org, projectById, selectedProject, setSelectedProject, setSelectedFolder, addProject, updateProject, deleteProject, defaultProject,
  folderEdit, addFolder, replaceFolder, hasFolder, setDirPick,
} from "../../screens/setup/store";
import { ProjectLabel, RichSelect, SDialog, closeSetup, openSetup } from "../../screens/setup/ui";

// Project dialogs from ProjectsDialog.cpp: New project, the color popup,
// Add/Edit project folder and the Delete project confirmation.

function NewProjectForm() {
  const [name, setName] = createSignal("");
  const ok = () => {
    const n = name().trim();
    if (!n) return;
    const id = addProject(n);
    setSelectedProject(id);
    setSelectedFolder(0);
    closeSetup("setup-new-project");
  };
  return (
    <>
      <div class="dlg-body">
        <div class="form-row"><label for="np-name">Name</label>
          <input id="np-name" class="field" maxLength={80} autofocus value={name()} onInput={(e) => setName(e.currentTarget.value)} onKeyDown={(e) => { if (e.key === "Enter") ok(); }} /></div>
      </div>
      <div class="dlg-foot">
        <span class="grow" />
        <button class="btn" onClick={() => closeSetup("setup-new-project")}>Cancel</button>
        <button class="btn primary" disabled={!name().trim()} onClick={ok}>OK</button>
      </div>
    </>
  );
}

const toHex = (h: number, s: number, l: number) => {
  const a = (s / 100) * Math.min(l / 100, 1 - l / 100);
  const f = (n: number) => {
    const k = (n + h / 30) % 12;
    const c = l / 100 - a * Math.max(-1, Math.min(k - 3, 9 - k, 1));
    return Math.round(c * 255).toString(16).padStart(2, "0");
  };
  return `#${f(0)}${f(8)}${f(4)}`;
};

function ColorForm() {
  const p = untrack(() => projectById(selectedProject()) ?? org.projects[0]);
  // A palette slot is resolved to the skin's current value, as Qt starts from the stored color.
  const slot = getComputedStyle(document.documentElement).getPropertyValue(`--z-entity-palette-${p.colorIndex}`).trim();
  const initial = (p.color ?? (/^#[0-9a-f]{6}$/i.test(slot) ? slot : PROJECT_SWATCHES[0])).toLowerCase();
  const [hex, setHex] = createSignal(initial);
  const [vivid, setVivid] = createSignal(p.vivid);
  const [more, setMore] = createSignal(false);
  const [hsl, setHsl] = createSignal({ h: 160, s: 60, l: 55 });
  const valid = () => /^#[0-9a-fA-F]{6}$/.test(hex());
  const applyHsl = (patch: Partial<{ h: number; s: number; l: number }>) => { const v = { ...hsl(), ...patch }; setHsl(v); setHex(toHex(v.h, v.s, v.l)); };
  return (
    <>
      <div class="dlg-body pc-body">
        <ProjectLabel project={{ ...p, color: valid() && hex() !== initial ? hex() : p.color, vivid: vivid() }} class="pc-preview" />
        <div class="pc-swatches" role="group" aria-label="Preset colors">
          <For each={PROJECT_SWATCHES}>
            {(c) => <button class="pc-sw" classList={{ on: hex().toLowerCase() === c }} style={{ background: c }} aria-label={c} data-tip={c} onClick={() => setHex(c)} />}
          </For>
        </div>
        <div class="form-row"><label for="pc-hex">Color</label><input id="pc-hex" class="field mono pc-hex" maxLength={7} value={hex()} aria-invalid={!valid()} onInput={(e) => setHex(e.currentTarget.value)} /></div>
        <div class="form-row"><label for="pc-style">Style</label>
          <select id="pc-style" class="field" value={vivid() ? "bright" : "soft"} onChange={(e) => setVivid(e.currentTarget.value === "bright")}>
            <option value="soft">Soft fill</option><option value="bright">Bright fill</option>
          </select></div>
        <button class="btn small pc-more" aria-expanded={more()} onClick={() => setMore(!more())}><Icon name="palette" size={12} />More colors…</button>
        <Show when={more()}>
          <div class="pc-picker" role="group" aria-label="Color picker">
            <label>Hue<input type="range" min="0" max="359" value={hsl().h} onInput={(e) => applyHsl({ h: +e.currentTarget.value })} /></label>
            <label>Saturation<input type="range" min="0" max="100" value={hsl().s} onInput={(e) => applyHsl({ s: +e.currentTarget.value })} /></label>
            <label>Lightness<input type="range" min="10" max="90" value={hsl().l} onInput={(e) => applyHsl({ l: +e.currentTarget.value })} /></label>
          </div>
        </Show>
      </div>
      <div class="dlg-foot">
        <span class="grow" />
        <button class="btn" onClick={() => closeSetup("setup-project-color")}>Cancel</button>
        <button class="btn primary" disabled={!valid()} onClick={() => { updateProject(p.id, { color: hex().toLowerCase(), vivid: vivid() }); closeSetup("setup-project-color"); }}>Apply</button>
      </div>
    </>
  );
}

const LOCAL = machines.find((m) => m.thisComputer)!.alias;

function FolderForm() {
  const ctx = untrack(folderEdit);
  const existing = ctx.index >= 0 ? projectById(ctx.project)?.folders[ctx.index] : undefined;
  const [machine, setMachine] = createSignal(existing?.machine ?? LOCAL);
  const [name, setName] = createSignal(existing?.name ?? "");
  const [path, setPath] = createSignal(existing?.path ?? "");
  const absolute = () => path().startsWith("/") || /^[A-Za-z]:\\/.test(path());
  const duplicate = () => hasFolder(ctx.project, machine(), path(), ctx.index);
  const save = () => {
    const f = { name: name().trim(), machine: machine(), path: path().trim() };
    if (ctx.index >= 0) replaceFolder(ctx.project, ctx.index, f);
    else { addFolder(ctx.project, f); setSelectedFolder((projectById(ctx.project)?.folders.length ?? 1) - 1); }
    closeSetup("setup-folder");
  };
  const browse = () => {
    if (machine() === LOCAL) { setPath("/home/example/src/tools"); toast("Choose a folder", "The system folder dialog of this computer opens here"); return; }
    setDirPick({ machine: machine(), path: path() || (machine() === "macbook" ? "/Users/example" : "/srv/example"), onPick: setPath });
    openSetup("setup-directory");
  };
  return (
    <>
      <div class="dlg-body">
        <div class="form-row"><label for="pf-machine">Computer</label>
          <RichSelect id="pf-machine" label="Computer" value={machine()} onChange={setMachine}
            options={machines.map((m) => ({ value: m.alias, label: m.thisComputer ? `${m.alias} (this computer)` : m.alias, swatch: `var(--z-entity-palette-${m.colorIndex})`, detail: m.connection === "offline" ? "(offline)" : undefined }))} /></div>
        <div class="form-row"><label for="pf-name">Name</label><input id="pf-name" class="field" placeholder="Optional label" value={name()} onInput={(e) => setName(e.currentTarget.value)} /></div>
        <div class="form-row"><label for="pf-path">Folder</label>
          <div class="row">
            <input id="pf-path" class="field mono grow" placeholder="Absolute folder path" value={path()} aria-invalid={duplicate()} title={duplicate() ? "This folder is already in this project." : undefined} onInput={(e) => setPath(e.currentTarget.value)} />
            <button class="btn" onClick={browse}>Browse…</button>
          </div></div>
        <Show when={duplicate()}><div class="form-row"><span /><span class="danger hint">This folder is already in this project.</span></div></Show>
      </div>
      <div class="dlg-foot">
        <span class="grow" />
        <button class="btn" onClick={() => closeSetup("setup-folder")}>Cancel</button>
        <button class="btn primary" disabled={!absolute() || duplicate()} onClick={save}>{ctx.index >= 0 ? "Save folder" : "Add folder"}</button>
      </div>
    </>
  );
}

function DeleteProjectForm() {
  const id = untrack(selectedProject);
  const p = projectById(id) ?? org.projects[0];
  const others = org.projects.filter((x) => x.id !== p.id);
  const fallback = defaultProject();
  const [target, setTarget] = createSignal(fallback && fallback.id !== p.id ? fallback.id : others[0]?.id ?? "");
  return (
    <>
      <div class="dlg-body">
        <p class="confirm-body">Delete “{p.name}”? Choose the project that will receive its sessions. Files and running agents stay in place.</p>
        <RichSelect label="Project that receives the sessions" value={target()} onChange={setTarget}
          options={others.map((x) => ({ value: x.id, label: x.name, swatch: `var(--z-entity-palette-${x.colorIndex})`, detail: x.isDefault ? "Default" : undefined }))} />
      </div>
      <div class="dlg-foot">
        <span class="grow" />
        <button class="btn" autofocus onClick={() => closeSetup("setup-delete-project")}>Cancel</button>
        <button class="btn solid-danger" disabled={!target()} onClick={() => { deleteProject(p.id, target()); closeSetup("setup-delete-project"); }}>Move sessions and delete</button>
      </div>
    </>
  );
}

export function ProjectDialogs() {
  return (
    <>
      <SDialog id="setup-new-project" title="New project" width="420px" icon="folder" bare><NewProjectForm /></SDialog>
      <SDialog id="setup-project-color" title="Project color" width="420px" icon="palette" lightDismiss bare><ColorForm /></SDialog>
      <SDialog id="setup-folder" title={folderEdit().index >= 0 ? "Edit project folder" : "Add project folder"} width="560px" icon="folder" bare><FolderForm /></SDialog>
      <SDialog id="setup-delete-project" title="Delete project" width="500px" icon="triangle-alert" bare><DeleteProjectForm /></SDialog>
    </>
  );
}
