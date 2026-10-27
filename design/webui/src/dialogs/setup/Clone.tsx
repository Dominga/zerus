import { For, Show, createEffect, createMemo, createSignal, untrack } from "solid-js";
import { createStore } from "solid-js/store";
import { Icon } from "../../lib/icons";
import { machines } from "../../mock/data";
import { remoteDirs } from "../../mock/setup-extra";
import { toast } from "../../state";
import {
  org, addFolder, addProject, cloneMachine, setCloneMachine, dirPick, setDirPick, selectedProject, setSelectedProject, setSelectedFolder,
  projectColor, markSaved,
} from "../../screens/setup/store";
import { MockSwitch, Progress, RichSelect, SDialog, closeSetup, isOpen, join, openSetup, parentOf, replaceSetup } from "../../screens/setup/ui";
import { suggestedName } from "./NewSession";

// Clone repository (CloneRepositoryDialog.cpp) with its computer chooser,
// and the SSH-backed folder browser (DirectoryDialog.cpp). An uncertain clone
// offers only Check result or another destination; it is never repeated.
// A lost mkdir receipt is resolved by reading the folder again.

const HOME: Record<string, string> = { workstation: "/home/example", macbook: "/Users/example", "build-01": "/srv/example", "build-02": "/srv/example", "win-lab": "C:\\src\\example" };
const NEW_PROJECT = "__new__";

function CloneComputer() {
  const choices = machines.filter((m) => m.connection !== "offline");
  const [m, setM] = createSignal(untrack(cloneMachine));
  return (
    <>
      <div class="dlg-body">
        <div class="form-row">
          <label for="clone-on">Clone on computer</label>
          <RichSelect id="clone-on" label="Clone on computer" value={m()} onChange={setM}
            options={choices.map((x) => ({ value: x.alias, label: x.thisComputer ? `${x.alias} (this computer)` : x.alias, swatch: `var(--z-entity-palette-${x.colorIndex})` }))} />
        </div>
      </div>
      <div class="dlg-foot">
        <span class="grow" />
        <button class="btn" onClick={() => closeSetup("setup-clone-computer")}>Cancel</button>
        <button class="btn primary" disabled={!choices.length} onClick={() => { setCloneMachine(m()); replaceSetup("setup-clone-computer", "setup-clone"); }}>Continue</button>
      </div>
    </>
  );
}

type CloneState = "idle" | "checking" | "cloning" | "ready" | "failed" | "uncertain";

const [cloneRunning, setCloneRunning] = createSignal(false);

