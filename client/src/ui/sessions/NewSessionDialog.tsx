import { For, Show, createEffect, createMemo, createSignal, on, onCleanup, type JSX } from "solid-js";
import { AgentGlyph, Icon } from "../lib/icons";
import { MachineTile, type MachineState } from "../components/MachineTile";
import { UsageMeters } from "../components/UsageMeter";
import { closeNewSession, newSession, newSessionActions as act, startNewSession, type NewSessionState } from "../../store/newSession";
import { machineCards } from "../../store/machines";
import { catalog, agentKind } from "../../store/catalog";
import { connectionOf, osOf } from "../../store/overview";
import { now } from "../../store/ui";
import { projectColor } from "./organization-ui";
import { launchWindows, type AccountRow } from "./newsession-ui";
import { WARNING_BUTTONS, WARNING_TITLE, accountKey, computerKey, createConfirm, folderKey, projectKey } from "./newsession-view";
import type { Json } from "../lib/json";
import "../styles/newsession.css";

// New session (tray/src/NewSessionDialog.cpp, constructor 136-475 and
// updateForm 660-742), from the prototype's layout
// (design/webui/src/dialogs/setup/NewSession.tsx) where Qt does not differ.
// It renders the store's state and calls its actions; the form's rules come
// from the core through the store (`form` is `launch::shown`). Opening and
// every choice here only read; Start hands the plan to background creation.
// The child dialogs (a peer's folder browser, Choose existing…, Create
// manually…, the model picker) open over it from `newSessionChild` and the
// store's `pickerOpen`.

/** A child dialog New session opens over itself, with the context it opened in (585, 1114-1143). */
export type NewSessionChild =
  | { kind: "browse"; target: string; computer: string; project: string; path: string }
  | { kind: "browse-repository"; target: string; computer: string; parent: string }
  | { kind: "choose-worktree"; target: string; computer: string; project: string; path: string }
  | { kind: "create-worktree"; target: string; computer: string; project: string; path: string; catalog: Json; branch: string };
export const [newSessionChild, setNewSessionChild] = createSignal<NewSessionChild>();

const AGENTS = new Set(["codex", "claude", "kimi", "dsh", "opencode"]);
const sameKeys = (a: string[], b: string[]) => a.length === b.length && a.every((key, i) => key === b[i]);

/** The row whose key is `key`, keeping the last one found while a removed row is disposed. */
function rowOf<T>(find: () => T | undefined): () => T {
  return createMemo<T>((last) => find() ?? last, find() as T);
}

/**
 * A choice list with rows that a label cannot carry in a native list (the
 * project colour, the computer's OS tile). Rows are keyed: relabelling keeps
 * them and the hovered one. As Qt's combo box: arrows on the closed button
 * change the choice; Alt+Down, F4, Enter and Space open it; Escape closes it.
 */
