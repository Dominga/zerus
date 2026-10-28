import { createEffect, createMemo, createRoot, createSignal, on, untrack } from "solid-js";
import * as bridge from "../bridge";
import type {
  Account, ComputerId, CreationAdmission, Details, Folder, LaunchAccounts, LaunchChoices, LaunchForm, LaunchPlan, LaunchProject, LaunchShown, MachineMeta,
  NewSessionContext, NewSessionFormInput, NewSessionFormView, PathStyle, Project, ProjectsView, RepositoryFields,
} from "../types";
import type { Session } from "../ui/sessions/types";
import {
  accountRows, computerChoice, computerRows, folderLaunchProblem, folderRows, projectChoice, projectRows, refreshRows,
  type AccountChoice, type AccountRow, type ComputerRow, type Fleet, type FolderRow, type ProjectRow,
} from "../ui/sessions/newsession-ui";
import { markFavorite } from "../ui/sessions/opencode-models";
import { array, object, string, type Json } from "../ui/lib/json";
import { createOpenCode, type OpenCodeView } from "./newSessionOpencode";
import { absolute, clean, matchingProjectFolder, native, of, prefillTarget, restoredFolder } from "../ui/sessions/paths";
import { catalog, computerOffline } from "./catalog";
import { machineCards, requestMachine } from "./machines";
import { projectsPage, requestProject } from "./projects";
import { detailsFor, followedFor, notify } from "./actions";
import { machineFilter, selected, setPage } from "./ui";
import { snapshotOf } from "../ui/sessions/actions-ui";

// The one open New session dialog (tray/src/NewSessionDialog.cpp with
// SessionsWindow::showNewSession): its members and slots, ported as Qt
// keeps them, minus the foreground path that task executors use (ruling D2).
// Rules the core has go through its commands: the form's derived state and
// Start (`new_session_form`, `new_session_start`), the plan, the repository
// fields, the Windows warning and every read. This keeps what Qt keeps in
// its views and the request bookkeeping: each read carries its context, and
// an answer for a dialog, computer, folder or request that has moved on is
// dropped. Opening, choosing and reading never create anything; only Start
// submits, once (`creation_submit`). Nothing outlives the dialog.

/** What New session is opened with (`showNewSession`'s arguments). */
export interface NewSessionOptions {
  agent?: string;
  /** A project id or name: it wins, and only it makes the project explicit (2921). */
  project?: string;
  /** "" or the alias; the single selected computer filter when left out (`selectedHostTarget`). */
  computer?: string;
  /** A project folder id (`selectFolder`). */
  folder?: string;
  /** A folder path (`selectPath(path, false)`). */
  path?: string;
  /** An account to preselect; "" is the agent's native account. */
  account?: string | null;
  /** A failed creation's plan (Edit choices…, `restoreCreationChoices`). */
  recipe?: Record<string, unknown>;
}

/** The commands the store sends (bridge.ts); tests record their own. */
export type NewSessionBridge = Pick<typeof bridge,
  | "newSessionContext" | "newSessionForm" | "newSessionStart" | "newSessionPlan" | "launchWarning" | "repositoryFields" | "worktreeSnapshot"
  | "worktreeCatalog" | "pickLocalFolder" | "localFolderExists" | "openCodeModels" | "openCodeFavorite" | "openCodeStart" | "launchAccounts"
  | "launchAccountRows" | "launchAccountsClose" | "onLaunchAccountsChanged" | "creationSubmit">;

export interface NewSessionDeps {
  bridge: NewSessionBridge;
  /** Machine cards by computer ("" is this computer). */
  cards(): Record<ComputerId, MachineMeta>;
  /** The configured peers' aliases. */
  peers(): string[];
  /** The computer's card is ok. */
  online(computer: ComputerId): boolean;
  projects(): ProjectsView;
  /** The selected session row and its details (Qt's m_details), for a generic open. */
  selection(): { row: Session; details?: Details } | undefined;
  /** The Sessions computer filters as computers ("" is this computer). */
  hostFilters(): ComputerId[];
  notify(tone: "error" | "warn" | "info", text: string, sticky?: boolean): void;
  /** Projects on that project (Manage this project's folders). */
  openProjects(project: string): void;
  /** Machines on this computer (Open Machines). */
  openMachines(): void;
  /** Runs `run` every `ms`; returns its stop. */
  every(ms: number, run: () => void): () => void;
  /** Unix seconds. */
  now(): number;
}

export type { EffortOption, OpenCodeView } from "./newSessionOpencode";

/** What the dialog shows (Task 3 renders it); its OpenCode part is `OpenCodeView`. */
export interface NewSessionState extends OpenCodeView {
  projects: ProjectRow[];
  project: number;
  /** The New project name field shows (a repository going to New project…). */
  newProjectVisible: boolean;
  newProjectName: string;
  computers: ComputerRow[];
  computer: number;
  /** The selected computer: "" or the alias. */
  host: string;
  noMachine: boolean;
  /** The source is a Git repository. */
  repository: boolean;
  url: string;
  /** The Clone to field. */
  destination: string;
  /** Counts each time the URL field should take the focus (606). */
  focusUrl: number;
  folders: FolderRow[];
  folder: number;
  /** The selected folder in its computer's form, "" for none. */
  folderPath: string;
  /** Auto is preferred (else None). */
  autoPreferred: boolean;
  worktrees: {
    /** The folder's catalog (Choose existing… and Create manually… read it). */
    catalog: Json;
    /** Choose existing… shows when the catalog lists worktrees, Create manually… when its state is ok. */
    chooseVisible: boolean;
    createVisible: boolean;
  };
  accounts: AccountRow[];
  /** The selected row, -1 for none. */
  account: number;
  /** "Loading accounts…", "Could not load accounts: …", "Could not start OpenCode: …" or "". */
  accountStatus: string;
  refreshEnabled: boolean;
  /** The selected row's agent, else the intended one; its account, "" for none. */
  agent: string;
  accountId: string;
  name: string;
  branch: string;
  openTerminal: boolean;
  /** The core's view of the form for the latest state (`new_session_form`); undefined until it answers. */
  form: LaunchShown | undefined;
  /** The error line: the core's notice, else the last transient error. */
  error: string;
  /** Start is running its steps. */
  starting: boolean;
  /** Start submitted the plan (`m_creationSubmitted`). */
  submitted: boolean;
}