function CloneForm() {
  const machine = untrack(cloneMachine);
  const local = machines.find((m) => m.alias === machine)?.thisComputer;
  const [url, setUrl] = createSignal("");
  const [parent, setParent] = createSignal(HOME[machine] ?? "/home/example");
  const [name, setName] = createSignal("");
  const [nameEdited, setNameEdited] = createSignal(false);
  const [project, setProject] = createSignal(untrack(selectedProject));
  const [newName, setNewName] = createSignal("");
  const [state, setStateRaw] = createSignal<CloneState>("idle");
  const setState = (v: CloneState) => { setStateRaw(v); setCloneRunning(v === "cloning"); };
  const [outcome, setOutcome] = createSignal<"success" | "failure" | "uncertain">("success");
  const folderName = () => (nameEdited() ? name() : suggestedName(url()));
  const destination = () => (folderName() ? join(parent(), folderName()) : "");
  const busy = () => state() === "checking" || state() === "cloning";
  const locked = () => busy() || state() === "uncertain" || state() === "ready";
  const status = () => ({
    idle: "",
    checking: "Checking repository access and destination…",
    cloning: "Cloning repository…",
    ready: "Checkout is ready. Confirm to use it.",
    failed: "Repository not found or access denied. Any remaining checkout files are kept in the destination.",
    uncertain: "The original clone is still running or its completion is unknown. Check again; the operation will not be repeated.",
  })[state()];
  const canClone = () => !!url().trim() && !!folderName() && folderName() !== "." && folderName() !== ".." && (project() !== NEW_PROJECT || !!newName().trim());

  const clone = () => {
    setState("checking");
    setTimeout(() => {
      setState("cloning");
      setTimeout(() => setState(outcome() === "success" ? "ready" : outcome() === "failure" ? "failed" : "uncertain"), 1400);
    }, 900);
  };
  const useCheckout = () => {
    const target = project() === NEW_PROJECT ? addProject(newName().trim()) : project();
    addFolder(target, { name: "", machine, path: destination() });
    setSelectedProject(target);
    setSelectedFolder(org.projects.find((p) => p.id === target)!.folders.length - 1);
    markSaved();
    toast("Checkout added", `${destination()} on ${machine}`);
    closeSetup("setup-clone");
  };
  const browse = () => {
    if (local) { setParent("/home/example/src"); toast("Choose a folder", "The system folder dialog of this computer opens here"); return; }
    setDirPick({ machine, path: parent(), onPick: setParent });
    openSetup("setup-directory");
  };
  return (
    <>
      <div class="dlg-body">
        <p class="muted">Computer: {machine}</p>
        <div class="form-row"><label for="cl-url">Repository</label>
          <input id="cl-url" class="field mono" maxLength={4096} placeholder="git@example.org:team/project.git" disabled={locked()} value={url()} onInput={(e) => setUrl(e.currentTarget.value)} /></div>
        <div class="form-row"><label for="cl-parent">Parent folder</label>
          <div class="row"><input id="cl-parent" class="field mono grow" disabled={locked()} value={parent()} onInput={(e) => setParent(e.currentTarget.value)} /><button class="btn" disabled={locked()} onClick={browse}>Browse…</button></div></div>
        <div class="form-row"><label for="cl-name">Folder name</label>
          <input id="cl-name" class="field mono" maxLength={80} disabled={locked()} value={folderName()} onInput={(e) => { setNameEdited(true); setName(e.currentTarget.value); }} /></div>
        <div class="form-row"><label>Destination</label><span class="mono su-select ell" title={destination()}>{destination() || "—"}</span></div>
        <div class="form-row"><label for="cl-proj">Project</label>
          <RichSelect id="cl-proj" label="Project" value={project()} disabled={locked()} onChange={setProject}
            options={[...org.projects.map((p) => ({ value: p.id, label: p.name, swatch: projectColor(p), vivid: p.vivid })), { value: NEW_PROJECT, label: "New project…", icon: "plus" }]} /></div>
        <Show when={project() === NEW_PROJECT}>
          <div class="form-row"><label for="cl-newp">New project name</label><input id="cl-newp" class="field" maxLength={80} disabled={locked()} value={newName()} onInput={(e) => setNewName(e.currentTarget.value)} /></div>
        </Show>
        <p class="hint">Uses Git access configured on this computer. The checkout remains available if the session cannot start.</p>
        <Show when={state() !== "idle"}>
          <div class="clone-state" classList={{ warn: state() === "uncertain", danger: state() === "failed" }} role="status">{status()}</div>
          <Show when={busy()}><Progress label={status()} /></Show>
        </Show>
        <MockSwitch label="Clone result" value={outcome()} onChange={setOutcome}
          options={[{ value: "success", label: "succeeds" }, { value: "failure", label: "fails" }, { value: "uncertain", label: "unknown" }]} />
      </div>
      <div class="dlg-foot">
        <Show when={state() === "uncertain"}>
          <button class="btn" onClick={() => { setState("checking"); setTimeout(() => setState("ready"), 900); }}>Check result</button>
          <button class="btn" onClick={() => { setState("idle"); setNameEdited(true); setName(`${folderName()}-2`); }}>Choose another destination…</button>
        </Show>
        <span class="grow" />
        <button class="btn" disabled={state() === "cloning"} onClick={() => closeSetup("setup-clone")}>Cancel</button>
        <Show when={state() === "ready"} fallback={
          <button class="btn primary" disabled={!canClone() || busy() || state() === "uncertain"} onClick={clone}>Clone repository</button>
        }>
          <button class="btn primary" onClick={useCheckout}>Use checkout</button>
        </Show>
      </div>
    </>
  );
}

// ---------------------------------------------------------------------------

const [created, setCreated] = createStore<Record<string, string[]>>({});
const [newFolderBusy, setNewFolderBusy] = createSignal(false);
const [newFolderCtx, setNewFolderCtx] = createSignal<{ machine: string; parent: string; onCreated: (name: string) => void }>();