function Choice(props: {
  id: string;
  label: string;
  keys: string[];
  selected: string | undefined;
  placeholder?: string;
  disabled: boolean;
  open: boolean;
  onOpen(open: boolean): void;
  onPick(key: string): void;
  option(key: string): JSX.Element;
  tip?(key: string): string | undefined;
  class?: string;
  initial?: boolean;
}) {
  let root!: HTMLDivElement;
  let button!: HTMLButtonElement;
  const [active, setActive] = createSignal<string>();
  const step = (from: string | undefined, by: number) => {
    const keys = props.keys;
    if (!keys.length) return undefined;
    const at = from === undefined ? -1 : keys.indexOf(from);
    if (at < 0) return by > 0 ? keys[0] : keys[keys.length - 1];
    return keys[Math.max(0, Math.min(keys.length - 1, at + by))];
  };
  const show = () => { setActive(props.selected); props.onOpen(true); };
  const pick = (key: string | undefined) => {
    props.onOpen(false);
    button.focus();
    if (key !== undefined && key !== props.selected) props.onPick(key);
  };
  const onKey = (e: KeyboardEvent) => {
    if (props.disabled) return;
    if (!props.open) {
      if ((e.key === "ArrowDown" && e.altKey) || e.key === "F4" || e.key === "Enter" || e.key === " ") { e.preventDefault(); show(); }
      else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const next = step(props.selected, e.key === "ArrowDown" ? 1 : -1);
        if (next !== undefined && next !== props.selected) props.onPick(next);
      }
      return;
    }
    if (e.key === "ArrowDown" || e.key === "ArrowUp") { e.preventDefault(); setActive(step(active(), e.key === "ArrowDown" ? 1 : -1)); }
    else if (e.key === "Home" || e.key === "End") { e.preventDefault(); setActive(e.key === "Home" ? props.keys[0] : props.keys[props.keys.length - 1]); }
    else if (e.key === "Enter" || e.key === " ") { e.preventDefault(); pick(active()); }
    // Escape reaches the dialog's cancel, which closes the open list first.
    else if (e.key === "Tab") props.onOpen(false);
  };
  createEffect(() => {
    if (!props.open) return;
    const away = (e: PointerEvent) => { if (!root.contains(e.target as Node)) props.onOpen(false); };
    document.addEventListener("pointerdown", away, true);
    onCleanup(() => document.removeEventListener("pointerdown", away, true));
  });
  createEffect(() => {
    const key = active();
    if (!props.open || key === undefined) return;
    root.querySelector(`[data-key="${CSS.escape(key)}"]`)?.scrollIntoView({ block: "nearest" });
  });
  const optionId = (key: string) => `${props.id}-o-${props.keys.indexOf(key)}`;
  return (
    <div class={`ns-choice ${props.class ?? ""}`} ref={root}>
      <button type="button" ref={button} class="field ns-choice-btn" role="combobox" aria-label={props.label} aria-haspopup="listbox"
        aria-expanded={props.open} aria-controls={`${props.id}-list`} disabled={props.disabled} data-initial={props.initial || undefined}
        aria-activedescendant={props.open && active() !== undefined ? optionId(active()!) : undefined}
        title={props.selected !== undefined ? props.tip?.(props.selected) : undefined}
        onClick={() => (props.open ? props.onOpen(false) : show())} onKeyDown={onKey}>
        <span class="ns-choice-value">
          <Show when={props.selected !== undefined} fallback={<span class="ns-placeholder">{props.placeholder}</span>}>{props.option(props.selected!)}</Show>
        </span>
        <Icon name="chevron-down" size={14} class="ns-choice-chevron" />
      </button>
      <Show when={props.open}>
        <ul class="ns-choice-list" id={`${props.id}-list`} role="listbox" aria-label={props.label}>
          <For each={props.keys}>{(key, index) => (
            <li id={`${props.id}-o-${index()}`} data-key={key} role="option" aria-selected={key === props.selected}
              classList={{ on: key === props.selected, active: key === active() }} title={props.tip?.(key)}
              onPointerEnter={() => setActive(key)} onMouseDown={(e) => e.preventDefault()} onClick={() => pick(key)}>
              {props.option(key)}
            </li>
          )}</For>
        </ul>
      </Show>
    </div>
  );
}

/** One account row (AccountChoice::Delegate): the provider's logo, the bold name, then compact meters or the summary. */
function AccountLine(props: { row: () => AccountRow; selected: boolean }) {
  const minute = createMemo(() => Math.floor(now() / 60) * 60);
  const windows = createMemo(() => launchWindows(props.row().usage, minute()));
  const provider = () => props.row().provider;
  return (
    <>
      <span class="ns-acc-logo" style={{ color: AGENTS.has(provider()) ? `var(--z-agent-${agentKind(provider())})` : undefined }} aria-hidden="true">
        <Show when={!props.row().shell && AGENTS.has(provider())} fallback={<Icon name="terminal" size={26} />}>
          <AgentGlyph agent={agentKind(provider())} size={26} />
        </Show>
      </span>
      <span class="ns-acc-text">
        <b class="ell">{props.row().name}</b>
        <Show when={windows().length} fallback={
          <span class="ns-usage ell" classList={{ [`tone-${props.row().tone}`]: !!props.row().tone, selected: props.selected }}>{props.row().summary}</span>
        }>
          <span class="ns-meters"><UsageMeters windows={windows()} compact /></span>
        </Show>
      </span>
    </>
  );
}

