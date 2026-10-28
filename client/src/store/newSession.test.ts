import { describe, expect, it, vi } from "vitest";
import effortIds from "../../../tests/fixtures/opencode-effort-ids.json";
import { machineMeta, NOW, usageOf } from "./testing";
import { createNewSessionStore, fleetKey, type NewSessionBridge, type NewSessionDeps, type NewSessionOptions } from "./newSession";
import type {
  Account, CreationAdmission, Details, Folder, LaunchAccounts, LaunchForm, LaunchShown, MachineMeta, NewSessionContext, NewSessionFormInput, Profile, Project, ProjectedWindow,
  ProjectsView, RepositoryFields,
} from "../types";
import type { Session } from "../ui/sessions/types";

// tray/tests/test_newsessiondialog.cpp, ported over a recorded bridge: each
// command the store sends is kept, and a fake world answers as the Qt test's
// fake hgs does. Nothing here runs hgs. Synthetic computers and paths only.

vi.mock("./ui", () => ({ selected: () => undefined, machineFilter: () => [], setPage: () => {} }));
vi.mock("../bridge", () => ({}));

type Json = Record<string, unknown>;
interface Call { command: string; args: unknown[] }

/** Commands that create or change something; opening, choosing and reading send none of them. */
const CREATING = ["creationSubmit", "createWorktree", "createDirectory", "openCodeStart", "openCodeFavorite"];

const NOT_LOADED: LaunchAccounts = { loaded: false, error: null, profiles: [], groups: [] };

const settle = async () => { for (let i = 0; i < 4; i++) await new Promise((resolve) => setTimeout(resolve, 0)); };

function deferred() {
  let release!: () => void;
  const done = new Promise<void>((resolve) => { release = resolve; });
  return { done, release };
}

// The fake world: the Qt fixture's accounts, catalogs and OpenCode service.

const window = (used: number, minutes: number, resets: number): ProjectedWindow => ({ id: `w${minutes}`, label: "", used, minutes, resets_at: resets });

function member(host: string, id: string, provider: string, label: string, extra: Partial<Profile> = {}): Profile {
  return {
    key: `${host}:${provider}:${id}`, host, machine: host || "pc", id, label, provider, native: id.startsWith("native-"), installed: true,
    is_default: false, home: `/profiles/${id}`, auth_revision: "", online: true, state: "ready", detail: "", signed_in: true,
    permission: "", permission_label: "", permission_hint: "", model_supported: false, model_text: "", usage: usageOf({ provider }), opencode: null, ...extra,
  };
}

class World {
  flags = new Set<string>();
  favorites: string[] = [];
  calls: Call[] = [];
  gates = new Map<string, ReturnType<typeof deferred>>();
  online: Record<string, boolean> = { "": true, mac: true };
  cards: Record<string, MachineMeta> = { "": machineMeta("pc"), mac: machineMeta("mac") };
  warning: string | null = null;
  /** What `creation_submit` answers; an Error is a lost call (an unknown outcome). */
  admission: CreationAdmission | Error = { admitted: "creation-1" };
  /** Computers whose catalog reply is not filed yet: `launch_accounts` answers `loaded: false`. */
  unfiled = new Set<string>();
  /** Runs inside each `new_session_start`, with its 1-based number. */
  onStart: ((n: number) => void) | undefined;
  localExists = true;
  projects: Project[] = [];
  swarm = "";
  extraModel: Json | undefined;
  selection: { row: Session; details?: Details } | undefined;
  filters: string[] = [];
  listeners = new Set<(computer: string) => void>();
  ticks: (() => void)[] = [];
  notices: { tone: string; text: string; sticky: boolean }[] = [];
  opened: string[] = [];
  admitted: string[] = [];

  hold(name: string) { const gate = deferred(); this.gates.set(name, gate); return gate; }
  release(name: string) { this.gates.get(name)?.release(); this.gates.delete(name); }
  async wait(name: string) { const gate = this.gates.get(name); if (gate) await gate.done; }
  count(command: string) { return this.calls.filter((c) => c.command === command).length; }
  of(command: string) { return this.calls.filter((c) => c.command === command); }
  creating() { return this.calls.filter((c) => CREATING.includes(c.command)); }

  /** OpenCode's model catalog on an account row: its variants are each model's effort options. */
  rowCatalog(host: string): Json {
    const stopped = this.flags.has("opencode-stopped");
    const many = Array.from({ length: 20 }, (_, n) => `level-${n}`);
    const models: Json[] = [
      { id: "fixture/local", value: "fixture/local", label: "Local fixture", provider: "fixture", provider_label: "fixture", free: true, effort_options: ["low", "medium", "high"], effort_labels: ["low", "medium", "high"] },
      { id: "fixture/plain", value: "fixture/plain", label: "Plain fixture", provider: "fixture", provider_label: "fixture", free: true, effort_options: [], effort_labels: [] },
      { id: "fixture/other", value: "fixture/other", label: "Other fixture", provider: "fixture", provider_label: "fixture", free: false, effort_options: ["high", "max", "", "bad value", "x".repeat(80), "max"], effort_labels: ["High", "Maximum", "", "", "", ""] },
      { id: "fixture/many", value: "fixture/many", label: "Many fixture", provider: "fixture", provider_label: "fixture", free: false, effort_options: many, effort_labels: many },
    ];
    if (this.extraModel) models.push(this.extraModel);
    const result: Json = { model_options: models, settings_model: "fixture/local", catalog_state: stopped ? "not_running" : "ready",
      catalog_reason: stopped ? `OpenCode is not running on ${host || "pc"}` : "", catalog_at: 1791018000 };
    if (!this.flags.has("old-hgs")) result.model_options_truncated = false;
    return result;
  }

  usage(host: string, id: string, provider: string) {
    const ended = this.flags.has("ended");
    const windows = this.flags.has("missing") || provider === "dsh" || provider === "opencode" ? []
      : [window(host === "mac" ? 91 : 17, 300, ended ? NOW - 60 : NOW + 3600), window(78, 10080, ended ? NOW - 60 : NOW + 86_400)];
    const email = provider === "opencode" ? "" : this.flags.has("machine-accounts") ? `${host === "mac" ? "mac" : "pc"}@example.invalid` : "shared@example.invalid";
    return usageOf({ provider, status: provider === "dsh" ? "signed_out" : provider === "opencode" ? "configured" : "ok", identity: { ...usageOf().identity, email }, windows });
  }

  accounts(host: string): LaunchAccounts {
    let rows: [string, string, string, Partial<Profile>][] = [["work", "codex", "Work", {}], ["other", "codex", "Work", { is_default: true }], ["claude", "claude", "Review", {}], ["native-dsh", "dsh", "DeepSeek", {}]];
    if (this.flags.has("removed")) rows = rows.filter(([id]) => id !== "work");
    if (this.flags.has("machine-accounts")) rows = (host === "mac" ? ["mac-claude"] : ["pc-claude", "pc-copy"]).map((id) => [id, "claude", "Review", { is_default: true }]);
    if (this.flags.has("opencode")) rows.push(["native-opencode", "opencode", "Native configuration", { is_default: true }]);
    if (this.flags.has("empty-catalog")) rows = [];
    const profiles: Json[] = rows.map(([id, provider, label, extra]) => ({
      id, provider, label, home: `/profiles/${id}`, installed: true, is_default: !!extra.is_default,
      ...(provider === "opencode" && this.flags.has("opencode-models") ? this.rowCatalog(host) : {}),
    }));
    const members = rows.map(([id, provider, label, extra]) => member(host, id, provider, label, { ...extra, usage: this.usage(host, id, provider) }));
    // machine-accounts: this computer's two rows are one account.
    const grouped: Profile[][] = this.flags.has("machine-accounts") && host !== "mac" ? [members] : members.map((m) => [m]);
    const groups: Account[] = grouped.map((list) => ({
      key: `${list[0].provider}:${list[0].id}`, label: list[0].label, provider: list[0].provider, installed: true, machines: [host || "pc"], members: list,
      usage: list[0].usage, permission: "", identity_lines: [], identity_note: "",
    }));
    return { loaded: true, error: this.flags.has("fail") ? "Could not load accounts: exit code 1" : null, profiles, groups };
  }