/** What the dialog's controls do. Each is a no-op without an open dialog. */
export interface NewSessionActions {
  chooseProject(index: number): void;
  chooseComputer(index: number): void;
  chooseFolder(index: number): void;
  selectPath(path: string, automaticWorktree?: boolean): void;
  setSource(repository: boolean): void;
  setUrl(text: string): void;
  setDestination(text: string): void;
  /** Browse… for the clone's parent folder chose `path`. */
  setRepositoryParent(path: string): void;
  setNewProjectName(text: string): void;
  setAuto(on: boolean): void;
  chooseAccount(index: number): void;
  selectAccount(account: string): void;
  refreshAccounts(): void;
  setName(text: string): void;
  setOpenTerminal(on: boolean): void;
  chooseModel(value: string): void;
  chooseEffort(value: string): void;
  openModelPicker(): void;
  closeModelPicker(): void;
  /** Use native default model (338). */
  useNativeModel(): void;
  pickerClick(value: string, star: boolean): void;
  pickerSelect(value: string): void;
  pickerToggleAll(): void;
  pickerSearch(text: string): void;
  startOpenCode(): void;
  /** Browse… on this computer: the system folder dialog. */
  browseLocal(): Promise<void>;
  /** A folder browser on `target` chose `path` (applies only while computer and project are unchanged). */
  applyBrowsedFolder(target: string, project: string, path: string): void;
  /** Choose existing… chose `path`. */
  chooseExistingWorktree(target: string, project: string, path: string): void;
  /** Create manually… created `path`; `verified` is `launch::created_catalog` of the catalog it was created from. */
  worktreeCreated(target: string, project: string, path: string, verified: Json): void;
  manageFolders(): void;
  openMachines(): void;
}

/** Every control, checked complete by its type. */
const ACTION_KEYS: Record<keyof NewSessionActions, true> = {
  chooseProject: true, chooseComputer: true, chooseFolder: true, selectPath: true, setSource: true, setUrl: true,
  setDestination: true, setRepositoryParent: true, setNewProjectName: true, setAuto: true, chooseAccount: true, selectAccount: true,
  refreshAccounts: true, setName: true, setOpenTerminal: true, chooseModel: true, chooseEffort: true, openModelPicker: true,
  closeModelPicker: true, useNativeModel: true, pickerClick: true, pickerSelect: true, pickerToggleAll: true, pickerSearch: true,
  startOpenCode: true, browseLocal: true, applyBrowsedFolder: true, chooseExistingWorktree: true, worktreeCreated: true, manageFolders: true,
  openMachines: true,
};
const ACTIONS = Object.keys(ACTION_KEYS) as (keyof NewSessionActions)[];
const EMPTY: Json = Object.freeze({}) as Json;
const AGENTS = ["codex", "claude", "kimi", "dsh", "opencode"];
const INBOX = "ungrouped";
const FOLDER_CHANGED = "This project folder changed. Select it again.";
/** The verified catalogs one dialog keeps (Qt keeps them for the dialog's life; this bounds them). */
const VERIFIED = 64;
const USAGE_MS = 60_000;

const errorText = (error: unknown) => (typeof error === "string" ? error : error instanceof Error ? error.message : String(error));

function emptyMeta(): MachineMeta {
  return {
    host: null, hgs_version: null, platform: null, ok: null, local_sessions: null, local_host: null,
    metrics: { cpu_percent: null, cpu_count: null, cpu_state: null, cpu_interval_seconds: null, load_1: null, load_5: null, load_15: null, memory_used_bytes: null, memory_total_bytes: null, memory_available_bytes: null, uptime_seconds: null, sampled_at: null },
    presence: { idle_seconds: null, locked: null, locked_seconds: null },
  };
}

/** Where the dialog's two leaving buttons go once it closed (281-282, 2955-2956). */
interface Leave {
  /** Manage this project's folders. */
  folders(project: string): void;
  /** Open Machines. */
  machines(): void;
}

