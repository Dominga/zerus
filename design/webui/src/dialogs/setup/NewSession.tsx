import { For, Show, createEffect, createMemo, createSignal, on, onCleanup, onMount, untrack } from "solid-js";
import { AgentGlyph, AGENT_LABEL, Icon } from "../../lib/icons";
import { UsageMeters, type UsageIndicator } from "../../components/UsageMeter";
import { accountWindows, usageTooltip } from "../../lib/usage";
import { prefs } from "../../screens/settings/store";
import { accounts, machines } from "../../mock/data";
import { LAUNCH_AGENTS, accountIdentity, catalogFor, extraAccounts } from "../../mock/setup-extra";
import type { Account, AgentKind, Machine } from "../../mock/types";
import { demoState, selected, setMenu, setPage, toast } from "../../state";
import {
  org, projectById, launchPreset, setLaunchPreset, setDirPick, setSelectedProject, setSelectedFolder, projectColor,
} from "../../screens/setup/store";
import { RichSelect, SDialog, Segmented, closeSetup, hex8, isOpen, join, leaf, openSetup, parentOf, Progress, MockSwitch } from "../../screens/setup/ui";
import { WorktreePanel } from "../../screens/setup/WorktreePanel";
import { OpenCodeModelPicker, modelName, modelVariants } from "./ModelPicker";
import { startCreation } from "./Creation";

// New session (NewSessionDialog.cpp). Opening the form, choosing a folder or
// checking a folder never creates anything: a generated worktree is created
// by the background creation pipeline only after Start session.

const review = new URLSearchParams(location.search).get("setup") ?? "";

const NEW_PROJECT = "__new__";
type Source = "folder" | "repo";
type RowId = string; // account id, or "shell"

const HOME: Record<string, string> = { workstation: "/home/example", macbook: "/Users/example", "build-01": "/srv/example", "build-02": "/srv/example", "win-lab": "C:\\src\\example" };

/** SessionTag::problem (SessionTag.h:10-18). */
export function nameProblem(name: string): string {
  if (!name) return "Enter a session name.";
  if (name !== name.trim()) return "Remove spaces at the beginning or end.";
  // eslint-disable-next-line no-control-regex
  if (/[/:.\u0000-\u001f\u007f]/.test(name)) return "Names cannot contain /, : or . or control characters.";
  return "";
}

/** RepositoryClone::suggestedName: last / or : segment, minus .git, sanitized, ≤ 80. */
export function suggestedName(url: string): string {
  const seg = url.trim().replace(/\/+$/, "").split(/[/:]/).pop() ?? "";
  const name = seg.replace(/\.git$/i, "").replace(/[^A-Za-z0-9._-]+/g, "-").slice(0, 80);
  return name === "." || name === ".." ? "" : name;
}

/** SessionCreationController URL safety (`:27-32`). */
export function urlProblem(url: string): string {
  if (!url.includes("://")) return "";
  try {
    const u = new URL(url);
    if (u.password || u.search || u.hash || (u.username && u.protocol !== "ssh:")) return "Use a repository address without embedded passwords, tokens, query or fragment.";
  } catch { return ""; }
  return "";
}

const usable = [...accounts, ...extraAccounts].filter((a) => (a as { installed?: boolean }).installed !== false);
const accountsOn = (alias: string) =>
  usable.filter((a) => a.machine === alias).sort((a, b) => LAUNCH_AGENTS.indexOf(a.agent) - LAUNCH_AGENTS.indexOf(b.agent));
const accountName = (a: Account) => {
  const id = accountIdentity[a.id];
  const n = id?.email ?? a.label;
  return !n || n === "Default account" || n === "Native account" ? "Account n/a" : n;
};
const usageSummary = (a: Account) => (a.fiveHour || a.week ? `5h ${a.fiveHour}% used  7d ${a.week}% used` : "limits n/a");
const usageTone = (a: Account) => { const p = Math.max(a.fiveHour, a.week); return p >= 90 ? "danger" : p >= 70 ? "warn" : ""; };

// Context for the child dialogs opened from this form.
const [wtContext, setWtContext] = createSignal<{ machine: string; source: string; branches: string[]; branch: string; onCreated: (path: string) => void }>();
const [pickContext, setPickContext] = createSignal<{ machine: string; source: string; project: string; onUse: (path: string) => void }>();
const [warnContext, setWarnContext] = createSignal<{ onLaunch: () => void }>();
const [wtBusy, setWtBusy] = createSignal(false);
const [nsBusy, setNsBusy] = createSignal(false);