  folderCatalog(location: string): Json {
    if (this.flags.has("models-stopped")) {
      return { catalog_state: "not_running", catalog_reason: "OpenCode is not running on pc", model_options: [], models: [], providers: [],
        model_options_truncated: false, providers_truncated: false, models_truncated: false, favorites: [] };
    }
    const [tag, name] = location === "/workspace/example" ? ["proj", "Project model"] : ["other", "Other folder model"];
    const short: Json[] = (this.rowCatalog("").model_options as Json[]).filter((m) => location !== "/workspace/nolocal" || m.value !== "fixture/local").map((m) => ({ ...m }));
    short.push({ id: `folderonly/${tag}`, value: `folderonly/${tag}`, label: name, provider: "folderonly", provider_label: "Folder-only fixture", free: false, effort_options: [], effort_labels: [] });
    const full: Json[] = [...short.map((m) => ({ ...m })), { id: "solo/solo-model", value: "solo/solo-model", label: "Solo model", provider: "solo", provider_label: "Solo fixture", free: false, effort_options: ["low"], effort_labels: ["low"] }];
    for (const entry of [...short, ...full]) if (this.favorites.includes(entry.value as string)) entry.favorite = true;
    const held = this.flags.has("hold-next-models") && !this.flags.has("models-held");
    if (held) short.push({ id: "stale/old", value: "stale/old", label: "Stale model", provider: "stale", provider_label: "Stale fixture", free: false, effort_options: [], effort_labels: [] });
    return { catalog_state: "ready", catalog_reason: "", location, model_options: short, models: full,
      providers: [{ id: "folderonly", label: "Folder-only fixture", kind: "config", credential_count: 0, model_count: 1, folder: true }],
      model_options_truncated: false, providers_truncated: false, models_truncated: false, favorites: this.favorites.map((value) => ({ value })) };
  }

  worktrees(path: string): Json {
    if (this.flags.has("prefill-linked")) {
      return { state: "ok", path, selected_root: path, common_dir: "/workspace/example/.git",
        worktrees: [{ path: "/workspace/example", kind: "main", available: true }, { path: "/workspace/example-work-one", kind: "linked", available: true }] };
    }
    return { state: "not_repo", worktrees: [] };
  }
}

/** A minimal `launch::shown` for the fake: Start's enablement, the notice and the preview. */
function shownOf(f: LaunchForm): LaunchShown {
  const repository = f.repository;
  const automatic = f.auto_preferred && (!!repository || !!f.automatic_available);
  const ready = repository
    ? f.online && repository.can_submit && (!repository.new_project || repository.new_project_name.trim() !== "")
    : !f.prefill_pending && f.path !== "" && f.folder_online && (!f.auto_preferred || !f.catalog_pending);
  const nameProblem = f.name.trim() ? null : "Enter a session name.";
  const notice = f.no_machine ? (f.can_enable_local_sessions ? "Enable local sessions for this computer in Machines, or add a remote machine." : "Add a remote machine in Machines to start sessions from this computer.")
    : f.account_preset && !f.account_selected ? (f.account_loading ? "Loading this session's account…" : "This session's account is unavailable. Choose another account.")
      : f.account_count === 0 ? "Restore or add an account in Agent accounts to start this agent."
        : !repository && f.path && !f.folder_online ? "This computer is unavailable. Choose another folder or reconnect it." : null;
  return {
    start_enabled: !f.no_machine && f.account_selected && (!f.account_loading || f.agent === "sh") && !nameProblem && ready,
    start_text: repository ? "Clone and start" : "Start session",
    preview: f.prefill_pending ? "Checking the session's main working folder…" : f.path ? "" : "Browse to choose a folder on this computer.",
    outside_project: false, name_problem: nameProblem, notice, automatic, auto_enabled: false, none_enabled: true, worktree_target: "", worktree_tooltip: "",
    choose_existing_enabled: false, create_manually_enabled: false, worktree_options_enabled: false, fields_enabled: true, browse_enabled: true,
    cancel_enabled: true, open_terminal_enabled: true, open_terminal_tooltip: "", clone_launch_due: false,
  };
}

function bridgeOf(world: World): NewSessionBridge {
  const record = (command: string, ...args: unknown[]) => { world.calls.push({ command, args }); };
  const context = (): NewSessionContext => ({ branch: "work-1a2b3c4d", own_windows_session: null, build_style: "posix", remembered: { agent: "", accounts: {}, open_terminal: false } });
  return {
    newSessionContext: async () => { record("newSessionContext"); return world.context ?? context(); },
    newSessionForm: async (input: NewSessionFormInput) => { record("newSessionForm", input); return { shown: shownOf(input.form), automatic_available: false, automatic_path: "", project_worktree: false }; },
    newSessionStart: async (input: NewSessionFormInput, submitted: boolean) => {
      record("newSessionStart", input, submitted);
      const answer = !submitted && shownOf(input.form).start_enabled ? "background" : "nothing";
      world.onStart?.(world.count("newSessionStart"));
      return answer;
    },
    newSessionPlan: async (input, choices) => { record("newSessionPlan", input, choices); return { cmd: choices.agent, tag: input.form.name } as never; },
    launchWarning: async (local, computer) => { record("launchWarning", local, computer); return world.warning; },
    repositoryFields: async (url, destination, style, custom, fresh = null) => {
      record("repositoryFields", url, destination, style, custom, fresh);
      const suggested = (url.split(/[/:]/).pop() ?? "").replace(/\.git$/, "");
      const field = fresh !== null ? `~/${fresh || suggested}` : custom ? destination : `${destination ? destination.slice(0, destination.lastIndexOf("/")) : "~"}/${suggested}`;
      const at = field.lastIndexOf("/");
      const fields: RepositoryFields = { url, parent: at <= 0 ? "/" : field.slice(0, at), name: field.slice(at + 1), field, destination: field, suggested, can_submit: url !== "" && field.slice(at + 1) !== "" };
      return fields;
    },
    worktreeSnapshot: async (computer, path) => { record("worktreeSnapshot", computer, path); return {}; },
    worktreeCatalog: async (computer, path, refresh) => {
      record("worktreeCatalog", computer, path, refresh);
      await world.wait(refresh ? "prefill" : "catalog");
      return world.worktrees(path);
    },
    pickLocalFolder: async (start) => { record("pickLocalFolder", start); return world.picked ?? null; },
    localFolderExists: async (path) => { record("localFolderExists", path); return world.localExists; },
    openCodeModels: async (computer, folder, name) => {
      record("openCodeModels", computer, folder, name);
      if (world.flags.has("old-hgs")) throw `Update Zerus on ${name} to search all models and star favorites.`;
      const held = world.flags.has("hold-next-models") && !world.flags.has("models-held");
      const answer = world.folderCatalog(folder);
      if (held) { world.flags.add("models-held"); await world.wait("next-models"); }
      if (folder === "/workspace/example") await world.wait("models");
      return answer;
    },
    openCodeFavorite: async (computer, value, favorite, name) => {
      record("openCodeFavorite", computer, value, favorite, name);
      await world.wait("favorite");
      if (world.flags.has("favorite-fails")) throw "Account catalog is busy. Retry after the current operation finishes.";
      world.favorites = [...world.favorites.filter((v) => v !== value), ...(favorite ? [value] : [])];
      return { status: "applied", model: value, favorite };
    },
    openCodeStart: async (computer) => { record("openCodeStart", computer); world.flags.delete("opencode-stopped"); return world.rowCatalog(computer); },
    launchAccounts: async (computer, refresh, agent = null) => {
      record("launchAccounts", computer, refresh, agent);
      // The core answers without rows while the reply is not filed; a held answer was built at the call.
      const answer = world.unfiled.has(computer) ? NOT_LOADED : world.accounts(computer);
      await world.wait(`accounts:${computer}`);
      return answer;
    },
    launchAccountRows: async (computer) => {
      record("launchAccountRows", computer);
      // The answer is built at the call; a held one arrives late.
      const answer = world.unfiled.has(computer) ? NOT_LOADED : world.accounts(computer);
      await world.wait(`rows:${world.count("launchAccountRows")}`);
      return answer;
    },
    launchAccountsClose: async () => { record("launchAccountsClose"); },
    onLaunchAccountsChanged: (listener) => { world.listeners.add(listener); return () => world.listeners.delete(listener); },
    creationSubmit: async (plan) => {
      record("creationSubmit", plan);
      const admission = world.admission;
      await world.wait("submit");
      if (admission instanceof Error) throw admission.message;
      return admission;
    },
  };
}

// Optional parts of the world a test sets.
interface World { context?: NewSessionContext; picked?: string }