/** One open dialog: Qt's NewSessionDialog members and slots. */
function createDialog(deps: NewSessionDeps, context: NewSessionContext, init: { host: string; agent: string; project: string }, emit: () => void, leave: Leave) {
  const b = deps.bridge;
  let closed = false;
  let ticket = 0;
  const next = () => ++ticket;

  // Agents and the intended choice (221-224, 263).
  const agents = [...AGENTS];
  if (init.agent && init.agent !== "sh" && !agents.includes(init.agent)) agents.push(init.agent);
  const lastAgent = context.remembered.agent;
  let preferredAgent = init.agent || (agents.includes(lastAgent) || lastAgent === "sh" ? lastAgent : "codex");
  let preferredAccount = "";
  let accountPreset = false;
  let projectExplicit = false;
  let swarm = "";
  const branch = context.branch;
  let name = branch;
  let openTerminal = context.remembered.open_terminal;

  // Projects, computers and folders.
  let projects: Project[] = [];
  let defaultProject = INBOX;
  let projectList: ProjectRow[] = [];
  let projectIndex = -1;
  let machines: ComputerRow[] = [];
  let machineIndex = -1;
  let folders: FolderRow[] = [];
  let folderIndex = -1;
  let folderHost = "";
  let folderAvailable = false;
  let folderContext = "";
  const browsed = new Map<string, string>();

  // The repository source.
  let repositorySource = false, customDestination = false, customProjectName = false, repositoryNewProject = false;
  let url = "", field = "", newProjectName = "";
  let repository: { host: string; style: PathStyle; fields: RepositoryFields | undefined; request: number; pending: boolean } | undefined;
  let focusUrl = 0;
  /** Repository answers to requests up to this one leave the project name as the recipe set it (794-799). */
  let nameFloor = 0;

  // Accounts.
  let accountHost = "";
  let accountRequest = 0;
  let accountLoaded = false;
  let accounts: Json[] = [];
  let groups: Account[] = [];
  let accountStatus = "";
  let rows: AccountRow[] = [];
  let accountIndex = -1;
  let accountStyle: PathStyle | undefined;
  /** The account request whose core answer came before its catalog was filed: it stays loading (944-950). */
  let accountAwaiting = 0;
  /** The newest rows read for new limits; an older answer is dropped. */
  let usageRequest = 0;

  // Worktrees and the prefill.
  let autoPreferred = true;
  let worktreeCatalog: Json = {};
  const verified = new Map<string, Json>();
  let worktreeContext: string | undefined;
  let catalogRequest = 0;
  let snapshotRequest = 0;
  let staleAfterSnapshot = false;
  let prefillRequest = 0;
  let prefillPath = "", prefillCanonical = "", prefillContext = "";

  // Start and the form.
  let error = "";
  let errorAt = 0;
  let submitted = false;
  let starting = false;
  let predecessor = "";
  let formView: NewSessionFormView | undefined;
  let formAt = 0;
  let formRevision = 0;
  let formQueued = false;

  const cards = () => deps.cards();
  const localCard = () => cards()[""];
  const localHostName = () => localCard()?.host ?? "";
  const host = () => machines[machineIndex]?.target ?? "";
  /** Qt's folderStyle: this computer's platform for this computer, POSIX for a peer. */
  const style = (computer: string): PathStyle => of(localCard()?.platform, computer === "", context.build_style);
  const online = (computer: string) => deps.online(computer);
  const fleet = (): Fleet => ({ local: localCard(), peers: deps.peers(), online });
  const projectRow = () => projectList[projectIndex];
  const projectId = () => projectRow()?.id ?? "";
  const isNewProject = () => !!projectRow()?.newProject;
  const group = (id: string) => (id ? projects.find((p) => p.id === id) : undefined);
  const path = () => folders[folderIndex]?.path ?? "";
  const folderId = () => folders[folderIndex]?.id ?? "";
  const agent = () => rows[accountIndex]?.provider ?? preferredAgent;
  const account = () => rows[accountIndex]?.id ?? "";
  const implicitInbox = () => !projectExplicit && projectId() === INBOX;
  const machineName = () => host() || localHostName();

  /** The project as the core reads it: folders named by "" for this computer. */
  function launchProject(project: Project | undefined): LaunchProject | null {
    if (!project) return null;
    const local = localHostName();
    return { id: project.id, name: project.name, folders: project.folders.map((f) => ({ machine: f.machine === local ? "" : f.machine, path: f.path })) };
  }

  /** A transient error; a later answer of the core with its notice replaces it, an earlier one never does. */
  function setError(text: string) {
    error = text;
    errorAt = formRevision + (formQueued ? 1 : 0);
  }

  // The form (updateForm 660-742): the core answers for the latest state only.

  function formInput(): NewSessionFormInput {
    const fields = repository?.fields;
    const form: LaunchForm = {
      no_machine: machines.length === 0, can_enable_local_sessions: false, online: online(host()), agent: agent(), name,
      account_count: rows.length, account_selected: accountIndex >= 0, account_loading: accountRequest !== 0, account_preset: accountPreset,
      path: path(), listed_folder: folderId() !== "", folder_online: folderIndex >= 0 && folderAvailable, prefill_pending: prefillRequest !== 0,
      catalog_pending: catalogRequest !== 0, catalog_worktrees: worktreeCatalog.stale !== true && array(worktreeCatalog.worktrees).length > 0,
      auto_preferred: autoPreferred, creation_failed: false, implicit_inbox: implicitInbox(), branch, style: style(host()), stage: "idle",
      repository: repositorySource ? {
        busy: false, cloning: false, can_submit: !!fields && !repository?.pending && fields.can_submit, created: false, uncertain: false, fields_locked: false,
        new_project: isNewProject(), new_project_name: newProjectName, destination: fields?.destination ?? "",
      } : null,
      clone_launch_pending: false,
    };
    return {
      form, catalog: worktreeCatalog, verified: verified.get(`${host()}\n${path()}`) ?? null,
      project: launchProject(group(projectId())), host: host(), local: localCard() ?? null,
    };
  }

  function updateForm() {
    syncRepositoryComputer();
    emit();
    if (formQueued || closed) return;
    formQueued = true;
    // One question per turn, for the state the turn ended with.
    queueMicrotask(() => {
      formQueued = false;
      if (closed) return;
      const revision = ++formRevision;
      b.newSessionForm(formInput()).then((view) => {
        if (closed || revision !== formRevision) return;
        formView = view;
        formAt = revision;
        // The core's standing notice replaces the error the form showed before it (719-731).
        if (view.shown.notice && revision > errorAt) error = "";
        emit();
      }, () => {});
    });
  }

  // Projects, computers and folders.

  function setGroups(list: Project[], selectedProject: string) {
    prefillRequest = 0;
    const keepNew = repositorySource && isNewProject() && !selectedProject;
    projects = list;
    defaultProject = list.find((p) => p.is_default)?.id ?? INBOX;
    projectList = projectRows(projects, repositorySource);
    projectIndex = projectChoice(projectList, { keepNew, selected: selectedProject, initial: init.project, fallback: defaultProject });
    loadMachines();
    loadFolders();
  }

  function projectChanged() {
    prefillRequest = 0;
    loadMachines();
    loadFolders();
  }

  function machineChanged() {
    prefillRequest = 0;
    accountPreset = false;
    loadFolders();
  }

  function folderChanged() {
    prefillRequest = 0;
    browsed.delete(host());
    loadAccounts();
    updateModels();
    updateForm();
    updateWorktrees();
  }

  function setProjectIndex(index: number) {
    if (index === projectIndex) return;
    projectIndex = index;
    projectChanged();
  }

  function setMachineIndex(index: number) {
    if (index === machineIndex) return;
    machineIndex = index;
    machineChanged();
  }

  function loadMachines() {
    const previous = machineIndex < 0 ? init.host : host();
    // Fleet polls refresh labels in place; the choice stays.
    machines = computerRows(fleet(), previous, group(projectId()));
    machineIndex = computerChoice(machines, previous);
  }

  function loadFolders(selectedPath = "") {
    const contextKey = `${projectId()}\n${host()}`;
    // Refreshes retain the displayed folder, but only Browse choices cross projects.
    const previous = contextKey === folderContext ? path() : "";
    folderContext = contextKey;
    const st = style(host());
    const custom = selectedPath || previous || browsed.get(host()) || "";
    const built = folderRows(group(projectId()), host(), localHostName(), st, custom);
    const same = built.rows.length === folders.length && folderHost === host()
      && built.rows.every((row, i) => row.id === folders[i].id && row.path === folders[i].path && row.name === folders[i].name);
    // Messages stay until the folder choice changes.
    if (!same || folderIndex !== built.index) error = "";
    if (!same) { folders = built.rows; folderHost = host(); }
    folderAvailable = online(host());
    folderIndex = built.index;
    loadAccounts();
    updateModels();
    updateForm();
    updateWorktrees();
  }

  function selectFolder(id: string): boolean {
    prefillRequest = 0;
    const project = group(projectId());
    if (!project) return false;
    const found = project.folders.find((f) => f.id === id);
    if (!found) return false;
    const target = found.machine === localHostName() ? "" : found.machine;
    const machine = machines.findIndex((m) => m.target === target);
    if (machine < 0) return false;
    setMachineIndex(machine);
    const index = folders.findIndex((f) => f.id === id);
    if (index < 0) return false;
    browsed.delete(host());
    if (index !== folderIndex) { folderIndex = index; folderChanged(); }
    updateForm();
    return true;
  }

  function selectPath(target: string, automaticWorktree = true) {
    prefillRequest = 0;
    const st = style(host());
    if (!absolute(target, st)) return;
    if (!automaticWorktree) autoPreferred = false;
    browsed.delete(host());
    loadFolders(clean(target, st));
  }

  function applyBrowsedFolder(target: string, project: string, chosen: string) {
    if (!chosen || host() !== target || projectId() !== project) return;
    selectPath(chosen);
    browsed.set(target, clean(chosen, style(target)));
  }

  // The repository source (590-639).

  function setRepositorySource(enabled: boolean) {
    if (repositorySource === enabled) return;
    prefillRequest = 0;
    if (repositorySource && !enabled) repositoryNewProject = isNewProject();
    repositorySource = enabled;
    const newIndex = projectList.findIndex((p) => p.newProject);
    if (enabled && newIndex < 0) projectList = projectRows(projects, true);
    else if (!enabled && newIndex >= 0) {
      if (projectIndex === newIndex) setProjectIndex(Math.max(0, projectList.findIndex((p) => !p.newProject && p.id === defaultProject)));
      projectList = projectList.filter((p) => !p.newProject);
    }
    // Folder mode needs a real project; keep the repository-only choice until
    // the user explicitly chooses a project there, even the same fallback Inbox.
    if (enabled && repositoryNewProject) setProjectIndex(projectList.findIndex((p) => p.newProject));
    syncRepositoryComputer();
    updateForm();
    if (enabled) focusUrl++;
  }

  function askFields(destination: string, custom: boolean, fresh: string | null) {
    if (!repository) return;
    const record = repository;
    const request = next();
    record.request = request;
    record.pending = true;
    b.repositoryFields(url, destination, record.style, custom, fresh).then((fields) => {
      if (closed || repository !== record || record.request !== request) return;
      record.fields = fields;
      record.pending = false;
      field = fields.field;
      if (!customProjectName && request > nameFloor) newProjectName = fields.name.slice(0, 80);
      updateForm();
    }, () => {
      if (closed || repository !== record || record.request !== request) return;
      record.pending = false;
      updateForm();
    });
  }

  /** A new computer or style starts fresh fields with the URL and the old name (608-627). */
  function syncRepositoryComputer() {
    if (!repositorySource) return;
    const st = style(host());
    if (repository && repository.host === host() && repository.style === st) return;
    const fresh = repository ? (repository.fields?.name ?? "") : null;
    repository = { host: host(), style: st, fields: undefined, request: 0, pending: true };
    customDestination = false;
    askFields("", false, fresh);
  }

  function setUrl(text: string) {
    url = text.slice(0, 4096);
    if (repository) askFields(field, customDestination, null);
    else updateForm();
  }

  function setDestination(text: string) {
    customDestination = true;
    field = text.slice(0, 8192);
    if (repository) askFields(field, true, null);
    else updateForm();
  }

  function setRepositoryParent(parent: string) {
    if (!repository || !parent) return;
    field = native(`${parent}/${repository.fields?.name ?? ""}`, repository.style);
    askFields(field, true, null);
  }

  // Worktrees (1081-1143).

  function updateWorktrees() {
    const folder = path();
    const on = online(host());
    const key = `${host()}\n${folder}\n${on ? 1 : 0}`;
    if (key === worktreeContext) return;
    worktreeContext = key;
    catalogRequest = 0;
    staleAfterSnapshot = false;
    worktreeCatalog = {};
    const computer = host();
    const snapshot = next();
    snapshotRequest = snapshot;
    b.worktreeSnapshot(computer, folder).then((saved) => {
      if (closed || snapshot !== snapshotRequest) return;
      snapshotRequest = 0;
      worktreeCatalog = staleAfterSnapshot ? { ...object(saved), stale: true } : object(saved);
      updateForm();
    }, () => { if (snapshot === snapshotRequest) snapshotRequest = 0; });
    if (folder && on) {
      const request = next();
      catalogRequest = request;
      b.worktreeCatalog(computer, folder, false).then((data) => {
        if (closed || request !== catalogRequest || computer !== host() || folder !== path()) return;
        catalogRequest = 0;
        snapshotRequest = 0;
        worktreeCatalog = object(data);
        updateForm();
      }, () => {
        if (closed || request !== catalogRequest) return;
        catalogRequest = 0;
        if (snapshotRequest) staleAfterSnapshot = true;
        else worktreeCatalog = { ...worktreeCatalog, stale: true };
        updateForm();
      });
    }
    updateForm();
  }

  function keepVerified(key: string, catalog: Json) {
    verified.delete(key);
    verified.set(key, catalog);
    while (verified.size > VERIFIED) verified.delete(verified.keys().next().value as string);
  }

  function chooseExistingWorktree(target: string, project: string, chosen: string) {
    if (!chosen || host() !== target || projectId() !== project) return;
    autoPreferred = false;
    selectPath(chosen);
  }

  function worktreeCreated(target: string, project: string, created: string, catalog: Json) {
    if (!created) return;
    if (host() !== target || projectId() !== project) {
      setError(`Worktree created at ${created} on ${target || localHostName()}. Select that folder to start a session.`);
      emit();
      return;
    }
    // Successful creation confirms membership before the catalog refresh lists it; only in this dialog.
    keepVerified(`${target}\n${created}`, catalog);
    autoPreferred = false;
    selectPath(created);
  }

  // The prefill from the selected session (1045-1080).

  function prefillSessionFolder(folder: string, canonical: string) {
    const st = style(host());
    if (!absolute(folder, st)) return;
    prefillPath = clean(folder, st);
    prefillCanonical = absolute(canonical, st) ? clean(canonical, st) : "";
    selectPath(matchingProjectFolder(prefillPath, prefillCanonical, launchProject(group(projectId())), host(), st));
    prefillContext = `${projectId()}\n${host()}\n${path()}`;
    if (online(host())) {
      const request = next();
      prefillRequest = request;
      const computer = host(), wanted = prefillPath;
      b.worktreeCatalog(computer, wanted, true).then((data) => {
        if (closed || !prefillRequest || request !== prefillRequest || computer !== host() || wanted !== prefillPath) return;
        prefillRequest = 0;
        finishFolderPrefill(object(data));
        updateForm();
      }, () => {
        if (closed || !prefillRequest || request !== prefillRequest) return;
        prefillRequest = 0;
        updateForm();
      });
    }
    updateForm();
  }

  function finishFolderPrefill(data: Json) {
    const key = `${projectId()}\n${host()}\n${path()}`;
    if (key !== prefillContext || !online(host())) return;
    const target = prefillTarget(data, prefillPath, prefillCanonical, launchProject(group(projectId())), host(), style(host()));
    if (target === null) return;
    keepVerified(`${host()}\n${target}`, data);
    selectPath(target);
  }

  // Accounts (865-1023).

  function loadAccounts(refresh = false) {
    if (!machines.length) {
      accountRequest = 0;
      accountLoaded = false;
      accounts = [];
      groups = [];
      updateAccounts();
      return;
    }
    if (accountHost === host() && (accountRequest || (!refresh && accountLoaded))) return;
    if (accountHost !== host()) {
      accounts = [];
      groups = [];
      accountPreset = false;
      accountLoaded = false;
    }
    accountHost = host();
    accountStatus = "Loading accounts…";
    const request = next();
    accountRequest = request;
    accountAwaiting = 0;
    const computer = host();
    b.launchAccounts(computer, refresh, init.agent || null).then((answer: LaunchAccounts) => {
      if (closed || request !== accountRequest || computer !== host()) return;
      // The core waited without a filed reply: Qt's request stays pending until hgs
      // answers, so the list keeps loading and the reply's event brings the catalog.
      if (!answer.loaded) {
        accountAwaiting = request;
        checkArrival();
        return;
      }
      catalogArrived(answer);
    }, (failure) => {
      if (closed || request !== accountRequest || computer !== host()) return;
      accountRequest = 0;
      accountAwaiting = 0;
      accountLoaded = true;
      accountStatus = `Could not load accounts: ${errorText(failure)}`;
      updateAccounts();
    });
    updateAccounts();
  }

  /** The computer's catalog arrived (Qt's accountsReady, 380-388): rows are grouped from it. */
  function catalogArrived(answer: LaunchAccounts) {
    accountRequest = 0;
    accountAwaiting = 0;
    usageRequest = 0;
    accountLoaded = true;
    accounts = answer.profiles.slice(0, 256);
    groups = answer.groups;
    accountStatus = answer.error ?? "";
    updateAccounts();
  }

  /** While the request awaits its filed reply, the rows read without a read take its place once they are loaded. */
  function checkArrival() {
    const request = accountAwaiting;
    if (!request || closed) return;
    const computer = host();
    b.launchAccountRows(computer).then((answer) => {
      if (closed || request !== accountRequest || request !== accountAwaiting || computer !== host() || !answer.loaded) return;
      catalogArrived(answer);
    }, () => {});
  }

  /** The hub's catalog or limits changed for the dialog's computer, or the minute passed. */
  function rowsChanged() {
    if (accountAwaiting && accountAwaiting === accountRequest) checkArrival();
    else refreshUsage();
  }

  function cachedEmail(provider: string, id: string): string {
    const raw = accounts.find((p) => p.provider === provider && p.id === id);
    return string(object(object(raw?.account_status).identity).email);
  }

  function choiceOf(): AccountChoice {
    const current = rows[accountIndex];
    return {
      host: host(), agent: preferredAgent, account: preferredAccount, preset: accountPreset,
      current: current && !current.shell ? { host: current.host, provider: current.provider, id: current.id } : undefined,
      remembered: object(object(context.remembered.accounts)[host()]) as Record<string, string>,
      loading: accountRequest !== 0, style: style(host()), cachedEmail, now: deps.now(),
    };
  }

  function updateAccounts() {
    if (!machines.length) {
      rows = [];
      accountIndex = -1;
      updateModels();
      updateForm();
      return;
    }
    accountStyle = style(host());
    const built = accountRows(groups, choiceOf());
    rows = built.rows;
    accountIndex = built.selected;
    updateModels();
    updateForm();
  }

  /** New limits change rows in place: rows, choice and order stay (refreshUsage 990-1000). */
  function refreshUsage() {
    if (!machines.length || closed) return;
    const computer = host();
    const request = next();
    usageRequest = request;
    b.launchAccountRows(computer).then((answer) => {
      if (closed || request !== usageRequest || computer !== host() || accountHost !== computer) return;
      usageRequest = 0;
      // The catalog's grouping stays until the dialog reads the next catalog.
      groups = groups.map((g) => {
        const fresh = answer.groups.find((f) => f.key === g.key);
        return fresh ? { ...g, usage: fresh.usage, members: g.members.map((m) => fresh.members.find((f) => f.provider === m.provider && f.id === m.id) ?? m) } : g;
      });
      rows = refreshRows(rows, groups, choiceOf());
      emit();
    }, () => {});
  }

  function chooseAccount(index: number) {
    if (index === accountIndex || index < -1 || index >= rows.length) return;
    accountIndex = index;
    preferredAgent = agent();
    preferredAccount = account();
    accountPreset = true;
    updateModels();
    updateForm();
  }

  function selectAccount(id: string) {
    preferredAccount = id || `native-${preferredAgent}`;
    accountPreset = true;
    updateAccounts();
  }

  function refreshAccounts() {
    // Refresh asks again for folders whose last answer was not a ready catalog.
    oc.dropUnready();
    loadAccounts(true);
  }

  // OpenCode (1145-1313).

  function openCodeProfile(): Json {
    if (agent() !== "opencode") return EMPTY;
    return accounts.find((p) => p.provider === "opencode" && p.id === account()) ?? EMPTY;
  }

  // OpenCode's models, folder catalogs, favorites and service (1145-1313).
  const oc = createOpenCode({
    bridge: b, host, path, agent, machineName, openCodeProfile, online, emit, updateForm,
    accountsLoading: () => accountRequest !== 0,
    reloadAccounts: () => loadAccounts(true),
    markRowFavorite(value, favorite) {
      accounts = accounts.map((profile) => (profile.provider === "opencode" && profile.id === account() && "model_options" in profile
        ? { ...profile, model_options: markFavorite(profile.model_options, value, favorite) } : profile));
    },
    setAccountStatus: (text) => { accountStatus = text; },
    showError: (text) => setError(text),
    clearError: (text) => { if (error === text) error = ""; },
  });
  const updateModels = () => oc.updateModels();

  // Start (743-786).

  function choices(): LaunchChoices {
    const card = cards()[host()];
    const fields = repository?.fields;
    const model = oc.model();
    return {
      host: host(), machine: card?.host || host(), agent: agent(), account: account(), name, project: projectId(),
      project_name: projectRow()?.name ?? "", implicit_inbox: implicitInbox(), swarm, path: path(), auto_preferred: autoPreferred,
      automatic_available: false, branch, catalog: worktreeCatalog, automatic_path: "", listed_folder: folderId() !== "", project_worktree: false,
      model, effort: model ? oc.effort() : "", open_terminal: openTerminal,
      repository: repositorySource ? {
        url: fields?.url ?? url, parent: fields?.parent ?? "", name: fields?.name ?? "", destination: fields?.destination ?? "",
        created_path: "", new_project: isNewProject(), project_name: newProjectName.trim(),
      } : null,
      predecessor,
    };
  }

  async function start(confirm: (text: string) => Promise<boolean>): Promise<{ id: string; plan: LaunchPlan } | undefined> {
    if (closed || starting || submitted) return undefined;
    starting = true;
    emit();
    try {
      if ((await b.newSessionStart(formInput(), submitted)) !== "background" || closed) return undefined;
      // Spec D5: ask before this computer's session host of another Windows
      // session starts the agent. Cancel keeps the form; nothing else runs.
      const warning = await b.launchWarning(localCard() ?? emptyMeta(), host());
      if (closed) return undefined;
      if (warning && !(await confirm(warning))) return undefined;
      if (closed) return undefined;
      // The form and its choices are read in one turn, as Qt builds the plan (751-771).
      const input = formInput();
      const picked = choices();
      if ((await b.newSessionStart(input, submitted)) !== "background" || closed) return undefined;
      // One Start submits once: set before the submission, cleared only by a refusal.
      submitted = true;
      emit();
      const plan = await b.newSessionPlan(input, picked);
      if (closed) return undefined;
      let admission: CreationAdmission;
      try {
        admission = await b.creationSubmit(plan);
      } catch (failure) {
        // A lost call is an unknown outcome: it stays submitted, and nothing sends it again.
        if (!closed) { setError(errorText(failure)); updateForm(); }
        return undefined;
      }
      if ("refused" in admission) {
        submitted = false;
        if (!closed) { setError(admission.refused); updateForm(); }
        return undefined;
      }
      return { id: admission.admitted, plan };
    } catch (failure) {
      // Nothing was sent: Start's questions and the plan create nothing.
      if (closed) return undefined;
      submitted = false;
      setError(errorText(failure));
      updateForm();
      return undefined;
    } finally {
      starting = false;
      if (!closed) emit();
    }
  }

  /** Edit choices…: a failed creation's choices (787-808). */
  function restoreCreationChoices(plan: Json) {
    predecessor = string(plan.id);
    name = string(plan.tag);
    openTerminal = plan.terminal === true;
    selectAccount(string(plan.account));
    if (plan.repository === true && plan.clone_done !== true) {
      setRepositorySource(true);
      setUrl(string(plan.url));
      setDestination(string(plan.target));
      if (plan.new_project === true) {
        setProjectIndex(projectList.findIndex((p) => p.newProject));
        newProjectName = string(plan.project_name).slice(0, 80);
        // Qt sets the name last; the destination's answers still on their way must not replace it.
        nameFloor = ticket;
      }
    } else {
      const [folder, auto] = restoredFolder(plan);
      selectPath(folder, auto);
    }
    if (string(plan.model)) oc.restore(string(plan.model), string(plan.effort));
    updateForm();
  }

  /** Fleet updates relabel in place and keep the choices (setFleet 478-486). */
  function fleetChanged() {
    if (style(host()) !== accountStyle) updateAccounts(); else refreshUsage();
    loadMachines();
    loadFolders();
  }

  function state(): NewSessionState {
    const notice = formView?.shown.notice ?? null;
    const project = projectRow();
    return {
      projects: projectList, project: projectIndex, newProjectVisible: repositorySource && !!project?.newProject, newProjectName,
      computers: machines, computer: machineIndex, host: host(), noMachine: machines.length === 0,
      repository: repositorySource, url, destination: field, focusUrl,
      folders, folder: folderIndex, folderPath: native(path(), style(host())), autoPreferred,
      worktrees: { catalog: worktreeCatalog, chooseVisible: array(worktreeCatalog.worktrees).length > 0, createVisible: worktreeCatalog.state === "ok" },
      accounts: rows, account: accountIndex, accountStatus, refreshEnabled: machines.length > 0 && !accountRequest,
      agent: agent(), accountId: account(),
      ...oc.view(),
      name, branch, openTerminal, form: formView?.shown,
      error: notice && !(error && errorAt >= formAt) ? notice : error,
      starting, submitted,
    };
  }

  const events = b.onLaunchAccountsChanged((computer) => {
    // The hub's catalog or limits changed for the dialog's computer: rows change in place.
    if (!closed && computer === host() && accountHost === computer) rowsChanged();
  });
  const timer = deps.every(USAGE_MS, () => { if (!closed) rowsChanged(); });

  function dispose() {
    if (closed) return;
    closed = true;
    events();
    timer();
    oc.dispose();
    void b.launchAccountsClose().catch(() => {});
  }

  const actions: NewSessionActions = {
    chooseProject(index) {
      if (index < 0 || index >= projectList.length) return;
      setProjectIndex(index);
      // Activation is explicit, even of the project already chosen.
      projectExplicit = true;
      if (!repositorySource) repositoryNewProject = false;
      updateForm();
    },
    chooseComputer(index) { if (index >= 0 && index < machines.length) setMachineIndex(index); emit(); },
    chooseFolder(index) {
      if (index < 0 || index >= folders.length) return;
      if (index !== folderIndex) { folderIndex = index; folderChanged(); }
      prefillRequest = 0;
      updateForm();
    },
    selectPath: (target, automaticWorktree = true) => { selectPath(target, automaticWorktree); emit(); },
    setSource: setRepositorySource,
    setUrl,
    setDestination,
    setRepositoryParent,
    setNewProjectName(text) { customProjectName = true; newProjectName = text.slice(0, 80); updateForm(); },
    setAuto(on) { autoPreferred = on; updateForm(); },
    chooseAccount,
    selectAccount: (id) => selectAccount(id),
    refreshAccounts,
    setName(text) { name = text; updateForm(); },
    setOpenTerminal(on) { openTerminal = on; updateForm(); },
    ...oc.actions,
    async browseLocal() {
      if (host() !== "") return;
      const target = host(), project = projectId();
      let chosen: string | null = null;
      try { chosen = await b.pickLocalFolder(path()); } catch { chosen = null; }
      if (closed || !chosen) return;
      applyBrowsedFolder(target, project, chosen);
      emit();
    },
    applyBrowsedFolder: (target, project, chosen) => { applyBrowsedFolder(target, project, chosen); emit(); },
    chooseExistingWorktree: (target, project, chosen) => { chooseExistingWorktree(target, project, chosen); emit(); },
    worktreeCreated: (target, project, created, catalog) => worktreeCreated(target, project, created, catalog),
    manageFolders() { leave.folders(projectId()); },
    openMachines() { leave.machines(); },
  };

  return {
    actions, state, start, dispose, fleetChanged, setGroups, selectFolder, prefillSessionFolder, selectAccount, restoreCreationChoices,
    selectPath: (target: string, automaticWorktree?: boolean) => selectPath(target, automaticWorktree),
    setProjectExplicit: (explicit: boolean) => { projectExplicit = explicit; },
    setSwarm: (id: string) => { swarm = id; },
    closed: () => closed,
  };
}