/** The local Windows-session warning: an alert with Launch anyway and Cancel, Cancel the default and focused. */
function LaunchWarning(props: { confirm: ReturnType<typeof createConfirm> }) {
  let el!: HTMLDialogElement;
  createEffect(on(() => props.confirm.text(), (text) => {
    if (text === undefined) { if (el.open) el.close(); return; }
    if (!el.open) el.showModal();
    queueMicrotask(() => el.querySelector<HTMLElement>("[data-initial]")?.focus());
  }));
  return (
    <dialog class="dlg ns-warning" ref={el} role="alertdialog" aria-labelledby="ns-warning-title" aria-describedby="ns-warning-text"
      onCancel={(e) => { e.preventDefault(); props.confirm.cancel(); }} onClose={() => props.confirm.cancel()}>
      <Show when={props.confirm.text()}>{(text) => (
        <>
          <div class="dlg-head"><Icon name="triangle-alert" size={18} class="warn" /><h2 id="ns-warning-title">{WARNING_TITLE}</h2></div>
          <div class="dlg-body"><p class="confirm-body" id="ns-warning-text">{text()}</p></div>
          <div class="dlg-foot">
            <span class="spacer" />
            <For each={WARNING_BUTTONS}>{(button) => (
              <button type="button" class={button.initial ? "btn primary" : "btn"} data-initial={button.initial || undefined} autofocus={button.initial}
                onClick={() => props.confirm.answer(button.answer)}>{button.label}</button>
            )}</For>
          </div>
        </>
      )}</Show>
    </dialog>
  );
}