function NewSessionForm() {
  const preset = untrack(launchPreset);
  setLaunchPreset({});
  const restore = preset.restore;
  const noMachines = () => demoState() === "empty";
  const computers = createMemo<Machine[]>(() => (noMachines() ? [] : machines));
  const sel = untrack(selected);

  // Target ---------------------------------------------------------------
  const [projectId, setProjectId] = createSignal(restore?.project ?? preset.project ?? sel?.project ?? "general");
  const [projectExplicit, setProjectExplicit] = createSignal(!!(preset.project || restore));
  const [machine, setMachine] = createSignal(restore?.machine ?? preset.machine ?? sel?.machine ?? "workstation");
  const [newProjectName, setNewProjectName] = createSignal("");
  const project = () => projectById(projectId());
  const host = () => computers().find((m) => m.alias === machine());
  const online = () => host()?.connection === "online" && demoState() !== "error";
  const windowsHost = () => host()?.platform === "windows";

  // Source ---------------------------------------------------------------
  const [source, setSource] = createSignal<Source>(restore?.repository || preset.repository || review === "repo" || review === "uncertain" ? "repo" : "folder");
  const [path, setPath] = createSignal<string>(restore?.folder ?? preset.path ?? "");
  const [browsed, setBrowsed] = createSignal<Record<string, string>>({});
  const [prefill, setPrefill] = createSignal(!preset.path && !restore && !!sel);
  const folders = () => (host() ? (project()?.folders ?? []).filter((f) => f.machine === machine()) : []);
  const inProject = () => folders().some((f) => f.path === path());
  const catalog = createMemo(() => (path() ? catalogFor(machine(), path()) : undefined));
  const isProjectWorktree = () => !!project()?.folders.some((f) => f.machine === machine() && catalogFor(f.machine, f.path).checkouts.some((c) => c.path === path()));
  const outside = () => !!path() && !inProject() && !isProjectWorktree();
  const implicitInbox = () => !projectExplicit() && projectId() === "general";

  // Worktree -------------------------------------------------------------
  const [autoPreferred, setAutoPreferred] = createSignal(review !== "none");
  const [catalogLoading, setCatalogLoading] = createSignal(false);
  const [generated] = createSignal(restore?.worktree?.branch ?? `work-${hex8()}`);
  const autoAvailable = () => source() === "repo" || (!!catalog() && catalog()!.state === "ok" && online() && !catalogLoading());
  const automatic = () => autoPreferred() && autoAvailable();
  const linkedCount = () => catalog()?.checkouts.filter((c) => c.kind === "linked" && !c.unavailable).length ?? 0;

  // Repository -----------------------------------------------------------
  const [url, setUrl] = createSignal(review === "repo" || review === "uncertain" ? "git@example.org:team/telemetry.git" : restore?.repository ?? "");
  const [dest, setDest] = createSignal("");
  const [customDest, setCustomDest] = createSignal(false);
  const [cloneState, setCloneState] = createSignal<"none" | "uncertain" | "checking" | "ready">(review === "uncertain" ? "uncertain" : "none");
  const autoDest = () => join(join(HOME[machine()] ?? "/home/example", "src"), suggestedName(url()) || "repository");
  const destination = () => (customDest() ? dest() : autoDest());
  const repoLocked = () => cloneState() !== "none";
  const canSubmit = () => cloneState() === "ready" || (cloneState() === "none" && !!url().trim() && !urlProblem(url()) && !!suggestedName(url()) && destination().length > 1);

  // Account and agent ----------------------------------------------------
  const [accountsLoading, setAccountsLoading] = createSignal(false);
  const [accountStatus, setAccountStatus] = createSignal("");
  const rows = createMemo(() => (accountsLoading() || !host() ? [] : accountsOn(machine())));
  const firstFor = (agent: string) => rows().find((a) => a.agent === agent)?.id;
  const [row, setRow] = createSignal<RowId | undefined>();
  const agent = (): AgentKind | "shell" | undefined => (row() === "shell" ? "shell" : rows().find((a) => a.id === row())?.agent);
  const preferred = review === "opencode" ? "opencode" : restore ? restore.agent : "claude";

  // OpenCode -------------------------------------------------------------
  const [model, setModel] = createSignal("");
  const [effort, setEffort] = createSignal("");
  const [service, setService] = createSignal<"stopped" | "starting" | "running">("stopped");
  const [pickerOpen, setPickerOpen] = createSignal(false);
  let modelBtn!: HTMLButtonElement;
  let modelPop!: HTMLDivElement;
  // The picker is a light-dismiss popover in the top layer, placed under its button.
  const togglePicker = () => {
    if (pickerOpen()) { modelPop.hidePopover(); return; }
    const r = modelBtn.getBoundingClientRect();
    const below = innerHeight - r.bottom - 16;
    const h = Math.min(460, Math.max(below, r.top - 16));
    modelPop.style.left = `${Math.min(r.left, innerWidth - 500)}px`;
    modelPop.style.width = `${Math.max(r.width, 480)}px`;
    modelPop.style.maxHeight = `${h}px`;
    modelPop.style.top = below >= 300 ? `${r.bottom + 4}px` : `${Math.max(8, r.top - 4 - h)}px`;
    modelPop.showPopover();
  };

  // Name, terminal, errors -----------------------------------------------
  const [name, setName] = createSignal(restore?.tag ?? generated());
  const [terminal, setTerminal] = createSignal(false);
  const [error, setError] = createSignal("");
  const problem = () => nameProblem(name());

  // Machine change: reload folders and accounts; browsed folders stick per computer.
  createEffect(on(machine, (m) => {
    setAccountsLoading(true);
    setAccountStatus("Loading accounts…");
    const t = setTimeout(() => {
      setAccountsLoading(false);
      setAccountStatus(m === "build-02" ? "Could not load accounts: build-02 is offline." : "");
      const keep = row();
      if (keep === "shell" || (!keep && preferred === "shell")) { setRow("shell"); return; }
      setRow(rows().find((a) => a.id === keep)?.id ?? firstFor(preferred) ?? firstFor("codex") ?? rows()[0]?.id ?? "shell");
    }, 450);
    onCleanup(() => clearTimeout(t));
  }));
  createEffect(on([projectId, machine], () => {
    if (untrack(prefill)) return;
    const keep = untrack(path);
    if (keep && (folders().some((f) => f.path === keep) || browsed()[machine()] === keep)) return;
    setPath(browsed()[machine()] ?? folders()[0]?.path ?? "");
  }, { defer: true }));
  // Reading the folder's Git catalog keeps Start disabled while Auto waits for it.
  createEffect(on(path, (p) => {
    if (!p) return;
    setCatalogLoading(true);
    const t = setTimeout(() => setCatalogLoading(false), 550);
    onCleanup(() => clearTimeout(t));
  }));
  createEffect(on(model, (m) => { if (effort() && !modelVariants(m).includes(effort())) setEffort(""); }, { defer: true }));

  onMount(() => {
    // Prefill from the selected session: its project folder, or the main
    // checkout when the session runs in a linked worktree of that folder.
    if (prefill()) {
      setTimeout(() => {
        const match = (project()?.folders ?? []).find((f) => f.machine === machine() && (sel!.cwd === f.path || sel!.cwd.startsWith(`${f.path}-`)));
        setPath(match?.path ?? folders()[0]?.path ?? "");
        setPrefill(false);
      }, 650);
    }
    else if (!path()) setPath(folders()[0]?.path ?? "");
    if (review === "opencode") setTimeout(togglePicker, 900);
  });

  const browse = () => {
    if (!host()) return;
    if (host()!.thisComputer) {
      const picked = "/home/example/src/tools";
      setBrowsed((b) => ({ ...b, [machine()]: picked }));
      setPath(picked);
      toast("Choose a folder", "The system folder dialog of this computer opens here");
      return;
    }
    setDirPick({
      machine: machine(), path: path() || HOME[machine()] || "/",
      onPick: (p) => { setBrowsed((b) => ({ ...b, [machine()]: p })); setPath(p); },
    });
    openSetup("setup-directory");
  };
  const browseRepo = () => {
    setDirPick({ machine: machine(), path: parentOf(destination()), onPick: (p) => { setCustomDest(true); setDest(join(p, suggestedName(url()) || "repository")); } });
    openSetup("setup-directory");
  };

  const worktreeMenu = (e: MouseEvent) => {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    setMenu({
      x: r.left, y: r.bottom + 4, items: [
        { label: "Choose existing…", icon: "git-branch", disabled: !linkedCount() || catalogLoading(), action: () => {
          setPickContext({ machine: machine(), source: path(), project: projectId(), onUse: (p) => { setPath(p); setAutoPreferred(false); } });
          openSetup("setup-choose-worktree");
        } },
        { label: "Create manually…", icon: "plus", disabled: catalog()?.state !== "ok" || !online() || catalogLoading(), action: () => {
          setWtContext({ machine: machine(), source: path(), branches: catalog()?.branches ?? ["main"], branch: name(), onCreated: (p) => { setPath(p); setAutoPreferred(false); } });
          openSetup("setup-new-worktree");
        } },
      ],
    });
  };

  // Start enablement (NewSessionDialog.cpp:687-693) -----------------------
  const ready = () => {
    if (source() === "repo") return online() && canSubmit() && (projectId() !== NEW_PROJECT || !!newProjectName().trim());
    return !prefill() && !!path() && online() && !(autoPreferred() && catalogLoading());
  };
  const canStart = () => !!host() && !!row() && (!accountsLoading() || row() === "shell") && !problem() && ready() && cloneState() !== "checking";
  const primary = () => (source() === "repo" ? (cloneState() === "checking" ? "Checking…" : cloneState() === "ready" ? "Use checkout and start" : "Clone and start") : prefill() ? "Checking folder…" : "Start session");

  const errorLine = () => {
    if (!computers().length) return "Enable local sessions for this computer in Machines, or add a remote machine.";
    if (source() === "folder" && path() && !online()) return "This computer is unavailable. Choose another folder or reconnect it.";
    if (!accountsLoading() && rows().length === 0 && row() !== "shell") return "Restore or add an account in Agent accounts to start this agent.";
    if (source() === "repo" && urlProblem(url())) return urlProblem(url());
    return error();
  };

  const launch = () => {
    const a = agent();
    const record = startCreation({
      tag: name(),
      project: projectId() === NEW_PROJECT ? newProjectName().trim() : projectId(),
      machine: machine(),
      agent: a ?? "shell",
      folder: source() === "repo" ? destination() : path(),
      worktree: automatic() ? { path: `${source() === "repo" ? destination() : path()}-${generated()}`, branch: generated() } : undefined,
      model: a === "opencode" ? (model() ? `${modelName(model())}${effort() ? ` (${effort()})` : ""}` : "Native default model") : undefined,
      repository: source() === "repo" ? url().trim() : undefined,
      predecessor: restore?.id,
    });
    closeSetup("new-session");
    toast("Session creation started", `${record.tag}: ${record.history[0].text}`);
    openSetup("setup-creation");
  };
  const start = () => {
    if (!canStart()) return;
    // Only a local Windows launch asks first; Cancel keeps the form unchanged.
    if (host()?.thisComputer && windowsHost()) { setWarnContext({ onLaunch: launch }); openSetup("setup-launch-warning"); return; }
    launch();
  };

  const projectOptions = () => [
    ...org.projects.map((p) => ({ value: p.id, label: p.name, swatch: projectColor(p), vivid: p.vivid, detail: p.isDefault ? "Default" : undefined })),
    ...(source() === "repo" ? [{ value: NEW_PROJECT, label: "New project…", icon: "plus" }] : []),
  ];
  const computerOptions = () => computers().map((m) => ({
    value: m.alias, label: m.thisComputer ? `${m.alias} (this computer)` : m.alias,
    detail: m.connection === "offline" || (demoState() === "error" && !m.thisComputer) ? "(offline)" : m.connection === "reconnecting" ? "(stale)" : undefined,
    swatch: `var(--z-entity-palette-${m.colorIndex})`,
    tile: { os: m.platform, state: m.connection === "offline" || (demoState() === "error" && !m.thisComputer) ? "offline" as const : m.connection === "reconnecting" ? "connecting" as const : "online" as const },
  }));

  const preview = () =>
    prefill() ? "Checking the session's main working folder…"
    : !path() ? "Browse to choose a folder on this computer."
    : implicitInbox() ? "A matching project is detected automatically.\nOtherwise this session stays in Inbox."
    : outside() ? "This folder is outside the project.\nStarting here will add it to this project."
    : "";
  const busy = () => cloneState() === "checking";
  createEffect(() => setNsBusy(busy()));
  onCleanup(() => setNsBusy(false));

  return (
    <>
      <div class="dlg-body ns-body">
        <div class="form-row">
          <label>Target</label>
          <div class="ns-target">
            <RichSelect label="Project" value={projectId()} options={projectOptions()} disabled={busy()} class="grow"
              onChange={(v) => { setProjectId(v); setProjectExplicit(true); if (v === NEW_PROJECT && !newProjectName()) setNewProjectName(suggestedName(url())); }} />
            <button class="icon-btn" data-tip="Manage this project's folders" aria-label="Manage this project's folders" disabled={busy() || projectId() === NEW_PROJECT}
              onClick={() => { const id = projectId(); closeSetup("new-session"); setSelectedProject(id); setSelectedFolder(0); setPage("projects"); }}>
              <Icon name="settings" size={16} />
            </button>
            <RichSelect label="Computer" value={computers().length ? machine() : ""} options={computerOptions()} disabled={busy() || !computers().length} class="ns-computer"
              placeholder={computers().length ? undefined : "No computer"} onChange={(v) => setMachine(v)} />
          </div>
        </div>
        <Show when={source() === "repo" && projectId() === NEW_PROJECT}>
          <div class="form-row">
            <label for="ns-newproj">New project</label>
            <input id="ns-newproj" class="field" maxLength={80} placeholder="Project name" value={newProjectName()} onInput={(e) => setNewProjectName(e.currentTarget.value)} />
          </div>
        </Show>
        <div class="form-row">
          <label>Source</label>
          <Segmented label="Source" value={source()} disabled={busy()} onChange={(v) => { setSource(v); if (v === "folder" && projectId() === NEW_PROJECT) setProjectId("general"); }}
            options={[{ value: "folder", label: "Working folder", icon: "folder" }, { value: "repo", label: "Git repository", icon: "git-branch" }]} />
        </div>

        <Show when={source() === "folder"} fallback={
          <>
            <div class="form-row">
              <label for="ns-url">Repository</label>
              <input id="ns-url" class="field mono" maxLength={4096} placeholder="https://github.com/owner/repository.git" title="An SSH or HTTPS repository URL, on GitHub or another Git server."
                value={url()} disabled={repoLocked() || !online()} onInput={(e) => setUrl(e.currentTarget.value)} />
            </div>
            <div class="form-row top">
              <label for="ns-dest">Clone to</label>
              <div class="col ns-src-detail">
                <div class="row">
                  <input id="ns-dest" class="field mono grow" maxLength={8192} title="The complete new checkout path. Edit its folder name or choose a parent folder with Browse."
                    value={destination()} disabled={repoLocked() || !online()} onInput={(e) => { setCustomDest(true); setDest(e.currentTarget.value); }} />
                  <button class="btn" disabled={repoLocked() || !online()} onClick={browseRepo}>Browse…</button>
                </div>
                <Show when={cloneState() !== "none"}>
                  <div class="ns-repo-state" classList={{ warn: cloneState() === "uncertain" }}>
                    {cloneState() === "uncertain" ? "A previous clone is unconfirmed. Check its original result before continuing."
                      : cloneState() === "checking" ? "Checking repository access and destination…" : "Checkout is ready. Confirm to use it."}
                  </div>
                  <Show when={cloneState() === "checking"}><Progress label="Checking the previous clone" /></Show>
                  <Show when={cloneState() === "uncertain"}>
                    <div class="row">
                      <button class="btn small" disabled={!online()} onClick={() => { setCloneState("checking"); setTimeout(() => setCloneState("ready"), 900); }}>Check result</button>
                      <button class="btn small" onClick={() => { setCloneState("none"); setCustomDest(true); setDest(`${destination()}-2`); }}>Choose another destination…</button>
                    </div>
                  </Show>
                </Show>
              </div>
            </div>
          </>
        }>
          <div class="form-row">
            <label for="ns-folder">Working folder</label>
            <div class="row">
              <select id="ns-folder" class="field grow" aria-label="Working folder" disabled={busy() || prefill()} value={inProject() ? path() : ""}
                onChange={(e) => setPath(e.currentTarget.value)}>
                <option value="" disabled>{path() && !inProject() ? leaf(path()) : "Choose a folder…"}</option>
                <For each={folders()}>{(f) => <option value={f.path} selected={f.path === path()}>{f.name || leaf(f.path)}</option>}</For>
              </select>
              <button class="btn" disabled={busy() || !online()} onClick={browse}>Browse…</button>
            </div>
          </div>
          <div class="form-row top">
            <span />
            <div class="ns-src-detail">
              <div class="mono ns-path ell" title={path()} style={{ visibility: path() ? "visible" : "hidden" }}>{path() || "—"}</div>
              <div class="ns-preview" classList={{ warn: outside() && !prefill() && !implicitInbox() }}>{preview()}</div>
            </div>
          </div>
        </Show>

        <div class="form-row top">
          <label>Worktree</label>
          <div class="col ns-wt">
            <div class="row">
              <Segmented label="Worktree" value={automatic() ? "auto" : "none"} disabled={busy()}
                onChange={(v) => setAutoPreferred(v === "auto")}
                options={[
                  { value: "auto", label: "Auto", icon: "git-branch", tip: "Create a generated branch and worktree when you start the session.", disabled: !autoAvailable() },
                  { value: "none", label: "None", icon: "folder", tip: "Start directly in the working folder." },
                ]} />
              <button class="icon-btn" aria-label="Worktree options" data-tip="Worktree options" aria-haspopup="menu"
                disabled={source() === "repo" || !catalog() || (!linkedCount() && catalog()?.state !== "ok") || !online() || busy()} onClick={worktreeMenu}>
                <Icon name="ellipsis" size={16} />
              </button>
              <Show when={source() === "folder" && path() && catalogLoading()}><span class="hint"><Icon name="loader-circle" size={12} class="su-spin" /> Reading folder…</span></Show>
              <Show when={source() === "folder" && path() && !catalogLoading() && catalog()?.state === "plain"}><span class="hint">Ordinary folder (no Git).</span></Show>
            </div>
            <div class="mono ns-wt-target ell" title={automatic() ? `New branch: ${generated()}\nStart from: HEAD` : undefined} style={{ visibility: automatic() ? "visible" : "hidden" }}>
              <Icon name="git-branch" size={12} /> {source() === "repo" ? `${destination()}-${generated()}` : `${path()}-${generated()}`}
            </div>
          </div>
        </div>

        <div class="form-row top">
          <label>Account and agent</label>
          <div class="col">
            <ul class="ns-accounts" role="listbox" aria-label="Account and agent" tabindex="0"
              onKeyDown={(e) => {
                const ids = [...rows().map((a) => a.id), "shell"];
                if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
                e.preventDefault();
                const i = ids.indexOf(row() ?? "");
                setRow(ids[Math.max(0, Math.min(ids.length - 1, i + (e.key === "ArrowDown" ? 1 : -1)))]);
              }}>
              <Show when={accountsLoading()}><li class="ns-acc-loading"><span class="skeleton" /><span class="skeleton" /></li></Show>
              <For each={rows()}>
                {(a) => (
                  <li role="option" aria-selected={row() === a.id} classList={{ on: row() === a.id }} onClick={() => setRow(a.id)}>
                    {/* AccountChoice::Delegate: a two-line provider logo, the account name in bold, then its meters. */}
                    <span class="ns-acc-logo" style={{ color: `var(--z-agent-${a.agent})` }} title={AGENT_LABEL[a.agent]}><AgentGlyph agent={a.agent} size={26} /></span>
                    <span class="col grow">
                      <span class="ell"><b>{accountName(a) === "Account n/a" ? `${AGENT_LABEL[a.agent]} account` : accountName(a)}</b></span>
                      {/* The status-footer meters, compact and without the provider badge (AccountChoice::Delegate). */}
                      <Show when={accountWindows(a).length} fallback={<span class={`ns-usage tabular ${usageTone(a)}`}>{usageSummary(a)}</span>}>
                        <span class="ns-meters" title={usageTooltip(AGENT_LABEL[a.agent], accountName(a), accountWindows(a))}>
                          <UsageMeters windows={accountWindows(a)} compact indicator={prefs.usageIndicator as UsageIndicator} />
                        </span>
                      </Show>
                    </span>
                    <Show when={row() === a.id}><Icon name="check" size={14} class="accent" /></Show>
                  </li>
                )}
              </For>
              <Show when={!accountsLoading() && host()}>
                <li role="option" aria-selected={row() === "shell"} classList={{ on: row() === "shell" }} onClick={() => setRow("shell")}>
                  <span class="ns-acc-logo muted"><Icon name="terminal" size={26} /></span>
                  <span class="col grow">
                    <b>{windowsHost() ? "Run PowerShell" : "Run shell"}</b>
                    <span class="ns-usage">Interactive terminal session</span>
                  </span>
                  <Show when={row() === "shell"}><Icon name="check" size={14} class="accent" /></Show>
                </li>
              </Show>
            </ul>
            <div class="row ns-acc-status">
              <span class="hint grow" classList={{ danger: accountStatus().startsWith("Could not") }}>{accountStatus()}</span>
              <button class="btn small" aria-label="Refresh accounts" disabled={accountsLoading() || !host()}
                onClick={() => { setAccountsLoading(true); setAccountStatus("Loading accounts…"); setTimeout(() => { setAccountsLoading(false); setAccountStatus(""); }, 500); }}>
                <Icon name="refresh-cw" size={12} />Refresh
              </button>
            </div>
          </div>
        </div>

        <Show when={agent() === "opencode"}>
          <div class="form-row">
            <label>Model</label>
            <div class="ns-model">
              <button class="btn ns-model-btn" ref={modelBtn} title="Models OpenCode offers on this computer. Native default uses OpenCode's configuration."
                aria-haspopup="dialog" aria-expanded={pickerOpen()} disabled={!path() && source() === "folder"} onClick={togglePicker}>
                <span class="ell">{model() ? modelName(model()) : "Native default model"}</span><Icon name="chevron-down" size={13} />
              </button>
              <div class="ocm-pop" popover="auto" ref={modelPop} role="dialog" aria-label="Choose a model" on:toggle={(e: ToggleEvent) => setPickerOpen(e.newState === "open")}>
                <Show when={pickerOpen()}>
                  <OpenCodeModelPicker value={model()} onChoose={(id) => { setModel(id); modelPop.hidePopover(); }} />
                  <button class="btn small" onClick={() => { setModel(""); modelPop.hidePopover(); }}>Use native default model</button>
                </Show>
              </div>
            </div>
          </div>
          <Show when={!path() && source() === "folder"}><div class="form-row"><span /><span class="hint">Choose a working folder to list all of its models.</span></div></Show>
          <Show when={service() !== "running"}>
            <div class="form-row">
              <span />
              <div class="row">
                <span class="hint grow">OpenCode is not running on {machine()}. Showing the last known models.</span>
                <button class="btn small" disabled={!online() || service() === "starting"} title={online() ? "Start the shared OpenCode service on this computer. Zerus never stops it." : `${machine()} is offline.`}
                  onClick={() => { setService("starting"); setTimeout(() => setService("running"), 1000); }}>
                  {service() === "starting" ? "Starting OpenCode…" : `Start OpenCode on ${machine()}`}
                </button>
              </div>
            </div>
          </Show>
          <Show when={modelVariants(model()).length}>
            <div class="form-row">
              <label for="ns-effort">Effort</label>
              <select id="ns-effort" class="field" title="Variants of the selected model. Default uses the model's own setting." value={effort()} onChange={(e) => setEffort(e.currentTarget.value)}>
                <option value="">Default</option>
                <For each={modelVariants(model())}>{(v) => <option value={v}>{v}</option>}</For>
              </select>
            </div>
          </Show>
        </Show>

        <div class="form-row">
          <label for="ns-name">Session name</label>
          <input id="ns-name" class="field mono" value={name()} aria-invalid={!!problem()} aria-describedby="ns-name-err" onInput={(e) => setName(e.currentTarget.value)} />
        </div>
        <Show when={problem()}><div class="form-row"><span /><span id="ns-name-err" class="danger ns-err">{problem()}</span></div></Show>
        <div class="form-row">
          <span />
          <label class="check" title={agent() === "deepseek" ? "DeepSeek opens in the workspace's Native UI." : agent() === "opencode" ? "OpenCode runs through its native server and opens in Activity." : "Also open an external terminal for this session."}>
            <input type="checkbox" checked={terminal() && agent() !== "deepseek" && agent() !== "opencode"} disabled={agent() === "deepseek" || agent() === "opencode"}
              onChange={(e) => setTerminal(e.currentTarget.checked)} />
            Open terminal window
          </label>
        </div>
        <Show when={source() === "repo"}>
          <div class="form-row"><span /><MockSwitch label="Previous clone" value={cloneState() === "uncertain" ? "uncertain" : "none"}
            options={[{ value: "none", label: "none" }, { value: "uncertain", label: "unconfirmed" }]} onChange={(v) => setCloneState(v === "uncertain" ? "uncertain" : "none")} /></div>
        </Show>
      </div>
      <div class="dlg-foot">
        <span class="ns-error grow" classList={{ danger: !!errorLine() }} role="status">{errorLine()}</span>
        <Show when={!computers().length}><button class="btn" onClick={() => { closeSetup("new-session"); setPage("machines"); }}>Open Machines</button></Show>
        <button class="btn" disabled={busy()} onClick={() => closeSetup("new-session")}>Cancel</button>
        <button class="btn primary ns-start" disabled={!canStart()} onClick={start}>{primary()}</button>
      </div>
    </>
  );
}