type Dialog = ReturnType<typeof createDialog>;

/** The New session store over its dependencies; the app's instance is below. */
export function createNewSessionStore(deps: NewSessionDeps) {
  const [version, setVersion] = createSignal(0);
  let dialog: Dialog | undefined;
  let opening = false;
  let buildStyle: PathStyle | undefined;
  const admittedListeners = new Set<(id: string, plan: LaunchPlan) => void>();
  const emit = () => setVersion((v) => v + 1);
  const view = createRoot(() => createMemo(() => { version(); return dialog && !dialog.closed() ? dialog.state() : undefined; }));

  function closeNewSession() {
    const open = dialog;
    dialog = undefined;
    open?.dispose();
    emit();
  }

  /** `selectedHostTarget` (3128-3131): the single selected computer filter, "" otherwise or for this computer. */
  function selectedHostTarget(): string {
    const filters = deps.hostFilters();
    return filters.length === 1 ? filters[0] : "";
  }

  /** SessionsWindow::showNewSession (2895-2928). */
  async function openNewSession(options: NewSessionOptions = {}): Promise<void> {
    if (dialog || opening) return;
    opening = true;
    let made: Dialog | undefined;
    try {
      const view = deps.projects();
      const projects = view.projects;
      const fallback = projects.find((p) => p.is_default)?.id ?? INBOX;
      const selection = deps.selection();
      const named = (text: string) => projects.find((p) => p.id === text || p.name === text)?.id;
      // The selected session's project, else the default; a project option matching an id or name wins.
      let launchProject = (selection && named(selection.row.project)) || fallback;
      if (options.project) launchProject = named(options.project) ?? launchProject;
      // Only generic creation inherits the selected session; other entries carry their own target.
      const generic = !options.project && !options.folder && !options.path && options.account == null;
      const context = generic ? selection : undefined;
      const computer = context ? context.row.computer : options.computer ?? selectedHostTarget();
      const started = await deps.bridge.newSessionContext();
      buildStyle = started.build_style;
      const open = createDialog(deps, started, { host: computer, agent: options.agent ?? "", project: launchProject }, emit, {
        folders: (project) => { closeNewSession(); deps.openProjects(project); },
        machines: () => { closeNewSession(); deps.openMachines(); },
      });
      made = open;
      open.setProjectExplicit(!!options.project);
      open.setSwarm(view.swarm_id);
      open.setGroups(projects, launchProject);
      if (context) {
        const details = context.details;
        const folder = typeof details?.cwd === "string" ? details.cwd : context.row.cwdPath;
        const canonical = typeof details?.cwd_canonical === "string" ? details.cwd_canonical : context.row.cwdCanonical;
        open.prefillSessionFolder(folder, canonical);
      }
      if (options.folder && !open.selectFolder(options.folder)) {
        open.dispose();
        deps.notify("error", FOLDER_CHANGED, true);
        return;
      }
      if (options.path) open.selectPath(options.path, false);
      if (options.account != null) open.selectAccount(options.account);
      if (options.recipe) open.restoreCreationChoices(options.recipe);
      dialog = open;
      emit();
    } catch (failure) {
      // A dialog made before the failure ends its timer, listener and watch.
      if (made && made !== dialog) made.dispose();
      deps.notify("error", errorText(failure), true);
    } finally {
      opening = false;
    }
  }

  /** ProjectsDialog::newFolderSession (533-541): "" when the dialog opens, else why it cannot. */
  async function openFolderSession(projectId: string, folder: Folder): Promise<string> {
    const project = deps.projects().projects.find((p) => p.id === projectId);
    const present = !!project?.folders.some((f) => f.id === folder.id && f.machine === folder.machine && f.path === folder.path);
    if (!present) return FOLDER_CHANGED;
    const local = deps.cards()[""];
    const localHost = local?.host ?? "";
    if (!local?.platform && buildStyle === undefined) {
      try { buildStyle = (await deps.bridge.newSessionContext()).build_style; } catch { buildStyle = "posix"; }
    }
    const env = { localHost, online: deps.online, style: (machine: string) => of(local?.platform, !!machine && machine === localHost, buildStyle ?? "posix") };
    let problem = folderLaunchProblem(folder, env, undefined);
    if (!problem && folder.machine === localHost) {
      let exists = false;
      try { exists = await deps.bridge.localFolderExists(folder.path); } catch { exists = false; }
      problem = folderLaunchProblem(folder, env, exists);
    }
    if (problem) return problem;
    await openNewSession({ project: projectId, computer: folder.machine === localHost ? "" : folder.machine, folder: folder.id });
    return "";
  }

  /** Start: the steps of `start` (743-778); the dialog closes only once the creation is admitted (777). */
  async function startNewSession(confirm: (text: string) => Promise<boolean>): Promise<void> {
    const open = dialog;
    if (!open) return;
    const admitted = await open.start(confirm);
    if (!admitted) return;
    if (dialog === open) closeNewSession();
    // An admitted creation reaches its card whatever became of the dialog.
    for (const listener of admittedListeners) listener(admitted.id, admitted.plan);
  }

  /** store/creations.ts takes each admitted creation here and reveals its card. Returns the stop. */
  function onCreationAdmitted(listener: (id: string, plan: LaunchPlan) => void) {
    admittedListeners.add(listener);
    return () => { admittedListeners.delete(listener); };
  }

  // Each control acts on the open dialog, and on nothing once it closed.
  const actions = Object.fromEntries(ACTIONS.map((key) => [key, (...args: unknown[]) => {
    const fn = dialog?.actions[key] as ((...a: unknown[]) => unknown) | undefined;
    return fn?.(...args);
  }])) as unknown as NewSessionActions;

  return {
    newSession: view, actions, openNewSession, openFolderSession, selectedHostTarget, closeNewSession, startNewSession, onCreationAdmitted,
    /** Machine cards or peers changed (the app calls this from its fleet effect). */
    fleetChanged: () => { if (dialog) untrack(() => dialog?.fleetChanged()); },
  };
}