function listing(path: string): { dirs: string[]; truncated: boolean } {
  const dirs = [...(remoteDirs[path] ?? []), ...(created[path] ?? [])];
  return { dirs, truncated: path === "/srv/example/datasets" };
}

function DirectoryBrowser() {
  const ctx = untrack(dirPick);
  const home = HOME[ctx.machine] ?? "/";
  const [current, setCurrent] = createSignal(ctx.path);
  const [typed, setTyped] = createSignal(ctx.path);
  const [filter, setFilter] = createSignal("");
  const [hidden, setHidden] = createSignal(false);
  const [loading, setLoading] = createSignal(false);
  const [pick, setPick] = createSignal<string>();
  const [inline, setInline] = createSignal(false);
  const [inlineName, setInlineName] = createSignal("");
  const [justCreated, setJustCreated] = createSignal<string>();
  const sep = home.includes("\\") ? "\\" : "/";
  const go = (path: string) => {
    setLoading(true); setPick(undefined); setInline(false); setFilter("");
    setTimeout(() => { setCurrent(path); setTyped(path); setLoading(false); }, 260);
  };
  const entries = createMemo(() => {
    const { dirs } = listing(current());
    const f = filter().toLowerCase();
    return dirs.filter((d) => (hidden() || !d.startsWith(".")) && (!f || d.toLowerCase().includes(f))).slice(0, 2000);
  });
  const status = () =>
    loading() ? "Loading folders…"
    : typed() !== current() ? "Press Go to open this path, or New folder to create it."
    : justCreated() === current() ? "Folder created. Press Use this folder to select it."
    : listing(current()).truncated ? "Showing the first 2,000 folders. You can also enter an exact path above."
    : entries().length ? "Double-click to open a folder, or select it and press Use selected folder."
    : "No subfolders. You can use this folder.";
  const createInline = () => {
    const n = inlineName().trim();
    if (!n) return;
    setNewFolderBusy(true);
    setTimeout(() => {
      // The receipt is confirmed by reading the parent again, never by repeating mkdir.
      setCreated(current(), (l) => [...(l ?? []), n]);
      setNewFolderBusy(false); setInline(false); setInlineName("");
      go(join(current(), n)); setJustCreated(join(current(), n));
    }, 700);
  };
  const choose = () => { ctx.onPick?.(pick() ? join(current(), pick()!) : current()); closeSetup("setup-directory"); };
  return (
    <>
      <div class="dlg-body dir-body">
        <h3 class="section-title">Choose a folder</h3>
        <p class="muted">Folders on {ctx.machine}</p>
        <div class="row">
          <button class="btn" disabled={current() === parentOf(current())} onClick={() => go(parentOf(current()))}><Icon name="arrow-up" size={13} />Up</button>
          <button class="btn" onClick={() => go(home)}>Home</button>
          <input class="field mono grow" aria-label="Directory path" value={typed()} onInput={(e) => setTyped(e.currentTarget.value)} onKeyDown={(e) => { if (e.key === "Enter") go(typed()); }} />
          <button class="btn" onClick={() => go(typed())}>Go</button>
        </div>
        <input class="field" type="search" placeholder="Filter folders in this directory…" value={filter()} onInput={(e) => setFilter(e.currentTarget.value)} />
        <ul class="dir-list" role="listbox" aria-label="Directories">
          <Show when={!loading()} fallback={<li class="hint dir-loading">Loading folders…</li>}>
            <Show when={inline()}>
              <li class="dir-inline">
                <Icon name="folder" size={14} class="dim" />
                <input class="field grow" placeholder="Folder name" aria-label="New folder name" autofocus value={inlineName()} disabled={newFolderBusy()}
                  onInput={(e) => setInlineName(e.currentTarget.value)} onKeyDown={(e) => { if (e.key === "Enter") createInline(); if (e.key === "Escape") { e.stopPropagation(); setInline(false); } }} />
                <button class="btn small" disabled={!inlineName().trim() || newFolderBusy()} onClick={createInline}>Create</button>
                <button class="icon-btn small" aria-label="Cancel new folder" data-tip="Cancel new folder" onClick={() => setInline(false)}><Icon name="x" size={13} /></button>
              </li>
            </Show>
            <For each={entries()}>
              {(d) => (
                <li role="option" aria-selected={pick() === d} classList={{ on: pick() === d }} onClick={() => setPick(d)} onDblClick={() => go(join(current(), d))}>
                  <Icon name="folder" size={14} class="dim" /><span class="ell">{d}</span>
                </li>
              )}
            </For>
          </Show>
        </ul>
        <div class="row">
          <label class="check"><input type="checkbox" checked={hidden()} onChange={(e) => setHidden(e.currentTarget.checked)} />Show hidden folders</label>
          <span class="grow" />
          <button class="btn small" onClick={() => { setInline(true); setInlineName(""); }}><Icon name="plus" size={12} />New folder</button>
          <button class="btn small" aria-label="New folder dialog…" data-tip="New folder dialog…"
            onClick={() => { setNewFolderCtx({ machine: ctx.machine, parent: current(), onCreated: (n) => { go(join(current(), n)); setJustCreated(join(current(), n)); } }); openSetup("setup-new-folder"); }}>Dialog…</button>
        </div>
        <p class="hint" role="status">{newFolderBusy() ? `Creating folder on ${ctx.machine}…` : status()}</p>
      </div>
      <div class="dlg-foot">
        <span class="mono dim ell grow" title={current()}>{pick() ? `${current()}${current().endsWith(sep) ? "" : sep}${pick()}` : current()}</span>
        <button class="btn" onClick={() => closeSetup("setup-directory")}>Cancel</button>
        <button class="btn primary" disabled={loading() || typed() !== current()} onClick={choose}>{pick() ? "Use selected folder" : "Use this folder"}</button>
      </div>
    </>
  );
}