function setup(configure: (world: World) => void = () => {}) {
  const world = new World();
  world.projects = [project("ungrouped", "Inbox", [], { is_default: true })];
  configure(world);
  const projectsView = (): ProjectsView => ({ loaded: true, refreshing: false, status: "", status_error: false, conflicts: 0, unconnected: [], notice: null, projects: world.projects, swarm_id: world.swarm });
  const deps: NewSessionDeps = {
    bridge: bridgeOf(world),
    cards: () => world.cards,
    peers: () => ["mac"],
    online: (computer) => world.online[computer] ?? false,
    projects: projectsView,
    selection: () => world.selection,
    hostFilters: () => world.filters,
    notify: (tone, text, sticky = false) => world.notices.push({ tone, text, sticky }),
    openProjects: (id) => world.opened.push(`projects:${id}`),
    openMachines: () => world.opened.push("machines"),
    every: (_ms, run) => { world.ticks.push(run); return () => { world.ticks = world.ticks.filter((t) => t !== run); }; },
    now: () => NOW,
  };
  const store = createNewSessionStore(deps);
  store.onCreationAdmitted((id) => world.admitted.push(id));
  const open = async (options: NewSessionOptions = {}) => { await store.openNewSession(options); await settle(); };
  const state = () => { const s = store.newSession(); if (!s) throw new Error("no dialog"); return s; };
  const act = store.actions;
  const row = (id: string) => state().accounts.findIndex((r) => r.id === id && !r.shell);
  const choose = async (id: string) => { act.chooseAccount(row(id)); await settle(); };
  const fire = async (computer: string) => { for (const listener of world.listeners) listener(computer); await settle(); };
  const tick = async () => { for (const run of world.ticks) run(); await settle(); };
  const start = async (answer = true) => { const asked: string[] = []; await store.startNewSession(async (text) => { asked.push(text); return answer; }); await settle(); return asked; };
  const choices = () => world.of("newSessionPlan").at(-1)?.args[1] as Json;
  const values = () => state().picker.groups.flatMap((g) => g.models.map((m) => m.value));
  const star = (value: string) => state().picker.groups.flatMap((g) => g.models).find((m) => m.value === value);
  return { world, store, open, state, act, row, choose, fire, tick, start, choices, values, star };
}

function project(id: string, name: string, folders: Folder[] = [], extra: Partial<Project> = {}): Project {
  return { id, name, color: null, accessible: true, is_default: false, palette: 0, folders, ...extra };
}
const folder = (id: string, machine: string, path: string): Folder => ({ id, name: path.split("/").pop() ?? path, path, computer: machine, machine });

describe("accounts (NewSessionDialog.cpp 865-1023)", () => {
  // macOS gives the dialog's first focus to the list: while it loads, its only row (Run shell) is no choice.
  it("focusWhileCatalogLoadsKeepsRememberedAgent", async () => {
    const t = setup((w) => { w.context = { branch: "work-1a2b3c4d", own_windows_session: null, build_style: "posix", remembered: { agent: "claude", accounts: {}, open_terminal: false } }; });
    t.world.hold("accounts:mac");
    await t.open({ computer: "mac" });
    expect(t.state().accounts.length).toBe(1);
    expect(t.state().account).toBe(-1);
    t.world.release("accounts:mac");
    await settle();
    expect(t.state().accounts[t.state().account].id).toBe("claude");
  });

  // A remembered agent this computer lacks falls back to the first agent, not the shell.
  it("unavailableRememberedAgentSelectsFirstAgent", async () => {
    const t = setup((w) => { w.flags.add("machine-accounts"); w.context = { branch: "work-1a2b3c4d", own_windows_session: null, build_style: "posix", remembered: { agent: "kimi", accounts: {}, open_terminal: false } }; });
    await t.open();
    expect(t.row("pc-claude")).toBe(0);
    expect(t.state().account).toBe(0);
    expect(t.state().agent).toBe("claude");
  });

  it("rememberedAndExplicitMissingAccounts", async () => {
    const t = setup((w) => { w.context = { branch: "work-1a2b3c4d", own_windows_session: null, build_style: "posix", remembered: { agent: "", accounts: { "": { codex: "gone" } }, open_terminal: false } }; });
    await t.open({ agent: "codex", path: "/workspace/example" });
    expect(t.state().accounts.length).toBe(5);
    expect(t.state().accounts[t.state().account].id).toBe("other");
    t.act.selectAccount("gone");
    await settle();
    expect(t.state().account).toBe(-1);
    expect(t.state().form?.start_enabled).toBe(false);
    expect(t.state().error).toContain("unavailable");
    await t.choose("work");
    t.world.flags.add("removed");
    t.act.refreshAccounts();
    await settle();
    expect(t.row("work")).toBe(-1);
    expect(t.state().account).toBe(-1);
    t.act.chooseAccount(t.state().accounts.length - 1);
    await settle();
    expect(t.state().form?.start_enabled).toBe(true);
    await t.start();
    expect(t.choices()).toMatchObject({ agent: "sh", account: "" });
  });

  it("refreshKeepsSelectionAndLastReportedLimits", async () => {
    const t = setup();
    await t.open({ agent: "codex" });
    expect(t.state().accounts.length).toBe(5);
    await t.choose("work");
    expect(t.state().accounts[t.row("work")].tooltip).toContain("17% used");
    t.world.flags.add("fail");
    t.act.refreshAccounts();
    await settle();
    expect(t.world.of("launchAccounts").at(-1)?.args).toEqual(["", true, "codex"]);
    expect(t.state().accountStatus).toContain("Could not load accounts");
    expect(t.state().accounts[t.state().account].id).toBe("work");
    expect(t.state().accounts[t.state().account].tooltip).toContain("17% used");
    expect(t.state().accounts.length).toBe(5);
    // The minute timer refreshes rows without reading the catalog again.
    const reads = t.world.count("launchAccounts");
    await t.tick();
    expect(t.world.count("launchAccounts")).toBe(reads);
    expect(t.world.count("launchAccountRows")).toBe(1);
  });

  it("lateUsageCannotRetargetAnotherComputer", async () => {
    const t = setup();
    t.world.hold("accounts:mac");
    await t.open({ agent: "codex", computer: "mac" });
    t.act.chooseComputer(t.state().computers.findIndex((c) => c.target === ""));
    await settle();
    expect(t.state().accounts.length).toBe(5);
    await t.choose("work");
    t.world.release("accounts:mac");
    await t.fire("mac");
    expect(t.state().accounts[t.state().account].tooltip).toContain("17% used");
    expect(t.state().accounts[t.state().account].tooltip).not.toContain("91% used");
    expect(t.world.count("launchAccountRows")).toBe(0);
  });

  it("selectedComputerCatalogRejectsLateOtherComputerRows", async () => {
    const t = setup((w) => w.flags.add("machine-accounts"));
    t.world.hold("accounts:mac");
    await t.open({ agent: "claude", computer: "mac" });
    expect(t.state().accounts.length).toBe(1);
    expect(t.state().account).toBe(-1);
    t.act.chooseComputer(t.state().computers.findIndex((c) => c.target === ""));
    await settle();
    expect(t.row("pc-claude")).toBe(0);
    expect(t.state().accounts.length).toBe(2);
    expect(t.state().accounts[0].name).toBe("pc@example.invalid");
    t.world.release("accounts:mac");
    await settle();
    expect(t.row("mac-claude")).toBe(-1);
    expect(t.state().accounts.length).toBe(2);
    t.act.chooseComputer(t.state().computers.findIndex((c) => c.target === "mac"));
    expect(t.row("pc-claude")).toBe(-1);
    await settle();
    expect(t.row("mac-claude")).toBe(0);
    expect(t.state().accounts[0].name).toBe("mac@example.invalid");
  });

  it("pendingAccountsAllowShellOnlyWhenChosen", async () => {
    const t = setup();
    t.world.hold("accounts:mac");
    await t.open({ agent: "claude", computer: "mac", path: "/workspace/example" });
    expect(t.state().account).toBe(-1);
    expect(t.state().form?.start_enabled).toBe(false);
    t.act.chooseAccount(0);
    await settle();
    expect(t.state().form?.start_enabled).toBe(true);
    await t.start();
    expect(t.choices()).toMatchObject({ host: "mac", agent: "sh" });
    t.world.release("accounts:mac");
  });

  for (const host of ["", "mac"]) {
    it(`pairsAndLimitsLaunchExactProfile (${host || "this computer"})`, async () => {
      const t = setup();
      await t.open({ agent: "codex", computer: host, path: "/workspace/example" });
      const rows = t.state().accounts;
      expect(rows.length).toBe(5);
      const work = rows[t.row("work")];
      expect(work.tooltip).toContain(host ? "91% used" : "17% used");
      expect(work.tooltip).toContain("5h");
      expect(work.tooltip).toContain("7d");
      expect(work.tooltip).not.toContain("Reset in");
      expect(work.name).toBe("shared@example.invalid");
      expect(rows[t.row("other")].name).toBe("shared@example.invalid");
      expect(work.tooltip.split("\n").length).toBe(2);
      expect(rows[t.state().account].id).toBe("other");
      expect(rows[t.row("native-dsh")].tooltip).toBe("shared@example.invalid\nlimits n/a");
      expect(rows[rows.length - 1].name).toBe("Run shell");
      await t.choose("work");
      await t.start();
      expect(t.choices()).toMatchObject({ host, agent: "codex", account: "work", machine: host || "pc" });
    });
  }

  // Opening asks for limits; their replies change rows in place: rows, choice and order stay.
  it("openingRefreshesLimitsInPlace", async () => {
    const t = setup((w) => w.flags.add("missing"));
    await t.open({ agent: "claude" });
    expect(t.world.of("launchAccounts").map((c) => c.args)).toEqual([["", false, "claude"]]);
    expect(t.state().accounts[t.row("work")].tooltip).toBe("shared@example.invalid\nlimits n/a");
    await t.choose("claude");
    const keys = t.state().accounts.map((r) => r.key);
    t.world.flags.delete("missing");
    await t.fire("");
    expect(t.state().accounts.map((r) => r.key)).toEqual(keys);
    expect(t.state().accounts[t.state().account].id).toBe("claude");
    for (const id of ["work", "claude"]) expect(t.state().accounts[t.row(id)].tooltip).toContain("17% used");
    // Another computer's limits change nothing here; the minute timer only repaints.
    await t.fire("mac");
    await t.tick();
    expect(t.world.count("launchAccounts")).toBe(1);
    expect(t.world.of("launchAccountRows").map((c) => c.args)).toEqual([[""], [""]]);
    // Refresh asks again at once, with forced limits.
    t.act.refreshAccounts();
    await settle();
    expect(t.world.of("launchAccounts").at(-1)?.args).toEqual(["", true, "claude"]);
  });

  it("unknownAndEndedWindowsStayUnavailable", async () => {
    const t = setup((w) => w.flags.add("missing"));
    await t.open({ agent: "codex" });
    expect(t.state().accounts[t.row("work")].tooltip).toBe("shared@example.invalid\nlimits n/a");
    t.world.flags.delete("missing");
    t.world.flags.add("ended");
    t.act.refreshAccounts();
    await settle();
    expect(t.state().accounts[t.row("work")].tooltip).toBe("shared@example.invalid\nlimits n/a");
  });

  // 944-950: while the computer's catalog is not filed, the list keeps loading.
  it("keeps loading while the core answers before the catalog is filed", async () => {
    const t = setup((w) => w.unfiled.add("mac"));
    await t.open({ agent: "claude", computer: "mac", path: "/workspace/example" });
    expect(t.world.of("launchAccounts").map((c) => c.args)).toEqual([["mac", false, "claude"]]);
    expect(t.state()).toMatchObject({ account: -1, accountStatus: "Loading accounts…", refreshEnabled: false });
    expect(t.state().accounts.map((r) => r.shell)).toEqual([true]);
    expect(t.state().form?.start_enabled).toBe(false);
    // The reply is filed: its event brings the catalog, as Qt's accountsReady.
    t.world.unfiled.delete("mac");
    await t.fire("mac");
    expect(t.state().accounts[t.state().account].id).toBe("claude");
    expect(t.state()).toMatchObject({ accountStatus: "", refreshEnabled: true });
    expect(t.state().form?.start_enabled).toBe(true);
    expect(t.world.count("launchAccounts")).toBe(1);
  });

  it("takes a catalog filed before the core's late answer arrived", async () => {
    const t = setup((w) => w.unfiled.add(""));
    t.world.hold("accounts:");
    await t.open({ agent: "codex" });
    // The reply is filed, and its event sent, while the core's empty answer is on its way.
    t.world.unfiled.delete("");
    await t.fire("");
    expect(t.state().account).toBe(-1);
    t.world.release("accounts:");
    await settle();
    expect(t.state().accounts[t.state().account].id).toBe("other");
    expect(t.state().accountStatus).toBe("");
    expect(t.world.count("launchAccounts")).toBe(1);
  });

  it("drops a limits answer older than the newest", async () => {
    const t = setup((w) => w.flags.add("missing"));
    await t.open({ agent: "codex" });
    t.world.hold("rows:1");
    await t.fire("");
    t.world.flags.delete("missing");
    await t.fire("");
    expect(t.state().accounts[t.row("work")].tooltip).toContain("17% used");
    t.world.release("rows:1");
    await settle();
    expect(t.state().accounts[t.row("work")].tooltip).toContain("17% used");
  });

  it("closing ends the dialog's account watch", async () => {
    const t = setup();
    await t.open();
    t.store.closeNewSession();
    await settle();
    expect(t.store.newSession()).toBeUndefined();
    expect(t.world.count("launchAccountsClose")).toBe(1);
    expect(t.world.listeners.size).toBe(0);
    expect(t.world.ticks.length).toBe(0);
  });
});