function NewWorktreeForm() {
  const ctx = untrack(wtContext)!;
  const repo = leaf(ctx.source);
  const [branch, setBranch] = createSignal(ctx.branch);
  const [base, setBase] = createSignal("HEAD");
  const [customDest, setCustomDest] = createSignal<string>();
  const sanitized = () => branch().replace(/[^A-Za-z0-9._-]+/g, "-");
  const dest = () => customDest() ?? join(parentOf(ctx.source), `${repo}-${sanitized()}`);
  const absolute = () => dest().startsWith("/") || /^[A-Za-z]:\\/.test(dest());
  const create = () => {
    setWtBusy(true);
    setTimeout(() => { setWtBusy(false); ctx.onCreated(dest()); closeSetup("setup-new-worktree"); toast("Worktree created", `${dest()} on ${ctx.machine}`); }, 1100);
  };
  return (
    <>
      <div class="dlg-body">
        <p class="muted su-pre">{`Computer: ${ctx.machine}\nRepository: ${ctx.source}`}</p>
        <div class="form-row"><label for="nw-branch">New branch</label><input id="nw-branch" class="field mono" placeholder="feature/my-task" disabled={wtBusy()} value={branch()} onInput={(e) => setBranch(e.currentTarget.value)} /></div>
        <div class="form-row">
          <label for="nw-base">Start from</label>
          <input id="nw-base" class="field mono" list="nw-bases" disabled={wtBusy()} value={base()} title="Local branch, tag, commit or an already fetched remote branch (for example origin/main)." onInput={(e) => setBase(e.currentTarget.value)} />
          <datalist id="nw-bases"><option value="HEAD" /><For each={ctx.branches}>{(b) => <option value={b} />}</For></datalist>
        </div>
        <div class="form-row">
          <label for="nw-dest">New folder</label>
          <div class="row">
            <input id="nw-dest" class="field mono grow" disabled={wtBusy()} value={dest()} onInput={(e) => setCustomDest(e.currentTarget.value)} />
            <button class="btn" disabled={wtBusy()} onClick={() => { setDirPick({ machine: ctx.machine, path: parentOf(dest()), onPick: (p) => setCustomDest(join(p, `${repo}-${sanitized()}`)) }); openSetup("setup-directory"); }}>Parent folder…</button>
          </div>
        </div>
        <Show when={wtBusy()}><Progress label="Creating worktree" /></Show>
        <p class="hint">Creates a new branch and working folder. It remains on this computer if you cancel or cannot start the session.</p>
      </div>
      <div class="dlg-foot">
        <span class="hint grow">{wtBusy() ? "Creating worktree…" : ""}</span>
        <button class="btn" disabled={wtBusy()} onClick={() => closeSetup("setup-new-worktree")}>Cancel</button>
        <button class="btn primary" disabled={wtBusy() || !branch().trim() || !base().trim() || !absolute()} onClick={create}>Create worktree</button>
      </div>
    </>
  );
}