function NewSessionForm(props: { s: () => NewSessionState; popup: () => string | undefined; setPopup(popup: string | undefined): void; confirm: ReturnType<typeof createConfirm> }) {
  const s = props.s;
  const form = () => s().form;
  // Until the core's first answer: nothing is locked, nothing can start.
  const fields = () => form()?.fields_enabled ?? true;
  const project = () => s().projects[s().project];
  const computer = () => s().computers[s().computer];
  const folder = () => s().folders[s().folder];
  const online = () => computer()?.online ?? false;
  const computerName = () => s().host || (machineCards[""]?.host ?? "");
  const opened = (name: string) => props.popup() === name;
  const toggle = (name: string) => (open: boolean) => props.setPopup(open ? name : opened(name) ? undefined : props.popup());

  // Keys keep the list rows while labels change in place.
  const projectKeys = createMemo(() => s().projects.map(projectKey), [], { equals: sameKeys });
  const computerKeys = createMemo(() => s().computers.map(computerKey), [], { equals: sameKeys });
  const folderKeys = createMemo(() => s().folders.map(folderKey), [], { equals: sameKeys });
  const accountKeys = createMemo(() => s().accounts.map(accountKey), [], { equals: sameKeys });
  const projectOf = (key: string) => s().projects.find((row) => projectKey(row) === key);
  const computerOf = (key: string) => s().computers.find((row) => computerKey(row) === key);
  const folderOf = (key: string) => s().folders.find((row) => folderKey(row) === key);

  const projectOption = (key: string) => {
    // The project's colour as the Sessions list's header shows it.
    const color = () => {
      const row = projectOf(key);
      return row && !row.newProject ? projectColor(row) : undefined;
    };
    return (
      <>
        <Show when={!projectOf(key)?.newProject}><Icon name="projects" size={15} class="ns-project-icon" style={{ color: color() }} /></Show>
        <span class="ell">{projectOf(key)?.name}</span>
      </>
    );
  };
  const tile = (target: string): MachineState => {
    if (computerOf(target)?.online) return "online";
    return connectionOf(catalog.computers[target]?.status).tile === "connecting" ? "connecting" : "offline";
  };
  const computerOption = (key: string) => (
    <>
      <MachineTile os={osOf(machineCards[key]?.platform)} state={tile(key)} size={20} />
      <span class="ell">{computerOf(key)?.label}</span>
    </>
  );
  const folderOption = (key: string) => <span class="ell">{folderOf(key)?.name}</span>;

  // Browse… (581-588, 312-316): this computer's own dialog, else the folder browser over this one.
  const browse = () => {
    if (s().host === "") { void act.browseLocal(); return; }
    setNewSessionChild({ kind: "browse", target: s().host, computer: computerName(), project: project()?.id ?? "", path: folder()?.path ?? "" });
  };
  const browseRepository = () => {
    if (s().host === "") { void act.browseRepositoryLocal(); return; }
    setNewSessionChild({ kind: "browse-repository", target: s().host, computer: computerName(), parent: s().repositoryParent });
  };
  const chooseExisting = () => {
    props.setPopup(undefined);
    if (!form()?.choose_existing_enabled) return;
    setNewSessionChild({ kind: "choose-worktree", target: s().host, computer: computerName(), project: project()?.id ?? "", path: folder()?.path ?? "" });
  };
  const createManually = () => {
    props.setPopup(undefined);
    if (!form()?.create_manually_enabled) return;
    setNewSessionChild({ kind: "create-worktree", target: s().host, computer: computerName(), project: project()?.id ?? "", path: folder()?.path ?? "", catalog: s().worktrees.catalog, branch: s().name });
  };

  // The worktree options menu: Escape is the dialog's (it closes the menu first), arrows move.
  let menu: HTMLDivElement | undefined;
  createEffect(() => {
    if (!opened("worktree")) return;
    queueMicrotask(() => menu?.querySelector<HTMLElement>(".menu-item:not([aria-disabled=true])")?.focus());
    const away = (e: PointerEvent) => { if (!menu?.parentElement?.contains(e.target as Node)) props.setPopup(undefined); };
    document.addEventListener("pointerdown", away, true);
    onCleanup(() => document.removeEventListener("pointerdown", away, true));
  });
  const menuKey = (e: KeyboardEvent) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const items = [...(menu?.querySelectorAll<HTMLElement>(".menu-item:not([aria-disabled=true])") ?? [])];
    const at = items.indexOf(document.activeElement as HTMLElement);
    items[(at + (e.key === "ArrowDown" ? 1 : -1) + items.length) % items.length]?.focus();
  };

  // The account list: arrows choose; taking the focus never does (947-949).
  // Enter goes to the default button, as in Qt's dialog.
  let accountList!: HTMLUListElement;
  let formEl!: HTMLFormElement;
  const accountsEnabled = () => fields() && !s().noMachine;
  const accountKeyDown = (e: KeyboardEvent) => {
    if (!accountsEnabled()) return;
    const count = s().accounts.length;
    if (!count) return;
    const at = s().account;
    let next: number;
    if (e.key === "ArrowDown") next = at < 0 ? 0 : Math.min(count - 1, at + 1);
    else if (e.key === "ArrowUp") next = at < 0 ? 0 : Math.max(0, at - 1);
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = count - 1;
    else if (e.key === "PageDown") next = at < 0 ? 0 : Math.min(count - 1, at + 4);
    else if (e.key === "PageUp") next = at < 0 ? 0 : Math.max(0, at - 4);
    else if (e.key === "Enter") { e.preventDefault(); formEl.requestSubmit(); return; }
    else return;
    e.preventDefault();
    act.chooseAccount(next);
    queueMicrotask(() => accountList.querySelector(".on")?.scrollIntoView({ block: "nearest" }));
  };

  // The Git repository source focuses its URL field (606).
  let url: HTMLInputElement | undefined;
  createEffect(on(() => s().focusUrl, () => queueMicrotask(() => url?.focus()), { defer: true }));

  const start = (e: Event) => {
    e.preventDefault();
    if (!form()?.start_enabled || s().starting || s().submitted) return;
    void startNewSession((text) => props.confirm.ask(text));
  };

  return (
    <form class="ns-form" ref={formEl} onSubmit={start}>
      <div class="dlg-head">
        <h2 id="ns-title">New session</h2>
        <span class="grow" />
        <button type="button" class="icon-btn" aria-label="Close" disabled={!(form()?.cancel_enabled ?? true)} onClick={() => closeNewSession()}><Icon name="x" size={16} /></button>
      </div>
      <div class="dlg-body ns-body">
        <div class="form-row">
          <label for="ns-project">Target</label>
          <div class="ns-target">
            <Choice id="ns-project" label="Project" keys={projectKeys()} selected={project() ? projectKey(project()!) : undefined} disabled={!fields()}
              open={opened("project")} onOpen={toggle("project")} option={projectOption} class="ns-project" initial
              onPick={(key) => act.chooseProject(s().projects.findIndex((row) => projectKey(row) === key))} />
            <button type="button" class="icon-btn ns-manage" data-tip="Manage this project's folders" aria-label="Manage this project's folders" disabled={!fields()}
              onClick={() => act.manageFolders()}><Icon name="settings" size={18} /></button>
            <Choice id="ns-computer" label="Computer" keys={computerKeys()} selected={computer() ? computerKey(computer()!) : undefined} disabled={!fields()}
              open={opened("computer")} onOpen={toggle("computer")} option={computerOption} class="ns-computer"
              onPick={(key) => act.chooseComputer(s().computers.findIndex((row) => computerKey(row) === key))} />
          </div>
        </div>
        <Show when={s().newProjectRow}>
          <div class="form-row">
            <label for="ns-new-project" classList={{ "ns-reserved": !s().newProjectVisible }}>New project</label>
            <Show when={s().newProjectVisible} fallback={<span class="ns-reserved-field" />}>
              <input id="ns-new-project" class="field" maxLength={80} placeholder="Project name" value={s().newProjectName} disabled={!fields()}
                onInput={(e) => act.setNewProjectName(e.currentTarget.value)} />
            </Show>
          </div>
        </Show>
        <div class="form-row">
          <label>Source</label>
          <div class="ns-seg" role="group" aria-label="Source">
            <button type="button" class="ns-segbtn" classList={{ on: !s().repository }} aria-pressed={!s().repository} disabled={!fields()} onClick={() => act.setSource(false)}>
              <Icon name="folder" size={14} />Working folder
            </button>
            <button type="button" class="ns-segbtn" classList={{ on: s().repository }} aria-pressed={s().repository} disabled={!fields()} onClick={() => act.setSource(true)}>
              <Icon name="git-branch" size={14} />Git repository
            </button>
          </div>
        </div>
        <Show when={!s().repository} fallback={
          <>
            <div class="form-row">
              <label for="ns-url">Repository</label>
              <input id="ns-url" ref={url} class="field mono" maxLength={4096} placeholder="https://github.com/owner/repository.git"
                title="An SSH or HTTPS repository URL, on GitHub or another Git server." value={s().url} disabled={!fields() || !online()}
                onInput={(e) => act.setUrl(e.currentTarget.value)} />
            </div>
            <div class="form-row top">
              <label for="ns-destination">Clone to</label>
              <div class="ns-src-detail">
                <div class="row">
                  <input id="ns-destination" class="field mono grow" maxLength={8192} value={s().destination} disabled={!fields() || !online()}
                    title="The complete new checkout path. Edit its folder name or choose a parent folder with Browse."
                    onInput={(e) => act.setDestination(e.currentTarget.value)} />
                  <button type="button" class="btn" disabled={!fields() || !online()} onClick={browseRepository}>Browse…</button>
                </div>
              </div>
            </div>
          </>
        }>
          <div class="form-row">
            <label for="ns-folder">Working folder</label>
            <div class="row">
              <Choice id="ns-folder" label="Working folder" keys={folderKeys()} selected={folder() ? folderKey(folder()!) : undefined} placeholder="Choose a folder…"
                disabled={!fields()} open={opened("folder")} onOpen={toggle("folder")} option={folderOption} tip={(key) => folderOf(key)?.tooltip} class="grow"
                onPick={(key) => act.chooseFolder(s().folders.findIndex((row) => folderKey(row) === key))} />
              <button type="button" class="btn" disabled={!(form()?.browse_enabled ?? false)} onClick={browse}>Browse…</button>
            </div>
          </div>
          <div class="form-row top">
            <span />
            <div class="ns-src-detail">
              <div class="mono ns-path ell" title={s().folderPath} style={{ visibility: s().folderPath ? "visible" : "hidden" }}>{s().folderPath || " "}</div>
              <div class="ns-preview" classList={{ warn: !!form()?.outside_project }} title={form()?.preview || undefined}>{form()?.preview ?? ""}</div>
            </div>
          </div>
        </Show>

        <div class="form-row top">
          <label>Worktree</label>
          <div class="col ns-wt">
            <div class="row">
              <div class="ns-seg" role="group" aria-label="Worktree">
                <button type="button" class="ns-segbtn" classList={{ on: !!form()?.automatic }} aria-pressed={!!form()?.automatic} disabled={!(form()?.auto_enabled ?? false)}
                  title="Create a generated branch and worktree when you start the session." onClick={() => act.setAuto(true)}>
                  <Icon name="git-branch" size={14} />Auto
                </button>
                <button type="button" class="ns-segbtn" classList={{ on: !form()?.automatic }} aria-pressed={!form()?.automatic} disabled={!(form()?.none_enabled ?? true)}
                  title="Start directly in the working folder." onClick={() => act.setAuto(false)}>
                  <Icon name="folder" size={14} />None
                </button>
              </div>
              <div class="ns-menu-anchor">
                <button type="button" class="icon-btn" data-tip="Worktree options" aria-label="Worktree options" aria-haspopup="menu" aria-expanded={opened("worktree")}
                  disabled={!(form()?.worktree_options_enabled ?? false)} onClick={() => props.setPopup(opened("worktree") ? undefined : "worktree")}>
                  <Icon name="more" size={16} />
                </button>
                <Show when={opened("worktree")}>
                  <div class="menu ns-menu" role="menu" ref={menu} onKeyDown={menuKey}>
                    <Show when={s().worktrees.chooseVisible}>
                      <button type="button" class="menu-item" role="menuitem" aria-disabled={!form()?.choose_existing_enabled} onClick={chooseExisting}>
                        <Icon name="git-branch" size={14} /><span>Choose existing…</span>
                      </button>
                    </Show>
                    <Show when={s().worktrees.createVisible}>
                      <button type="button" class="menu-item" role="menuitem" aria-disabled={!form()?.create_manually_enabled} onClick={createManually}>
                        <Icon name="add" size={14} /><span>Create manually…</span>
                      </button>
                    </Show>
                  </div>
                </Show>
              </div>
            </div>
            <div class="mono ns-wt-target" title={form()?.worktree_tooltip || undefined}>{form()?.worktree_target ?? ""}</div>
          </div>
        </div>

        <div class="form-row top">
          <label>Account and agent</label>
          <div class="col">
            <ul class="ns-accounts" ref={accountList} role="listbox" aria-label="Account and agent" tabindex={accountsEnabled() ? 0 : -1}
              aria-disabled={!accountsEnabled()} aria-activedescendant={s().account >= 0 ? `ns-account-${s().account}` : undefined} onKeyDown={accountKeyDown}>
              <For each={accountKeys()}>{(key) => {
                const row = rowOf(() => s().accounts.find((r) => accountKey(r) === key));
                const index = () => s().accounts.findIndex((r) => accountKey(r) === key);
                const selected = () => index() >= 0 && index() === s().account;
                return (
                  <li id={`ns-account-${index()}`} role="option" aria-selected={selected()} classList={{ on: selected() }} title={row().tooltip || undefined}
                    aria-label={row().accessible} onClick={() => { if (accountsEnabled()) act.chooseAccount(index()); }}>
                    <AccountLine row={row} selected={selected()} />
                  </li>
                );
              }}</For>
            </ul>
            <div class="row ns-acc-status">
              <span class="grow ns-status">{s().accountStatus}</span>
              <button type="button" class="btn small" aria-label="Refresh accounts" disabled={!s().refreshEnabled} onClick={() => act.refreshAccounts()}>
                <Icon name="refresh" size={12} />Refresh
              </button>
            </div>
          </div>
        </div>

        <Show when={s().modelVisible}>
          <div class="form-row">
            <label for="ns-model">Model</label>
            <button type="button" id="ns-model" class="field ns-model-btn" aria-label="Model" aria-haspopup="dialog" aria-expanded={s().pickerOpen}
              title="Models OpenCode offers on this computer. Native default uses OpenCode's configuration." disabled={!s().modelEnabled}
              onClick={() => (s().pickerOpen ? act.closeModelPicker() : act.openModelPicker())}>
              <span class="ell">{s().modelButton}</span><Icon name="chevron-down" size={14} class="ns-choice-chevron" />
            </button>
          </div>
        </Show>
        <Show when={s().service.visible}>
          <div class="form-row">
            <span />
            <div class="row ns-service">
              <span class="grow ns-status">{s().service.reason}</span>
              <Show when={s().service.offer}>
                <button type="button" class="btn small" title={s().service.tooltip} disabled={!s().service.enabled} onClick={() => act.startOpenCode()}>{s().service.text}</button>
              </Show>
            </div>
          </div>
        </Show>
        <Show when={s().effortVisible}>
          <div class="form-row">
            <label for="ns-effort">Effort</label>
            <select id="ns-effort" class="field ns-effort" aria-label="Effort" title="Variants of the selected model. Default uses the model's own setting."
              disabled={!fields()} value={s().effort} onChange={(e) => act.chooseEffort(e.currentTarget.value)}>
              <For each={s().efforts}>{(effort) => <option value={effort.value} selected={effort.value === s().effort}>{effort.label}</option>}</For>
            </select>
          </div>
        </Show>

        <div class="form-row">
          <label for="ns-name">Session name</label>
          <input id="ns-name" class="field mono" value={s().name} disabled={!fields()} aria-invalid={!!form()?.name_problem}
            aria-describedby={form()?.name_problem ? "ns-name-problem" : undefined} onInput={(e) => act.setName(e.currentTarget.value)} />
        </div>
        <Show when={form()?.name_problem}>
          <div class="form-row"><span /><span id="ns-name-problem" class="ns-status">{form()?.name_problem}</span></div>
        </Show>
        <div class="form-row">
          <span />
          <label class="check" title={form()?.open_terminal_tooltip || undefined}>
            <input type="checkbox" checked={s().openTerminal} disabled={!(form()?.open_terminal_enabled ?? false)} onChange={(e) => act.setOpenTerminal(e.currentTarget.checked)} />
            Open terminal window
          </label>
        </div>
      </div>
      <div class="dlg-foot">
        <span class="ns-error grow" role="status">{s().error}</span>
        <Show when={s().noMachine}>
          <button type="button" class="btn" onClick={() => act.openMachines()}>Open Machines</button>
        </Show>
        <button type="button" class="btn" disabled={!(form()?.cancel_enabled ?? true)} onClick={() => closeNewSession()}>Cancel</button>
        <button type="submit" class="btn primary ns-start" disabled={!form()?.start_enabled || s().starting || s().submitted}>{form()?.start_text ?? "Start session"}</button>
      </div>
    </form>
  );
}