describe("repository source (NewSessionDialog.cpp 590-639)", () => {
  for (const host of ["", "mac"]) {
    for (const override of ["", "existing", "ungrouped"]) {
      it(`repositorySourceKeepsNewProjectIntent (${host || "local"}, ${override || "keep new"})`, async () => {
        const t = setup((w) => { w.projects = [project("ungrouped", "Inbox", [], { is_default: true }), project("existing", "Existing")]; });
        await t.open({ agent: "codex", project: "existing", computer: host });
        t.act.setSource(true);
        t.act.setUrl("git@example.invalid:team/fresh.git");
        await settle();
        t.act.chooseProject(t.state().projects.length - 1);
        await settle();
        expect(t.state().newProjectVisible).toBe(true);
        t.act.setNewProjectName("My new project");
        t.act.setSource(false);
        await settle();
        expect(t.state().newProjectVisible).toBe(false);
        expect(t.state().projects[t.state().project].id).not.toBe("");
        // A repeated source click must not erase the remembered repository choice.
        t.act.setSource(false);
        if (override) t.act.chooseProject(t.state().projects.findIndex((p) => p.id === override));
        t.act.setSource(true);
        await settle();
        expect(t.state().newProjectVisible).toBe(!override);
        expect(t.state().newProjectName).toBe("My new project");
        expect(t.state().projects[t.state().project].id).toBe(override);
        expect(t.world.creating()).toEqual([]);
      });
    }
  }

  it("repositorySourceKeepsOpaqueProjectId", async () => {
    const id = "new-repository-project";
    const t = setup((w) => { w.projects = [project(id, "Existing project", [], { is_default: true })]; });
    await t.open({ agent: "codex", project: id });
    t.act.setSource(true);
    t.act.setUrl("git@example.invalid:team/fresh.git");
    t.act.setDestination("/workspace/fresh");
    await settle();
    expect(t.state().newProjectVisible).toBe(false);
    expect(t.state().form?.start_enabled).toBe(true);
    await t.start();
    expect(t.choices()).toMatchObject({ project: id, repository: { url: "git@example.invalid:team/fresh.git", destination: "/workspace/fresh", new_project: false } });
  });

  // restoreCreationChoices 792-799: the recipe's project name comes last, after its destination.
  it("Edit choices… keeps a repository's own new project name", async () => {
    const t = setup((w) => { w.projects = [project("ungrouped", "Inbox", [], { is_default: true }), project("existing", "Existing")]; });
    const recipe = { id: "failed-1", tag: "retry", account: "work", repository: true, url: "git@example.invalid:team/fresh.git",
      target: "/workspace/fresh", new_project: true, project_name: "Team project" };
    await t.open({ agent: "codex", recipe });
    expect(t.state()).toMatchObject({ repository: true, destination: "/workspace/fresh", newProjectVisible: true, newProjectName: "Team project" });
    await t.start();
    expect(t.choices()).toMatchObject({ predecessor: "failed-1", repository: { destination: "/workspace/fresh", name: "fresh", new_project: true, project_name: "Team project" } });
  });

  it("a later edit of the restored destination names the project again", async () => {
    const t = setup();
    await t.open({ agent: "codex", recipe: { repository: true, url: "git@example.invalid:team/fresh.git", target: "/workspace/fresh", new_project: true, project_name: "Team project" } });
    t.act.setDestination("/workspace/renamed");
    await settle();
    expect(t.state().newProjectName).toBe("renamed");
  });

  it("the URL names the new project until its name is edited, at most 80 characters", async () => {
    const t = setup();
    await t.open({ agent: "codex" });
    t.act.setSource(true);
    await settle();
    expect(t.world.of("repositoryFields")[0].args).toEqual(["", "", "posix", false, null]);
    expect(t.state().focusUrl).toBe(1);
    t.act.setUrl("https://github.com/example/first.git");
    await settle();
    expect(t.state().destination).toBe("~/first");
    expect(t.state().newProjectName).toBe("first");
    t.act.setDestination("/workspace/custom");
    t.act.setUrl("git@example.invalid:team/third.git");
    await settle();
    expect(t.state().destination).toBe("/workspace/custom");
    t.act.setNewProjectName("x".repeat(90));
    t.act.setDestination("/workspace/renamed");
    await settle();
    expect(t.state().newProjectName).toBe("x".repeat(80));
    // Another computer starts fresh fields with the URL and the old name.
    t.act.chooseComputer(t.state().computers.findIndex((c) => c.target === "mac"));
    await settle();
    expect(t.world.of("repositoryFields").at(-1)?.args).toEqual(["git@example.invalid:team/third.git", "", "posix", false, "renamed"]);
    expect(t.state().destination).toBe("~/renamed");
  });

  it("repositorySourceIgnoresLateFolderPrefill", async () => {
    const t = setup((w) => {
      w.flags.add("prefill-linked");
      w.projects = [project("example", "Example", [folder("f1", "pc", "/workspace/example")], { is_default: true })];
      w.selection = { row: { computer: "", cwdPath: "/workspace/example-work-one", cwdCanonical: "", project: "Example" } as Session };
    });
    t.world.hold("prefill");
    await t.open({ agent: "codex" });
    expect(t.state().form?.preview).toBe("Checking the session's main working folder…");
    t.act.setSource(true);
    t.act.setUrl("git@example.invalid:team/fresh.git");
    t.act.setDestination("/workspace/keep-name");
    t.world.release("prefill");
    await settle();
    const linked = t.world.of("worktreeCatalog").filter((c) => c.args[1] === "/workspace/example-work-one");
    expect(linked.map((c) => c.args[2])).toEqual([false, true]);
    expect(t.state().folderPath).toBe("/workspace/example-work-one");
    expect(t.state().destination).toBe("/workspace/keep-name");
    t.act.setSource(false);
    await settle();
    expect(t.state().folderPath).toBe("/workspace/example-work-one");
    expect(t.world.creating()).toEqual([]);
  });

  it("sourceSwitchKeepsControls", async () => {
    const t = setup((w) => { w.projects = [project("example", "Example", [folder("f1", "pc", "/workspace/example")], { is_default: true })]; });
    await t.open({ agent: "codex", project: "example" });
    t.act.setAuto(false);
    t.act.setName("keep-this-session");
    for (let i = 0; i < 4; i++) { t.act.setSource(!t.state().repository); await settle(); }
    expect(t.state()).toMatchObject({ repository: false, autoPreferred: false, name: "keep-this-session" });
    expect(t.state().projects[t.state().project].id).toBe("example");
    expect(t.state().folderPath).toBe("/workspace/example");
    expect(t.world.creating()).toEqual([]);
  });
});