/**
 * What the open dialog reads of the fleet: the computers, whose card is ok
 * and its host name, and this computer's platform, local sessions and session
 * host (`can_enable_local_sessions`, launch.rs 178-185). A change runs setFleet.
 */
export function fleetKey(cards: Record<ComputerId, MachineMeta>, peers: string[], online: (computer: ComputerId) => boolean): string {
  const local = cards[""];
  return JSON.stringify([
    peers, ["", ...peers].map((id) => [online(id), cards[id]?.host ?? null]),
    local?.platform ?? null, local?.local_sessions ?? null, local?.local_host ?? null,
  ]);
}

// The app's store: the catalog's machine cards and computers, the Projects
// view, the selected session and the Sessions computer filter.

const peersOf = () => {
  const ids = new Set<string>([...Object.keys(catalog.computers), ...Object.keys(machineCards)]);
  ids.delete("");
  return [...ids].sort();
};
const onlineOf = (computer: ComputerId) => machineCards[computer]?.ok === true && !computerOffline(computer);

const app = createNewSessionStore({
  bridge,
  cards: () => machineCards,
  peers: peersOf,
  online: onlineOf,
  projects: () => projectsPage(),
  selection: () => {
    const row = selected();
    if (!row) return undefined;
    const snapshot = snapshotOf(row);
    return { row, details: detailsFor(snapshot, onlineOf(row.computer), followedFor(row.key)) };
  },
  hostFilters: () => machineFilter().map((name) => {
    const row = Object.values(catalog.rows).find((s) => s.machine === name);
    return row ? row.computer : name;
  }),
  notify: (tone, text, sticky) => notify(tone, text, sticky),
  openProjects: (project) => { setPage("projects"); requestProject(project); },
  openMachines: () => { setPage("machines"); requestMachine(""); },
  every: (ms, run) => { const timer = setInterval(run, ms); return () => clearInterval(timer); },
  now: () => Date.now() / 1000,
});

// Fleet updates reach the open dialog (SessionsWindow 2692).
createRoot(() => {
  createEffect(on(() => fleetKey(machineCards, peersOf(), onlineOf), () => app.fleetChanged(), { defer: true }));
});

/** The open dialog, one at a time. */
export const newSession: () => NewSessionState | undefined = app.newSession;
export const newSessionActions: NewSessionActions = app.actions;
export const openNewSession = (options?: NewSessionOptions): Promise<void> => app.openNewSession(options);
export const openFolderSession = (project: string, folder: Folder): Promise<string> => app.openFolderSession(project, folder);
export const selectedHostTarget = (): string => app.selectedHostTarget();
/** Cancel, Escape and the close button (`reject`, 476): with no foreground work, closing is always allowed. */
export const closeNewSession = (): void => app.closeNewSession();
export const startNewSession = (confirm: (text: string) => Promise<boolean>): Promise<void> => app.startNewSession(confirm);
export const onCreationAdmitted = (listener: (id: string, plan: LaunchPlan) => void) => app.onCreationAdmitted(listener);