function ChooseWorktree() {
  const ctx = untrack(pickContext)!;
  const first = catalogFor(ctx.machine, ctx.source).checkouts.find((c) => !c.unavailable)?.path;
  const [picked, setPicked] = createSignal(first);
  const use = (p: string) => { ctx.onUse(p); closeSetup("setup-choose-worktree"); };
  return (
    <>
      <div class="dlg-body"><WorktreePanel picker machine={ctx.machine} path={ctx.source} project={ctx.project} onSelect={setPicked} onUse={use} /></div>
      <div class="dlg-foot">
        <span class="grow" />
        <button class="btn" onClick={() => closeSetup("setup-choose-worktree")}>Cancel</button>
        <button class="btn primary" disabled={!picked()} onClick={() => use(picked()!)}>Use folder</button>
      </div>
    </>
  );
}

export function NewSessionDialogs() {
  // Review links: ?dialog=setup-opencode-models opens the form with OpenCode
  // and its model popup; ?dialog=setup-new-worktree / setup-choose-worktree
  // open those steps over a New session for the aurora checkout.
  createEffect(() => {
    if (isOpen("setup-new-worktree") && !wtContext()) setWtContext({ machine: "workstation", source: "/home/example/src/aurora", branches: catalogFor("workstation", "/home/example/src/aurora").branches, branch: `work-${hex8()}`, onCreated: () => {} });
    if (isOpen("setup-choose-worktree") && !pickContext()) setPickContext({ machine: "workstation", source: "/home/example/src/aurora", project: "aurora", onUse: () => {} });
    if (isOpen("setup-launch-warning") && !warnContext()) setWarnContext({ onLaunch: () => toast("Launch anyway", "The session host starts in the other Windows session") });
  });
  return (
    <>
      <SDialog id="new-session" title="New session" width="660px" class="ns-dlg" bare busy={nsBusy}>
        <NewSessionForm />
      </SDialog>
      <SDialog id="setup-new-worktree" title="New worktree" width="640px" icon="git-branch" busy={wtBusy} bare>
        <Show when={wtContext()}><NewWorktreeForm /></Show>
      </SDialog>
      <SDialog id="setup-choose-worktree" title="Choose an existing worktree" width="620px" height="min(480px, calc(100vh - 64px))" icon="git-branch" bare>
        <Show when={pickContext()}><ChooseWorktree /></Show>
      </SDialog>
      <SDialog id="setup-launch-warning" title="Launch in another Windows session?" width="480px" icon="triangle-alert"
        footer={<>
          <span class="grow" />
          <button class="btn" onClick={() => { warnContext()?.onLaunch(); closeSetup("setup-launch-warning"); }}>Launch anyway</button>
          <button class="btn primary" autofocus onClick={() => closeSetup("setup-launch-warning")}>Cancel</button>
        </>}>
        <div class="notice warn"><Icon name="triangle-alert" size={14} />
          <span>The session host runs in another Windows session, for example over SSH. Agents it starts cannot show windows on this desktop, such as a browser sign-in.</span>
        </div>
      </SDialog>
    </>
  );
}