describe("placement", () => {
  it("untouchedInboxUsesNativeFolderInferenceAndExplicitInboxWins", async () => {
    for (const explicit of [0, 1, 2]) {
      const t = setup((w) => { w.swarm = "swarm-fixture"; });
      await t.open({ agent: "codex", path: "/workspace/example", ...(explicit === 1 ? { project: "ungrouped" } : {}) });
      if (explicit === 2) t.act.chooseProject(t.state().project);
      await settle();
      await t.start();
      expect(t.choices()).toMatchObject({ project: "ungrouped", project_name: "Inbox", implicit_inbox: explicit === 0, swarm: "swarm-fixture", listed_folder: false });
    }
  });
});

describe("OpenCode (NewSessionDialog.cpp 1145-1313)", () => {
  const opencode = (w: World) => { w.flags.add("opencode"); w.flags.add("opencode-models"); };
  const efforts = (t: ReturnType<typeof setup>) => t.state().efforts.map((e) => e.label);

  it("openCodeEffortFollowsSelectedModel", async () => {
    const t = setup(opencode);
    await t.open({ agent: "opencode", path: "/workspace/example" });
    expect(t.state().models.some((m) => m.value === "fixture/local")).toBe(true);
    expect(t.state().accounts[t.state().account].id).toBe("native-opencode");
    expect(t.state()).toMatchObject({ modelVisible: true, model: "", effortVisible: false });
    t.act.chooseModel("fixture/local");
    expect(t.state().effortVisible).toBe(true);
    expect(efforts(t)).toEqual(["Default", "low", "medium", "high"]);
    expect(t.state().effort).toBe("");
    t.act.chooseEffort("high");
    t.act.chooseModel("fixture/other");
    expect(efforts(t)).toEqual(["Default", "High", "Maximum"]);
    expect(t.state().effort).toBe("high");
    t.act.chooseEffort("max");
    t.act.chooseModel("fixture/local");
    expect(t.state().effort).toBe("");
    t.act.chooseModel("fixture/many");
    expect(t.state().efforts.length).toBe(17);
    expect(t.state().efforts[16].value).toBe("level-15");
    t.act.chooseEffort("level-3");
    t.act.chooseModel("fixture/plain");
    expect(t.state()).toMatchObject({ effortVisible: false, effort: "" });
    t.act.chooseModel("fixture/many");
    expect(t.state().effort).toBe("");
    await t.choose("work");
    expect(t.state()).toMatchObject({ effortVisible: false, modelVisible: false });
    await t.choose("native-opencode");
    t.act.chooseModel("fixture/local");
    t.act.chooseEffort("high");
    await settle();
    await t.start();
    expect(t.choices()).toMatchObject({ agent: "opencode", account: "native-opencode", model: "fixture/local", effort: "high" });
  });

  it("openCodeEffortIdsFollowSharedRule", async () => {
    const t = setup((w) => {
      opencode(w);
      const ids = [...effortIds.reject, ...effortIds.accept];
      w.extraModel = { id: "fixture/ids", value: "fixture/ids", label: "Ids fixture", provider: "fixture", effort_options: ids, effort_labels: ids };
    });
    await t.open({ agent: "opencode", path: "/workspace/example" });
    t.act.chooseModel("fixture/ids");
    expect(t.state().efforts.map((e) => e.value)).toEqual(["", ...effortIds.accept]);
    t.act.chooseEffort("think:deep");
    await settle();
    await t.start();
    expect(t.choices().effort).toBe("think:deep");
  });

  it("openCodeStartsStoppedServiceExplicitly", async () => {
    const t = setup((w) => { opencode(w); w.flags.add("opencode-stopped"); });
    await t.open({ agent: "opencode", computer: "mac", path: "/workspace/example" });
    expect(t.state().service).toMatchObject({ visible: true, offer: true, text: "Start OpenCode on mac", enabled: true, reason: "OpenCode is not running on mac" });
    expect(t.world.count("openCodeStart")).toBe(0);
    t.act.startOpenCode();
    await settle();
    expect(t.world.of("openCodeStart").map((c) => c.args)).toEqual([["mac"]]);
    expect(t.world.of("launchAccounts").at(-1)?.args).toEqual(["mac", true, "opencode"]);
    expect(t.state().service).toMatchObject({ visible: false, offer: false });
    // Other agents never offer it.
    t.world.flags.add("opencode-stopped");
    t.act.refreshAccounts();
    await settle();
    expect(t.state().service.offer).toBe(true);
    await t.choose("work");
    expect(t.state().service).toMatchObject({ visible: false, offer: false });
  });

  it("openCodeStartNeedsOnlineComputer", async () => {
    const t = setup((w) => { opencode(w); w.flags.add("opencode-stopped"); w.online.mac = false; });
    await t.open({ agent: "opencode", computer: "mac", path: "/workspace/example" });
    expect(t.state().service).toMatchObject({ offer: true, enabled: false, tooltip: "mac is offline." });
    t.act.startOpenCode();
    await settle();
    expect(t.world.count("openCodeStart")).toBe(0);
    t.world.online.mac = true;
    t.store.fleetChanged();
    await settle();
    expect(t.state().service.enabled).toBe(true);
  });

  it("openCodePickerUsesTheSelectedFolderCatalog", async () => {
    const t = setup(opencode);
    await t.open({ agent: "opencode", computer: "mac", path: "/workspace/example" });
    expect(t.state()).toMatchObject({ modelEnabled: true, modelButton: "Native default model" });
    t.act.openModelPicker();
    await settle();
    expect(t.state().pickerOpen).toBe(true);
    expect(t.values()).toContain("folderonly/proj");
    expect(t.world.of("openCodeModels").map((c) => c.args)).toEqual([["mac", "/workspace/example", "mac"]]);
    expect(t.state().picker.groups.map((g) => g.label)).toContain("Folder-only fixture (this folder)");
    t.act.pickerSelect("folderonly/proj");
    expect(t.state()).toMatchObject({ pickerOpen: false, modelButton: "Project model", effortVisible: false });
    await settle();
    await t.start();
    expect(t.choices()).toMatchObject({ model: "folderonly/proj", effort: "" });
  });

  it("openCodeChoicesOutsideTheRowSurviveAnAccountRefresh", async () => {
    const t = setup(opencode);
    await t.open({ agent: "opencode", path: "/workspace/example" });
    t.act.openModelPicker();
    await settle();
    t.act.pickerSelect("folderonly/proj");
    expect(t.state().modelButton).toBe("Project model");
    const before = t.world.count("openCodeModels");
    t.act.refreshAccounts();
    await settle();
    expect(t.state()).toMatchObject({ modelButton: "Project model", model: "folderonly/proj" });
    t.act.openModelPicker();
    await settle();
    expect(t.values()).toContain("folderonly/proj");
    expect(t.world.count("openCodeModels")).toBe(before);
    // An All models choice is kept the same way, with its efforts.
    t.act.pickerToggleAll();
    t.act.pickerSelect("solo/solo-model");
    expect(t.state().effortVisible).toBe(true);
    t.act.chooseEffort("low");
    t.act.refreshAccounts();
    await settle();
    expect(t.state()).toMatchObject({ modelButton: "Solo model", effort: "low" });
    await t.start();
    expect(t.choices()).toMatchObject({ model: "solo/solo-model", effort: "low" });
  });

  it("openCodeStoppedServiceKeepsTheRowListAndNeverStarts", async () => {
    const t = setup((w) => { opencode(w); w.flags.add("models-stopped"); });
    await t.open({ agent: "opencode", path: "/workspace/example" });
    t.act.openModelPicker();
    await settle();
    expect(t.state().picker.notice).toBe("OpenCode is not running on pc");
    expect(t.values()).toContain("fixture/local");
    // Opening again shows that answer instead of asking again.
    t.act.closeModelPicker();
    t.act.openModelPicker();
    await settle();
    expect(t.state().pickerOpen).toBe(true);
    expect(t.world.count("openCodeModels")).toBe(1);
    expect(t.world.count("openCodeStart")).toBe(0);
  });

  it("openCodeOlderComputerOffersNoStarsOrAllModels", async () => {
    const t = setup((w) => { opencode(w); w.flags.add("old-hgs"); });
    await t.open({ agent: "opencode", computer: "mac", path: "/workspace/example" });
    t.act.openModelPicker();
    await settle();
    expect(t.values()).toContain("fixture/local");
    expect(t.world.count("openCodeModels")).toBe(0);
    expect(t.state().picker.all).toMatchObject({ enabled: false, tooltip: "Update Zerus on mac to search all models and star favorites." });
    t.act.pickerClick("fixture/plain", true);
    expect(t.star("fixture/plain")?.starTooltip).toBe("Update Zerus on mac to search all models and star favorites.");
    t.act.pickerSelect("fixture/local");
    await settle();
    expect(t.state().form?.start_enabled).toBe(true);
    expect(t.world.count("openCodeFavorite")).toBe(0);
  });

  it("openCodeStarSendsOneFavoriteRequestWithIdentity", async () => {
    const t = setup(opencode);
    t.world.hold("models");
    await t.open({ agent: "opencode", path: "/workspace/example" });
    t.act.openModelPicker();
    await settle();
    t.world.release("models");
    await settle();
    t.world.hold("models");
    t.act.pickerClick("fixture/plain", true);
    await settle();
    expect(t.world.of("openCodeFavorite").map((c) => c.args)).toEqual([["", "fixture/plain", true, "pc"]]);
    // The folder catalog is read again, and it marks the favorite.
    expect(t.world.count("openCodeModels")).toBe(2);
    t.world.release("models");
    await settle();
    expect(t.star("fixture/plain")?.favorite).toBe(true);
    t.world.flags.add("favorite-fails");
    t.act.pickerClick("fixture/other", true);
    await settle();
    expect(t.state().error.startsWith("Could not change the favorite on pc:")).toBe(true);
    expect(t.star("fixture/other")).toMatchObject({ favorite: false, pending: false });
    // A failure is never sent again on its own.
    await settle();
    expect(t.world.count("openCodeFavorite")).toBe(2);
  });

  it("openCodeLateFolderCatalogStaysWithItsFolder", async () => {
    const t = setup(opencode);
    t.world.hold("models");
    await t.open({ agent: "opencode", path: "/workspace/example" });
    t.act.openModelPicker();
    await settle();
    expect(t.values()).toContain("fixture/local");
    expect(t.world.count("openCodeModels")).toBe(1);
    t.act.selectPath("/workspace/other", false);
    expect(t.state().pickerOpen).toBe(false);
    expect(t.values()).toContain("fixture/local");
    expect(t.values()).not.toContain("folderonly/proj");
    expect(t.state().picker.selected).toBeUndefined();
    expect(t.state().modelButton).toBe("Native default model");
    t.world.release("models");
    await settle();
    expect(t.values()).not.toContain("folderonly/proj");
    t.act.openModelPicker();
    await settle();
    expect(t.values()).toContain("folderonly/other");
    expect(t.values()).not.toContain("folderonly/proj");
    expect(t.world.count("openCodeModels")).toBe(2);
    // Back in the first folder, its late catalog is shown without another request.
    t.act.closeModelPicker();
    t.act.selectPath("/workspace/example", false);
    expect(t.values()).toContain("folderonly/proj");
    expect(t.values()).not.toContain("folderonly/other");
    expect(t.world.count("openCodeModels")).toBe(2);
  });

  it("openCodeLateFavoriteAnswerStaysWithItsComputer", async () => {
    const t = setup(opencode);
    await t.open({ agent: "opencode", path: "/workspace/example" });
    t.act.openModelPicker();
    await settle();
    t.act.pickerSelect("folderonly/proj");
    expect(t.state().modelButton).toBe("Project model");
    t.act.openModelPicker();
    t.world.hold("favorite");
    t.act.pickerClick("fixture/plain", true);
    expect(t.star("fixture/plain")?.pending).toBe(true);
    await settle();
    t.act.chooseComputer(t.state().computers.findIndex((c) => c.target === "mac"));
    expect(t.state()).toMatchObject({ pickerOpen: false, modelButton: "Native default model" });
    expect(t.state().picker.selected).toBeUndefined();
    expect(t.values()).not.toContain("folderonly/proj");
    await settle();
    expect(t.state().modelEnabled).toBe(true);
    t.act.openModelPicker();
    await settle();
    expect(t.star("fixture/plain")).toMatchObject({ favorite: false, pending: false });
    t.world.release("favorite");
    await settle();
    expect(t.star("fixture/plain")?.favorite).toBe(false);
    expect(t.state().error.startsWith("Could not change the favorite")).toBe(false);
    expect(t.world.count("openCodeFavorite")).toBe(1);
  });

  it("openCodeCurrentModelSurvivesItsFavoriteChanges", async () => {
    const t = setup(opencode);
    await t.open({ agent: "opencode", path: "/workspace/example" });
    t.act.openModelPicker();
    await settle();
    t.act.pickerToggleAll();
    t.act.pickerSelect("solo/solo-model");
    expect(t.state().modelButton).toBe("Solo model");
    t.act.openModelPicker();
    for (const on of [true, false]) {
      const calls = t.world.count("openCodeModels");
      t.act.pickerClick("solo/solo-model", true);
      await settle();
      expect(t.world.count("openCodeModels")).toBe(calls + 1);
      expect(t.star("solo/solo-model")?.favorite).toBe(on);
    }
    expect(t.state().picker.selected).toBe("solo/solo-model");
    expect(t.state()).toMatchObject({ modelButton: "Solo model", model: "solo/solo-model" });
    expect(t.world.count("openCodeFavorite")).toBe(2);
    await t.start();
    expect(t.choices().model).toBe("solo/solo-model");
  });

  it("openCodeOlderFolderAnswerNeverReplacesANewerOne", async () => {
    const t = setup(opencode);
    await t.open({ agent: "opencode", path: "/workspace/example" });
    t.act.openModelPicker();
    await settle();
    t.act.pickerSelect("folderonly/proj");
    t.act.openModelPicker();
    // The star's folder read is built at once, with the favorite on, and held.
    t.world.flags.add("hold-next-models");
    t.world.hold("next-models");
    t.act.pickerClick("fixture/plain", true);
    await settle();
    expect(t.world.flags.has("models-held")).toBe(true);
    expect(t.star("fixture/plain")?.favorite).toBe(true);
    // The unstar's newer read answers first and is applied.
    t.act.pickerClick("fixture/plain", true);
    await settle();
    expect(t.world.count("openCodeFavorite")).toBe(2);
    expect(t.world.count("openCodeModels")).toBe(3);
    expect(t.star("fixture/plain")?.favorite).toBe(false);
    expect(t.values()).not.toContain("stale/old");
    // The older answer arrives last and is dropped.
    t.world.release("next-models");
    await settle();
    expect(t.star("fixture/plain")?.favorite).toBe(false);
    expect(t.values()).not.toContain("stale/old");
    expect(t.values()).toContain("folderonly/proj");
    expect(t.state()).toMatchObject({ modelButton: "Project model", model: "folderonly/proj" });
    // The folder's stored catalog is the newer one too.
    t.act.closeModelPicker();
    t.act.selectPath("/workspace/other", false);
    t.act.selectPath("/workspace/example", false);
    expect(t.values()).toContain("folderonly/proj");
    expect(t.values()).not.toContain("stale/old");
    expect(t.star("fixture/plain")?.favorite).toBe(false);
    expect(t.world.count("openCodeModels")).toBe(3);
  });

  it("openCodeFolderChangeKeepsOnlyAChoiceTheNewFolderOffers", async () => {
    const t = setup(opencode);
    await t.open({ agent: "opencode", path: "/workspace/other" });
    const visit = async (path: string) => { t.act.selectPath(path, false); t.act.openModelPicker(); await settle(); };
    // Read two folders first: one offers the row's fixture/local, one does not.
    t.act.openModelPicker();
    await settle();
    expect(t.values()).toContain("folderonly/other");
    expect(t.values()).toContain("fixture/local");
    t.act.closeModelPicker();
    await visit("/workspace/nolocal");
    expect(t.values()).not.toContain("fixture/local");
    t.act.closeModelPicker();
    // Uncached: a folder without a ready catalog does not take the choice.
    await visit("/workspace/example");
    t.act.pickerSelect("fixture/local");
    expect(t.state().modelButton).toBe("Local fixture");
    t.act.selectPath("/workspace/fresh", false);
    expect(t.state()).toMatchObject({ modelButton: "Native default model", model: "" });
    t.act.selectPath("/workspace/example", false);
    expect(t.state().modelButton).toBe("Native default model");
    // Not offering: a ready catalog without the model does not take it either.
    t.act.openModelPicker();
    t.act.pickerSelect("fixture/local");
    t.act.selectPath("/workspace/nolocal", false);
    expect(t.state()).toMatchObject({ modelButton: "Native default model", model: "" });
    // Offering: a ready catalog that lists it keeps the choice, and the session starts with it.
    t.act.selectPath("/workspace/example", false);
    t.act.openModelPicker();
    t.act.pickerSelect("fixture/local");
    t.act.selectPath("/workspace/other", false);
    expect(t.state()).toMatchObject({ modelButton: "Local fixture", model: "fixture/local" });
    expect(t.state().picker.selected).toBe("fixture/local");
    expect(t.world.count("openCodeModels")).toBe(3);
    await settle();
    await t.start();
    expect(t.choices()).toMatchObject({ path: "/workspace/other", model: "fixture/local" });
  });
});