function NewFolderForm() {
  const ctx = untrack(newFolderCtx)!;
  const [name, setName] = createSignal("");
  const create = () => {
    setNewFolderBusy(true);
    setTimeout(() => {
      setCreated(ctx.parent, (l) => [...(l ?? []), name().trim()]);
      setNewFolderBusy(false);
      ctx.onCreated(name().trim());
      closeSetup("setup-new-folder");
    }, 700);
  };
  return (
    <>
      <div class="dlg-body">
        <p class="muted">Create a folder in <span class="mono">{ctx.parent}</span> on {ctx.machine}.</p>
        <div class="form-row"><label for="nf-name">Folder name</label>
          <input id="nf-name" class="field" aria-label="New folder name" autofocus value={name()} disabled={newFolderBusy()} onInput={(e) => setName(e.currentTarget.value)} onKeyDown={(e) => { if (e.key === "Enter" && name().trim()) create(); }} /></div>
        <Show when={newFolderBusy()}><p class="hint">Creating folder on {ctx.machine}…</p></Show>
      </div>
      <div class="dlg-foot">
        <span class="grow" />
        <button class="btn" disabled={newFolderBusy()} onClick={() => closeSetup("setup-new-folder")}>Cancel</button>
        <button class="btn primary" disabled={!name().trim() || /[/\\]/.test(name()) || newFolderBusy()} onClick={create}>Create folder</button>
      </div>
    </>
  );
}

export function CloneDialogs() {
  createEffect(() => {
    if (isOpen("setup-new-folder") && !newFolderCtx()) setNewFolderCtx({ machine: dirPick().machine, parent: dirPick().path, onCreated: () => {} });
  });
  return (
    <>
      <SDialog id="setup-clone-computer" title="Clone repository" width="460px" icon="download" bare><CloneComputer /></SDialog>
      <SDialog id="setup-clone" title="Clone repository" width="640px" icon="download" bare busy={cloneRunning}><CloneForm /></SDialog>
      <SDialog id="setup-directory" title={`Choose folder — ${dirPick().machine}`} width="620px" height="min(600px, calc(100vh - 64px))" icon="folder-open" bare><DirectoryBrowser /></SDialog>
      <SDialog id="setup-new-folder" title={`New folder — ${newFolderCtx()?.machine ?? dirPick().machine}`} width="480px" icon="folder" bare busy={newFolderBusy}>
        <Show when={newFolderCtx()}><NewFolderForm /></Show>
      </SDialog>
    </>
  );
}