/** The one New session dialog, opened by the store (`openNewSession`) and closed by Cancel, Escape or an admitted Start. */
export function NewSessionDialog() {
  let el!: HTMLDialogElement;
  const confirm = createConfirm();
  const [popup, setPopup] = createSignal<string>();
  const open = () => !!newSession();
  createEffect(on(open, (shown) => {
    if (!shown) {
      setPopup(undefined);
      setNewSessionChild(undefined);
      confirm.cancel();
      if (el.open) el.close();
      return;
    }
    if (el.open) return;
    el.showModal();
    queueMicrotask(() => {
      const first = el.querySelector<HTMLElement>("[data-initial]:not(:disabled)")
        ?? el.querySelector<HTMLElement>(".dlg-body :is(input, select, button, [tabindex='0']):not(:disabled)");
      first?.focus();
    });
  }));
  const cancelEnabled = () => newSession()?.form?.cancel_enabled ?? true;
  return (
    <>
      <dialog class="dlg ns-dlg" ref={el} aria-labelledby="ns-title"
        onCancel={(e) => {
          // Escape closes an open list or menu first, back to its button; the dialog closes only through the store.
          e.preventDefault();
          if (popup()) {
            const owner = el.querySelector<HTMLElement>("[aria-expanded='true']");
            setPopup(undefined);
            owner?.focus();
            return;
          }
          if (cancelEnabled()) closeNewSession();
        }}
        onClose={() => { if (newSession()) closeNewSession(); }}>
        <Show when={newSession()}>{(state) => <NewSessionForm s={state} popup={popup} setPopup={setPopup} confirm={confirm} />}</Show>
      </dialog>
      <LaunchWarning confirm={confirm} />
    </>
  );
}