describe("Start (NewSessionDialog.cpp 743-786)", () => {
  const windowsCard = machineMeta("pc", {}, { platform: "windows", local_host: { state: "running", pid: 4242, protocol: 1, conpty: "bundled", integrity: "medium", windows_session: 2, error: null } });

  it("launchIntoAnotherWindowsSessionAsksFirst", async () => {
    const t = setup((w) => { w.cards[""] = windowsCard; w.warning = "The session host on this computer runs in another Windows session (2), for example one started over SSH. Launch anyway?"; });
    await t.open({ agent: "codex", path: "C:/work/example" });
    await t.choose("work");
    t.act.setName("keep-this-name");
    await settle();
    expect(t.state().form?.start_enabled).toBe(true);
    // Cancel keeps the form as it was: nothing is checked, created or started.
    const asked = await t.start(false);
    expect(asked.length).toBe(1);
    expect(asked[0]).toContain("another Windows session (2)");
    expect(t.world.of("launchWarning")[0].args).toEqual([windowsCard, ""]);
    expect(t.store.newSession()).toBeDefined();
    expect(t.world.count("newSessionPlan")).toBe(0);
    expect(t.world.creating()).toEqual([]);
    expect(t.state()).toMatchObject({ name: "keep-this-name", submitted: false });
    expect(t.state().accounts[t.state().account].id).toBe("work");
    expect(t.state().folderPath).toBe("C:\\work\\example");
    // Launch anyway submits exactly once, after a single question.
    const again = await t.start(true);
    expect(again.length).toBe(1);
    expect(t.world.count("creationSubmit")).toBe(1);
    expect(t.choices()).toMatchObject({ host: "", agent: "codex", path: "C:/work/example", name: "keep-this-name", account: "work" });
    expect(t.store.newSession()).toBeUndefined();
    expect(t.world.admitted).toEqual(["creation-1"]);
  });

  it("asks Start twice before it submits, and sends no creating command before", async () => {
    const t = setup();
    await t.open({ agent: "codex", path: "/workspace/example" });
    t.act.setName("at-the-click");
    await settle();
    // A change while Start's last question is under way is not the click's.
    t.world.onStart = (n) => { if (n === 2) { t.act.setName("after-the-click"); t.act.chooseAccount(t.row("work")); } };
    const before = t.world.calls.length;
    await t.start();
    const sent = t.world.calls.slice(before).map((c) => c.command).filter((c) => c !== "newSessionForm");
    expect(sent).toEqual(["newSessionStart", "launchWarning", "newSessionStart", "newSessionPlan", "creationSubmit", "launchAccountsClose"]);
    // The plan and Start read the same input, and the choices of the same turn.
    const [startInput] = t.world.of("newSessionStart")[1].args as [NewSessionFormInput];
    const [planInput, choices] = t.world.of("newSessionPlan")[0].args as [NewSessionFormInput, Json];
    expect(planInput).toEqual(startInput);
    expect(choices).toMatchObject({ name: "at-the-click", account: "other", agent: startInput.form.agent, path: startInput.form.path });
  });

  it("two Start clicks submit once", async () => {
    const t = setup();
    await t.open({ agent: "codex", path: "/workspace/example" });
    let answer!: (ok: boolean) => void;
    t.world.warning = "Launch anyway?";
    const first = t.store.startNewSession(() => new Promise((resolve) => { answer = resolve; }));
    await settle();
    const second = t.store.startNewSession(async () => true);
    answer(true);
    await Promise.all([first, second]);
    await settle();
    expect(t.world.count("creationSubmit")).toBe(1);
    expect(t.world.count("launchWarning")).toBe(1);
  });

  it("a refused admission keeps every choice and shows the reason", async () => {
    const t = setup((w) => { w.admission = { refused: "Too many creations are waiting. Let one finish first." }; });
    await t.open({ agent: "codex", path: "/workspace/example" });
    await t.choose("work");
    t.act.setName("keep-me");
    await settle();
    await t.start();
    expect(t.store.newSession()).toBeDefined();
    expect(t.state()).toMatchObject({ error: "Too many creations are waiting. Let one finish first.", submitted: false, name: "keep-me", folderPath: "/workspace/example" });
    expect(t.state().accounts[t.state().account].id).toBe("work");
    expect(t.world.admitted).toEqual([]);
    // The next Start submits again: only a refusal clears the submission.
    t.world.admission = { admitted: "creation-2" };
    await t.start();
    expect(t.world.count("creationSubmit")).toBe(2);
    expect(t.world.admitted).toEqual(["creation-2"]);
  });

  // The contract Task 5 implements: a refusal clears the submission; a lost call is an unknown outcome.
  it("a lost submission keeps it submitted and is never sent again", async () => {
    const t = setup((w) => { w.admission = new Error("hgs did not answer"); });
    await t.open({ agent: "codex", path: "/workspace/example" });
    await t.start();
    expect(t.store.newSession()).toBeDefined();
    expect(t.state()).toMatchObject({ error: "hgs did not answer", submitted: true });
    t.world.admission = { admitted: "creation-2" };
    await t.start();
    expect(t.world.count("creationSubmit")).toBe(1);
    expect(t.world.admitted).toEqual([]);
  });

  it("an admission after the dialog closed still reaches the creations", async () => {
    const t = setup();
    await t.open({ agent: "codex", path: "/workspace/example" });
    t.world.hold("submit");
    const started = t.store.startNewSession(async () => true);
    await settle();
    expect(t.world.count("creationSubmit")).toBe(1);
    t.store.closeNewSession();
    t.world.release("submit");
    await started;
    expect(t.world.admitted).toEqual(["creation-1"]);
    expect(t.store.newSession()).toBeUndefined();
  });

  it("carries the computer's identity and the folder's project row", async () => {
    const t = setup((w) => {
      w.cards.mac = machineMeta("mac-mini.example.invalid");
      w.projects = [project("work", "Work", [folder("f1", "mac", "/srv/repo")], { is_default: true })];
    });
    await t.open({ agent: "codex", project: "work", computer: "mac", folder: "f1", recipe: { id: "failed-1", tag: "retry", account: "work", source: "/srv/repo", target: "/srv/repo", auto: false } });
    await settle();
    await t.start();
    expect(t.choices()).toMatchObject({ host: "mac", machine: "mac-mini.example.invalid", project: "work", project_name: "Work", listed_folder: true, path: "/srv/repo", predecessor: "failed-1", name: "retry", account: "work" });
  });
});

