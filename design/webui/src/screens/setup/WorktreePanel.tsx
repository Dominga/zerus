import { For, Show, createMemo, createSignal } from "solid-js";
import { Icon } from "../../lib/icons";
import { catalogFor, type Checkout, type FolderCatalog } from "../../mock/setup-extra";
import { setMenu, setPage, setSelectedKey, setDetailTab, toast, demoState } from "../../state";
import { machineOf } from "../sessions/model";
import { openSetup } from "./ui";
import { setCleanupFocus, setLaunchPreset } from "./store";

// Folder and worktrees panel (WorktreePanel.h): a tree of the checkouts that
// share the selected folder's repository, with the sessions using each one.
// Picker mode (Choose an existing worktree) replaces New session with Use folder.

const KIND_LABEL: Record<Checkout["kind"], string> = {
  main: "Main checkout",
  linked: "Linked worktree",
  bare: "Bare repository",
  folder: "Folder",
};

export function statusLine(c: FolderCatalog, machine: string, offline: boolean): { text: string; tone?: "warn" | "danger" } {
  if (offline || c.state === "offline") return { text: `Offline. Showing the last snapshot. Snapshot: ${c.snapshot} (outdated)`, tone: "warn" };
  switch (c.state) {
    case "loading": return { text: "Reading folder…" };
    case "plain": return { text: "Ordinary folder (no Git)." };
    case "missing": return { text: "Folder is missing or inaccessible.", tone: "danger" };
    case "unavailable": return { text: "Git information unavailable. You can still use an accessible folder.", tone: "warn" };
    case "partial": return { text: `Partial catalog (first 512 worktrees). Snapshot: ${c.snapshot}`, tone: "warn" };
    default: return { text: `Existing working folders on ${machine}. Snapshot: ${c.snapshot}` };
  }
}

export function WorktreePanel(props: {
  machine?: string;
  path?: string;
  project?: string;
  picker?: boolean;
  onUse?: (path: string) => void;
  onSelect?: (path: string) => void;
}) {
  const [picked, setPicked] = createSignal<string>();
  const [reading, setReading] = createSignal(false);
  const catalog = createMemo(() => (props.machine && props.path ? catalogFor(props.machine, props.path) : undefined));
  const offline = () => machineOf(props.machine ?? "")?.connection === "offline" || demoState() === "error";
  const checkouts = () => {
    const c = catalog();
    if (!c) return [];
    return props.picker ? c.checkouts.filter((x) => x.kind !== "bare" && !x.unavailable) : c.checkouts;
  };
  const current = () => picked() ?? checkouts()[0]?.path;
  const refresh = () => { setReading(true); setTimeout(() => setReading(false), 700); };
  const select = (path: string) => { setPicked(path); props.onSelect?.(path); };
  const linkedMenu = (e: MouseEvent, c: Checkout) => {
    e.preventDefault();
    select(c.path);
    setMenu({
      x: e.clientX, y: e.clientY, items: [
        ...(c.kind === "linked" ? [{
          label: c.unavailable ? "Forget missing worktree…" : "Remove worktree…", icon: "trash", danger: true,
          action: () => { setCleanupFocus(c.path); openSetup("worktree-cleanup"); },
        }, { separator: true as const }] : []),
        { label: "Copy path", icon: "copy", action: () => toast("Copied", c.path) },
      ],
    });
  };
  const cleanable = () => catalog()?.cleanable ?? 0;
  return (
    <section class="wtp" classList={{ picker: props.picker }} aria-label="Folder and worktrees">
      <header class="wtp-head">
        <b>Folder and worktrees</b>
        <button class="icon-btn small" data-tip="Refresh" aria-label="Refresh" onClick={refresh} disabled={!catalog()}><Icon name="refresh-cw" size={13} /></button>
        <span class="grow" />
        <Show when={!props.picker}>
          <button class="btn small" disabled={!catalog()} onClick={() => { setPage("sessions"); toast("All sessions", "The folder filter was cleared"); }}>All sessions</button>
          <button class="btn small" disabled={!catalog() || catalog()!.state !== "ok" || offline()} data-tip="Review this repository's worktrees on its computer before anything is removed"
            onClick={() => { setCleanupFocus(undefined); openSetup("worktree-cleanup"); }}>
            {cleanable() ? `Clean up (${cleanable()})…` : "Clean up…"}
          </button>
          <button class="btn small" disabled={!current() || offline()} onClick={() => { setLaunchPreset({ project: props.project, machine: props.machine, path: current() }); openSetup("new-session"); }}>
            <Icon name="plus" size={12} />New session…
          </button>
        </Show>
      </header>
      <Show when={catalog()} fallback={<div class="wtp-status">Select a folder.</div>}>
        {(c) => {
          const st = () => (reading() ? { text: "Reading folder…" } : statusLine(c(), props.machine!, offline()));
          return (
            <>
              <div class="wtp-status" classList={{ warn: st().tone === "warn", danger: st().tone === "danger" }}>{st().text}</div>
              <ul class="wtp-tree" role="tree" aria-label="Checkouts">
                <For each={checkouts()}>
                  {(co) => (
                    <li class="wtp-root" classList={{ on: current() === co.path, gone: co.unavailable }} role="treeitem" aria-selected={current() === co.path}>
                      <button class="wtp-row" onClick={() => select(co.path)} onContextMenu={(e) => linkedMenu(e, co)}
                        onDblClick={() => { if (props.picker) props.onUse?.(co.path); else { setPage("sessions"); toast("Folder filter", `Sessions in ${co.path}`); } }}
                        title={props.picker ? "Double-click to use this folder" : "Click to select; double-click to show its sessions"}>
                        <Icon name={co.kind === "main" ? "folder-git-2" : co.kind === "folder" ? "folder" : co.kind === "bare" ? "box" : "git-branch"} size={14} class="wtp-ico" />
                        <span class="wtp-title ell">
                          <b>{KIND_LABEL[co.kind]}</b>
                          <Show when={co.branch}> / {co.branch}</Show>
                          <Show when={co.detached}> / detached {co.detached}</Show>
                        </span>
                        <Show when={co.locked}><span class="wtp-tag">(locked)</span></Show>
                        <Show when={co.prunable}><span class="wtp-tag warn">(prunable)</span></Show>
                        <Show when={co.unavailable}><span class="wtp-tag danger">(unavailable)</span></Show>
                        <Show when={co.sessions.length}><span class="wtp-tag">({co.sessions.length} {co.sessions.length === 1 ? "session" : "sessions"})</span></Show>
                      </button>
                      <div class="wtp-path mono ell" title={co.path}>{co.path}</div>
                      <Show when={!props.picker && co.sessions.length}>
                        <ul class="wtp-kids" role="group">
                          <For each={co.sessions}>
                            {(s) => (
                              <li>
                                <button class="wtp-kid" disabled={!s.key} onClick={() => { if (s.key) { setSelectedKey(s.key); setDetailTab("activity"); setPage("sessions"); } }}>
                                  <Icon name="corner-down-right" size={12} class="dim" />
                                  <span>{s.name}</span>
                                  <span class="dim">({offline() && !s.state.startsWith("last") ? `last recorded: ${s.state}` : s.state})</span>
                                </button>
                              </li>
                            )}
                          </For>
                        </ul>
                      </Show>
                    </li>
                  )}
                </For>
              </ul>
            </>
          );
        }}
      </Show>
    </section>
  );
}