describe("opening (SessionsWindow::showNewSession 2895-2928)", () => {
  const row = (extra: Partial<Session>) => ({ computer: "mac", cwdPath: "/srv/session", cwdCanonical: "/srv/canonical", project: "Work", ...extra } as Session);

  it("takes the selected session's project, else the default; a project option wins and makes it explicit", async () => {
    const projects = [project("ungrouped", "Inbox", [], { is_default: true }), project("work", "Work"), project("docs", "Docs")];
    const t = setup((w) => { w.projects = projects; w.selection = { row: row({}) }; });
    await t.open();
    expect(t.state().projects[t.state().project].id).toBe("work");
    t.store.closeNewSession();
    t.world.selection = undefined;
    await t.open();
    expect(t.state().projects[t.state().project].id).toBe("ungrouped");
    t.store.closeNewSession();
    await t.open({ project: "Docs", path: "/srv/x" });
    expect(t.state().projects[t.state().project].id).toBe("docs");
    await t.start();
    expect(t.choices()).toMatchObject({ project: "docs", implicit_inbox: false });
  });

  it("uses the selected session as context only for a generic open", async () => {
    const t = setup((w) => {
      w.projects = [project("work", "Work", [folder("f1", "mac", "/srv")], { is_default: true })];
      w.selection = { row: row({}), details: { cwd: "/srv/details", cwd_canonical: null } as Details };
    });
    await t.open();
    expect(t.state().host).toBe("mac");
    // The folder is the details' cwd, matched to the project folder holding it.
    expect(t.state().folderPath).toBe("/srv");
    expect(t.world.of("worktreeCatalog").find((c) => c.args[2] === true)?.args).toEqual(["mac", "/srv/details", true]);
    t.store.closeNewSession();
    await t.open({ path: "/srv/other" });
    expect(t.world.of("worktreeCatalog").filter((c) => c.args[2] === true).length).toBe(1);
    expect(t.state().host).toBe("");
  });

  it("a changed project folder raises a sticky notice and opens nothing", async () => {
    const t = setup((w) => { w.projects = [project("work", "Work", [folder("f1", "pc", "/srv/a")], { is_default: true })]; });
    await t.open({ project: "work", folder: "gone" });
    expect(t.store.newSession()).toBeUndefined();
    expect(t.world.notices).toEqual([{ tone: "error", text: "This project folder changed. Select it again.", sticky: true }]);
    expect(t.world.count("launchAccountsClose")).toBe(1);
    await t.open({ project: "work", folder: "f1" });
    expect(t.state().folderPath).toBe("/srv/a");
    expect(t.state().folders[t.state().folder].id).toBe("f1");
  });

  it("a failure while opening disposes the dialog it made", async () => {
    const t = setup();
    const recipe = Object.defineProperty({}, "tag", { enumerable: true, get: () => { throw new Error("unreadable recipe"); } });
    await t.open({ agent: "codex", recipe });
    expect(t.store.newSession()).toBeUndefined();
    expect(t.world.notices).toEqual([{ tone: "error", text: "unreadable recipe", sticky: true }]);
    expect(t.world.count("launchAccountsClose")).toBe(1);
    expect(t.world.listeners.size).toBe(0);
    expect(t.world.ticks.length).toBe(0);
    await t.open();
    expect(t.store.newSession()).toBeDefined();
  });

  it("opens one dialog at a time and starts from Qt's defaults", async () => {
    const t = setup();
    await t.open();
    await t.open({ agent: "claude" });
    expect(t.world.count("newSessionContext")).toBe(1);
    expect(t.state()).toMatchObject({ name: "work-1a2b3c4d", openTerminal: false, autoPreferred: true, agent: "codex" });
  });

  // setFleet (478-486) runs for every fleet change the form reads, the session host included (launch.rs 178-185).
  it("the fleet key changes with this computer's session host", () => {
    const card = machineMeta("pc", {}, { platform: "windows", local_sessions: false });
    const key = (local: MachineMeta) => fleetKey({ "": local, mac: machineMeta("mac") }, ["mac"], () => true);
    const before = key(card);
    expect(key({ ...card })).toBe(before);
    expect(key({ ...card, local_host: { state: "stopped", pid: null, protocol: 1, conpty: "bundled", integrity: "medium", windows_session: null, error: null } })).not.toBe(before);
    expect(key({ ...card, local_sessions: true })).not.toBe(before);
    expect(fleetKey({ "": card }, ["mac"], (c) => c !== "mac")).not.toBe(fleetKey({ "": card }, ["mac"], () => true));
  });

  it("selectedHostTarget names a single peer filter only", () => {
    const t = setup();
    expect(t.store.selectedHostTarget()).toBe("");
    t.world.filters = ["mac"];
    expect(t.store.selectedHostTarget()).toBe("mac");
    t.world.filters = [""];
    expect(t.store.selectedHostTarget()).toBe("");
    t.world.filters = ["mac", "studio"];
    expect(t.store.selectedHostTarget()).toBe("");
  });

  it("Manage this project's folders and Open Machines close first", async () => {
    const t = setup((w) => { w.projects = [project("work", "Work", [], { is_default: true })]; });
    await t.open();
    t.act.manageFolders();
    expect(t.store.newSession()).toBeUndefined();
    await t.open();
    t.act.openMachines();
    expect(t.store.newSession()).toBeUndefined();
    expect(t.world.opened).toEqual(["projects:work", "machines"]);
  });
});

describe("the Projects entries (ProjectsDialog.cpp 523-541)", () => {
  it("checks the folder is still the project's and can start a session", async () => {
    const f1 = folder("f1", "pc", "/srv/a"), remote = folder("f2", "mac", "/srv/b");
    const t = setup((w) => { w.projects = [project("work", "Work", [f1, remote], { is_default: true })]; });
    expect(await t.store.openFolderSession("work", { ...f1, path: "/srv/moved" })).toBe("This project folder changed. Select it again.");
    t.world.localExists = false;
    expect(await t.store.openFolderSession("work", f1)).toBe("This folder is unavailable on this computer.");
    t.world.online.mac = false;
    expect(await t.store.openFolderSession("work", remote)).toBe("Reconnect mac to start a session in this folder.");
    t.world.cards[""] = machineMeta("");
    expect(await t.store.openFolderSession("work", f1)).toBe("Waiting for this computer's identity.");
    expect(t.store.newSession()).toBeUndefined();
    t.world.cards[""] = machineMeta("pc");
    t.world.localExists = true;
    expect(await t.store.openFolderSession("work", f1)).toBe("");
    await settle();
    expect(t.state().folders[t.state().folder].id).toBe("f1");
    expect(t.world.of("localFolderExists").map((c) => c.args)).toEqual([["/srv/a"], ["/srv/a"]]);
  });
});

describe("reads only (Review Focus 1 and 2)", () => {
  it("opening, choosing, browsing and Cancel send only reads", async () => {
    const t = setup((w) => {
      w.flags.add("opencode");
      w.flags.add("opencode-models");
      w.flags.add("opencode-stopped");
      w.projects = [project("work", "Work", [folder("f1", "pc", "/workspace/example")], { is_default: true })];
      w.picked = "/workspace/picked";
    });
    await t.open({ agent: "opencode" });
    t.act.chooseComputer(t.state().computers.findIndex((c) => c.target === "mac"));
    await settle();
    t.act.chooseComputer(t.state().computers.findIndex((c) => c.target === ""));
    await settle();
    await t.act.browseLocal();
    await settle();
    expect(t.state().folderPath).toBe("/workspace/picked");
    t.act.openModelPicker();
    await settle();
    t.act.pickerToggleAll();
    await settle();
    t.act.setSource(true);
    t.act.setUrl("git@example.invalid:team/fresh.git");
    t.act.setSource(false);
    t.act.setAuto(false);
    t.act.setAuto(true);
    t.act.refreshAccounts();
    await settle();
    t.store.closeNewSession();
    await settle();
    expect(t.world.creating()).toEqual([]);
    const commands = new Set(t.world.calls.map((c) => c.command));
    for (const command of commands) expect(["newSessionContext", "newSessionForm", "repositoryFields", "worktreeSnapshot", "worktreeCatalog", "pickLocalFolder", "openCodeModels", "launchAccounts", "launchAccountsClose"]).toContain(command);
  });

  it("the prefill's late answer after a folder change does nothing", async () => {
    const t = setup((w) => {
      w.flags.add("prefill-linked");
      w.projects = [project("example", "Example", [folder("f1", "pc", "/workspace/example")], { is_default: true })];
      w.selection = { row: { computer: "", cwdPath: "/workspace/example-work-one", cwdCanonical: "", project: "Example" } as Session };
    });
    t.world.hold("prefill");
    await t.open({ agent: "codex" });
    expect(t.state().folderPath).toBe("/workspace/example-work-one");
    t.act.selectPath("/workspace/elsewhere", false);
    t.world.release("prefill");
    await settle();
    expect(t.state().folderPath).toBe("/workspace/elsewhere");
    expect(t.state().form?.preview).not.toBe("Checking the session's main working folder…");
  });

  it("the prefill moves a linked worktree's session to its main checkout", async () => {
    const t = setup((w) => {
      w.flags.add("prefill-linked");
      w.projects = [project("example", "Example", [folder("f1", "pc", "/workspace/example")], { is_default: true })];
      w.selection = { row: { computer: "", cwdPath: "/workspace/example-work-one", cwdCanonical: "", project: "Example" } as Session };
    });
    await t.open({ agent: "codex" });
    expect(t.state().folders[t.state().folder].id).toBe("f1");
    expect(t.state().folderPath).toBe("/workspace/example");
    // The verified catalog travels with the form for that folder.
    const input = t.world.of("newSessionForm").at(-1)?.args[0] as NewSessionFormInput;
    expect(input.verified).toMatchObject({ selected_root: "/workspace/example-work-one" });
  });
});
