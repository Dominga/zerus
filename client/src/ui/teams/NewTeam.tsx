import { For, Show, createEffect, createMemo, createResource, createSignal, onCleanup, onMount } from "solid-js";
import type { JSX } from "solid-js";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";
import { pickLocalFolder, projectMemory } from "../../bridge";
import { showTeam, teams } from "../../store/teams";
import { followDraft } from "../../store/newTeamFollow";
import { memberRowErrors, type ComputerChoice, type NewTeamStore } from "../../store/teamsNew";
import {
  FOLDER_POLICIES, applyPolicy, branchHint, canAdd, createReason, effectivePolicy, folderPolicy, leadMember, maxMembers, memberOf,
  memberRoles, memoryMode, pickTemplate, startReason, templateTasks, withMemory, type FolderPolicy, type LeadChoice, type MemoryMode,
} from "../../store/newTeamForm";
import type { Catalog } from "./catalog";
import type { DraftMember } from "./types";
import type { ComputerId } from "../../types";
import "../styles/newteam.css";

// New team (Qt TeamNewDialog/TeamNewTab, task 42). Opening it creates
// nothing; Start planning creates only the draft and its lead; Create team
// creates the members (and their worktrees). The owner's edits save to the
// draft shortly after the last one; the lead's `hgs team draft` edits show
// in the open form, except the fields the owner is changing.

export interface NewTeamProps {
  store: NewTeamStore;
  computer?: ComputerId;
  computers: ComputerChoice[];
  /** The folder is a Git repository (members may have worktrees). */
  git: boolean;
  /** The folder exists (the host checks it); default: a folder is typed. */
  folderOk?: boolean;
  catalog?: Catalog;
  /** The session New team was opened from, which may lead ("This session"). */
  coordinator?: { name: string; agent: string; model?: string; account?: string; busy?: boolean };
  /** This computer's agent accounts, for the lead and the member rows. */
  accounts?: { id: string; label: string; provider: string; signedIn: boolean }[];
  /** Project of the folder, sent with Start planning. */
  project?: string;
  projectId?: string;
  onOpenSettings?: () => void;
  onAskDesigner?: () => void;
  onCreated?: (team: string) => void;
  onDiscarded?: (team: string) => void;
}

const AGENTS = ["claude", "codex", "kimi", "dsh", "opencode"];
const EFFORTS = ["", "low", "medium", "high", "xhigh"];
const PERMISSIONS: [string, () => string][] = [["", () => tr("Account")], ["auto", () => tr("Auto")], ["bypass", () => tr("Bypass")]];

/** Discard draft's question (Qt teamNewDiscard): Cancel focused; it may also stop the lead's session. */
function DiscardDialog(props: { name: string; close: () => void; discard: (stopLead: boolean) => void }) {
  let dialog!: HTMLDialogElement;
  const [stopLead, setStopLead] = createSignal(false);
  onMount(() => { dialog.showModal(); queueMicrotask(() => dialog.querySelector<HTMLElement>("[data-initial]")?.focus()); });
  onCleanup(() => { if (dialog.open) dialog.close(); });
  return (
    <dialog ref={dialog} class="dlg" aria-labelledby="nt-discard-title" onCancel={(event) => { event.preventDefault(); props.close(); }}>
      <div class="dlg-head"><Icon name="triangle-alert" size={18} /><h2 id="nt-discard-title">{tr("Discard draft")}</h2></div>
      <div class="dlg-body">
        <p>{tr("Discard the draft “%1”? Nothing of it is created.", props.name || tr("New team"))}</p>
        <label class="nt-check"><input type="checkbox" checked={stopLead()} onChange={(event) => setStopLead(event.currentTarget.checked)} />{tr("Also stop the lead's session")}</label>
      </div>
      <div class="dlg-foot"><span class="spacer" />
        <button type="button" class="btn solid-danger" onClick={() => { props.discard(stopLead()); props.close(); }}>{tr("Discard draft")}</button>
        <button type="button" class="btn primary" data-initial autofocus onClick={props.close}>{tr("Cancel")}</button>
      </div>
    </dialog>
  );
}

export function NewTeam(props: NewTeamProps): JSX.Element {
  const store = props.store;
  const computer = () => props.computer ?? "";
  const fields = () => store.fields();
  const draft = () => store.draftId();
  const folderOk = () => props.folderOk ?? fields().folder.trim() !== "";
  const settings = () => fields().settings ?? {};
  const [template, setTemplate] = createSignal("-");
  const [planning, setPlanning] = createSignal("lead");
  const [showProtocol, setShowProtocol] = createSignal(false);
  const [addOpen, setAddOpen] = createSignal(false);
  const [discarding, setDiscarding] = createSignal(false);
  const [starting, setStarting] = createSignal(false);
  const leadRole = () => props.catalog?.roles.find((role) => role.id === "lead");
  const [lead, setLead] = createSignal<LeadChoice>({
    mode: props.coordinator?.name ? "this" : "new",
    session: props.coordinator?.name ? { name: props.coordinator.name, agent: props.coordinator.agent } : null,
    agent: leadRole()?.agent || "claude", model: leadRole()?.model ?? "", effort: "", account: "",
  });

  const policy = createMemo(() => effectivePolicy(folderPolicy(settings()), fields().folder, props.git));
  // Task 126: a project with RustCodeLore on gives the team a shared
  // workspace in its store; otherwise New team offers the memory mode,
  // RustCodeLore by default (written into the settings, so the draft has it).
  const [projectMemoryOn] = createResource(() => props.projectId || undefined, async (project) => {
    try { const outcome = await projectMemory({ project, op: { op: "read" } }); return outcome.outcome === "done" && outcome.view.enabled; }
    catch { return false; }
  });
  createEffect(() => { if (!draft() && !settings().memory) store.edit("settings", withMemory(settings(), "lore")); });
  const chooseMemory = (mode: MemoryMode) => store.edit("settings", withMemory(settings(), mode));
  const members = () => fields().members;
  const unseen = (field: string) => store.unseen().includes(field);
  const mark = (field: string) => (
    <Show when={unseen(field)}>
      <button type="button" class="nt-unseen" title={tr("Changed by the lead, not yet seen: click when you have seen it")} onClick={() => void store.markSeen(field)}>●</button>
    </Show>
  );

  // The lead's edits reach the open form; the owner's edits save shortly after the last one.
  followDraft({ computer, draft, show: showTeam, view: (c, id) => teams.newTeam(c, id), apply: store.applyView });
  let timer: ReturnType<typeof setTimeout> | undefined;
  createEffect(() => {
    if (!draft() || store.dirty().size === 0) return;
    clearTimeout(timer);
    timer = setTimeout(() => void store.flush(), 600);
  });
  onCleanup(() => clearTimeout(timer));

  const setSettings = (patch: Record<string, unknown>) => {
    const next = { ...settings(), ...patch };
    for (const [key, value] of Object.entries(patch)) if (value === undefined) delete next[key];
    store.edit("settings", next);
  };
  const setMembers = (next: DraftMember[]) => store.edit("members", next);
  const setMember = (index: number, patch: Partial<DraftMember>) => setMembers(members().map((member, at) => (at === index ? { ...member, ...patch } : member)));
  const choosePolicy = (value: FolderPolicy) => {
    store.editMany({ settings: { ...settings(), folder_policy: value }, members: applyPolicy(members(), value, props.git) });
  };
  const chooseTemplate = (id: string) => {
    setTemplate(id);
    const picked = pickTemplate(props.catalog, id, { settings: settings(), addendum: fields().addendum, addendumEdited: store.addendumEdited() },
      props.git && policy().policy !== "shared");
    setPlanning(picked.planning);
    store.editMany({ members: picked.members, settings: picked.settings, addendum: picked.addendum,
      tasks: templateTasks(props.catalog, id, picked.planning, picked.members) });
  };
  const choosePlanning = (value: string) => {
    setPlanning(value);
    store.edit("tasks", templateTasks(props.catalog, template(), value, members()));
  };
  const add = (roleId: string | null) => {
    setAddOpen(false);
    const role = roleId ? props.catalog?.roles.find((found) => found.id === roleId) : undefined;
    setMembers([...members(), memberOf(role, props.git && policy().policy !== "shared")]);
  };
  const browse = async () => {
    try {
      const picked = await pickLocalFolder(fields().folder);
      if (picked) store.setFolder(picked);
    } catch { /* the dialog is not available here: the field stays typed */ }
  };
  const start = async () => {
    if (startReason(fields().folder, folderOk(), lead())) return;
    setStarting(true);
    try {
      await store.startPlanning({
        computer: computer(), folder: fields().folder.trim(), name: fields().name.trim() || undefined,
        lead: leadMember(lead(), leadRole()?.prompt ?? ""), project: props.project, project_id: props.projectId,
        template: template() !== "-" && props.catalog?.templates.some((found) => found.id === template()) ? template() : undefined,
      });
    } finally { setStarting(false); }
  };
  const create = async () => {
    const id = draft();
    const result = await store.createTeam();
    if (result.ok) props.onCreated?.(id);
  };
  const discard = async (stopLead: boolean) => {
    const id = draft();
    const result = await store.discard(stopLead);
    if (result.ok) props.onDiscarded?.(id);
  };

  const badRows = () => members().filter((member) => memberRowErrors(member, props.computers).length > 0).length;
  const createWhy = () => createReason({ drafting: !!draft(), goal: fields().goal, name: fields().name, folderOk: folderOk(), badRows: badRows(),
    members: members().length, max: maxMembers(settings()), preflightErrors: store.view()?.validation.errors.length ?? 0 });
  const startWhy = () => startReason(fields().folder, folderOk(), lead());
  const accountsFor = (agent: string) => (props.accounts ?? []).filter((account) => account.provider === agent);

  return (
    <section class="nt" aria-label={tr("New team")}>
      <header class="nt-head">
        <h2>{tr("New team")}</h2>
        <span class="grow" />
        <Show when={props.onAskDesigner}>
          <button type="button" class="btn ghost small" title={tr("The Team designer keeps the templates and roles; the lead plans this team with you")}
            onClick={() => props.onAskDesigner?.()}>{tr("Templates and roles: Team designer")}</button>
        </Show>
        <Show when={draft() && props.onOpenSettings}>
          <button type="button" class="btn ghost small" onClick={() => props.onOpenSettings?.()}>{tr("Team settings")}</button>
        </Show>
        <button type="button" class="icon-btn small" aria-label={tr("Close")} onClick={store.close}><Icon name="x" size={14} /></button>
      </header>

      <div class="nt-body">
        <label class="nt-field">
          <span>{tr("Template")}</span>
          <select class="nt-template" value={template()} onChange={(event) => chooseTemplate(event.currentTarget.value)}>
            <For each={props.catalog?.templates ?? []}>{(found) => (
              <option value={found.id} title={found.description}>{found.roster.length ? tr("%1 (%2 members)", found.name, found.roster.reduce((n, seat) => n + Math.max(1, seat.count), 0)) : found.name}</option>
            )}</For>
            <option value="-">{tr("No template")}</option>
          </select>
        </label>
        <label class="nt-field">
          <span>{tr("Name")} {mark("name")}</span>
          <input value={fields().name} maxlength={80} placeholder={tr("Checkout redesign")} onInput={(event) => store.edit("name", event.currentTarget.value)} />
        </label>
        <div class="nt-field">
          <span>{tr("Folder")}</span>
          <div class="nt-inline">
            <input class="grow" value={fields().folder} disabled={!!draft()} onInput={(event) => store.setFolder(event.currentTarget.value)} />
            <button type="button" class="btn small" disabled={!!draft()} onClick={() => void browse()}>{tr("Browse…")}</button>
          </div>
        </div>
        <div class="nt-field">
          <span>{tr("Folders")} {mark("settings")}</span>
          <div class="nt-inline">
            <select class="nt-policy" value={policy().policy} disabled={policy().forced} onChange={(event) => choosePolicy(event.currentTarget.value as FolderPolicy)}
              title={policy().forced ? tr("Folder is not a Git repository: every member works in this folder, so the policy is shared.") : tr("Auto lets the lead choose the folder each member works in")}>
              <For each={FOLDER_POLICIES}>{(choice) => <option value={choice.value}>{choice.label()}</option>}</For>
            </select>
            <span class="nt-hint">{branchHint(fields().name, props.git)}</span>
          </div>
        </div>
        <Show when={fields().folder.trim() && !props.git}>
          <p class="nt-note">{tr("This folder is not a Git repository. Every member will share it; worktrees are off.")}</p>
        </Show>

        <div class="nt-members">
          <div class="nt-members-head">
            <b>{tr("Members")}</b>
            <span class="nt-hint" title={tr("The lead counts as one member")}>{tr("%1 / %2", members().length, maxMembers(settings()))}</span>
            {mark("members")}
            <span class="grow" />
            <div class="nt-add">
              <button type="button" class="btn small" aria-haspopup="menu" aria-expanded={addOpen()} disabled={!canAdd(members(), settings())}
                onClick={() => setAddOpen(!addOpen())}>{tr("+ Member")}</button>
              <Show when={addOpen()}>
                <div class="menu nt-add-menu" role="menu" aria-label={tr("Add a member")}>
                  <For each={memberRoles(props.catalog)}>{(role) => (
                    <button type="button" role="menuitem" class="menu-item" onClick={() => add(role.id)}>
                      {role.model ? tr("%1, %2, %3", role.name, role.agent, role.model) : tr("%1, %2", role.name, role.agent)}
                    </button>
                  )}</For>
                  <button type="button" role="menuitem" class="menu-item" onClick={() => add(null)}>{tr("Member without a role")}</button>
                </div>
              </Show>
            </div>
          </div>
          <Show when={members().length === 0}>
            <p class="nt-hint">{draft() ? tr("No members yet. The lead drafts them with you, or add them by role.") : tr("No members yet. Pick a template, add members by role, or let the lead draft them.")}</p>
          </Show>
          <For each={members()}>{(member, index) => (
            <div class="nt-row nt-member">
              <input class="nt-role" placeholder={tr("Role")} value={member.role} onInput={(event) => setMember(index(), { role: event.currentTarget.value })} />
              <select class="nt-agent" value={member.agent} onChange={(event) => setMember(index(), { agent: event.currentTarget.value, account: "" })}>
                <For each={AGENTS}>{(agent) => <option value={agent}>{agent}</option>}</For>
              </select>
              <input class="nt-model" placeholder={tr("default model")} value={member.model} onInput={(event) => setMember(index(), { model: event.currentTarget.value })} />
              <select class="nt-effort" value={member.effort} aria-label={tr("Effort")} onChange={(event) => setMember(index(), { effort: event.currentTarget.value })}>
                <For each={EFFORTS}>{(effort) => <option value={effort}>{effort || tr("Default effort")}</option>}</For>
              </select>
              <select class="nt-permissions" value={member.permissions} aria-label={tr("Permissions")} onChange={(event) => setMember(index(), { permissions: event.currentTarget.value })}>
                <For each={PERMISSIONS}>{([value, label]) => <option value={value}>{label()}</option>}</For>
              </select>
              <select class="nt-computer" value={member.computer} aria-label={tr("Computer")} onChange={(event) => setMember(index(), { computer: event.currentTarget.value })}>
                <option value="">{tr("this computer")}</option>
                <For each={props.computers.filter((choice) => choice.alias)}>{(choice) => <option value={choice.alias}>{choice.label}</option>}</For>
              </select>
              <select class="nt-account" value={member.account} aria-label={tr("Account")} onChange={(event) => setMember(index(), { account: event.currentTarget.value })}>
                <option value="">{tr("Default account")}</option>
                <For each={accountsFor(member.agent)}>{(account) => <option value={account.id}>{account.signedIn ? account.label || account.id : tr("%1 (signed out)", account.label || account.id)}</option>}</For>
              </select>
              <label class="nt-check" title={tr("Own worktree")}>
                <input type="checkbox" checked={member.folder === "worktree"} disabled={!props.git || policy().policy === "shared"}
                  onChange={(event) => setMember(index(), { folder: event.currentTarget.checked ? "worktree" : "shared" })} />{tr("Own worktree")}
              </label>
              <button type="button" class="icon-btn small" aria-label={tr("Duplicate")} title={tr("Duplicate")} disabled={!canAdd(members(), settings())}
                onClick={() => setMembers([...members().slice(0, index() + 1), { ...member, id: "" }, ...members().slice(index() + 1)])}><Icon name="copy" size={13} /></button>
              <button type="button" class="icon-btn small" aria-label={tr("Remove member")} title={tr("Remove member")}
                onClick={() => setMembers(members().filter((_, at) => at !== index()))}><Icon name="x" size={13} /></button>
              <Show when={memberRowErrors(member, props.computers).length > 0}>
                <span class="nt-error">{memberRowErrors(member, props.computers).map((error) => (error.code === "role" ? tr("Pick a role") : tr("Computer %1 is offline", error.alias))).join(", ")}</span>
              </Show>
            </div>
          )}</For>
        </div>

        <div class="nt-options">
          <div class="nt-field">
            <span>{tr("Planning")}</span>
            <div class="nt-inline" role="radiogroup" aria-label={tr("Planning")}>
              <label class="nt-check"><input type="radio" name="nt-planning" checked={planning() === "lead"} onChange={() => choosePlanning("lead")} />{tr("Lead plans")}</label>
              <label class="nt-check"><input type="radio" name="nt-planning" checked={planning() === "template"} onChange={() => choosePlanning("template")} />{tr("Use template tasks")}</label>
            </div>
          </div>
          <div class="nt-field">
            <span>{tr("Approval")}</span>
            <div class="nt-inline">
              <label class="nt-check"><input type="checkbox" checked={settings().approval !== "auto"} onChange={(event) => setSettings({ approval: event.currentTarget.checked ? "review" : "auto" })} />{tr("Review plan before running")}</label>
              <select value={typeof settings().phase_approval === "string" ? String(settings().phase_approval) : ""} onChange={(event) => setSettings({ phase_approval: event.currentTarget.value || undefined })}>
                <option value="">{tr("Later phases: as the plan")}</option>
                <option value="review">{tr("Later phases: review")}</option>
                <option value="auto">{tr("Later phases: auto")}</option>
              </select>
            </div>
          </div>
          <div class="nt-field">
            <span>{tr("Memory")}</span>
            <Show when={!projectMemoryOn()} fallback={<span class="nt-hint">{tr("RustCodeLore: a shared workspace in the project's store")}</span>}>
              <div class="nt-inline" role="radiogroup" aria-label={tr("Memory")}>
                <label class="nt-check"><input type="radio" name="nt-memory" checked={memoryMode(settings()) === "lore"} onChange={() => chooseMemory("lore")} />{tr("RustCodeLore")}</label>
                <label class="nt-check"><input type="radio" name="nt-memory" checked={memoryMode(settings()) === "plain"} onChange={() => chooseMemory("plain")} />{tr("Plain memory")}</label>
              </div>
            </Show>
          </div>
          <label class="nt-field">
            <span>{tr("Max members")}</span>
            <input type="number" min={1} max={32} class="nt-max" value={maxMembers(settings())}
              onInput={(event) => { const n = Math.round(Number(event.currentTarget.value)); if (n >= 1 && n <= 32) setSettings({ max_members: n }); }} />
          </label>
          <button type="button" class="btn ghost small" aria-expanded={showProtocol()} onClick={() => setShowProtocol(!showProtocol())}>{showProtocol() ? tr("Hide protocol addendum") : tr("Protocol addendum")}</button> {mark("addendum")}
          <Show when={showProtocol()}>
            <textarea rows={4} placeholder={tr("How the lead splits the work, checks reports and starts the next wave. A template fills this in.")}
              value={fields().addendum} onInput={(event) => store.edit("addendum", event.currentTarget.value)} />
          </Show>
        </div>

        <label class="nt-field nt-goal">
          <span>{tr("Goal")} {mark("goal")}</span>
          <textarea rows={4} placeholder={tr("Describe the result, constraints and what done means. The lead may write it with you.")}
            value={fields().goal} onInput={(event) => store.edit("goal", event.currentTarget.value)} />
        </label>

        <Show when={!draft()}>
          <fieldset class="nt-lead">
            <legend>{tr("Who leads the team?")}</legend>
            <label class="nt-check" title={props.coordinator?.name ?? ""}>
              <input type="radio" name="nt-lead" disabled={!props.coordinator?.name} checked={lead().mode === "this"} onChange={() => setLead({ ...lead(), mode: "this" })} />
              {tr("This session")}<Show when={props.coordinator?.name}><span class="nt-hint">{props.coordinator!.name}</span></Show>
            </label>
            <Show when={lead().mode === "this" && props.coordinator?.busy}>
              <p class="nt-hint">{tr("This session is working. The draft's brief waits until it is free; it is queued once, never sent twice.")}</p>
            </Show>
            <label class="nt-check"><input type="radio" name="nt-lead" checked={lead().mode === "new"} onChange={() => setLead({ ...lead(), mode: "new" })} />{tr("A new session in the team folder")}</label>
            <div class="nt-row nt-lead-row">
              <select value={lead().agent} disabled={lead().mode !== "new"} aria-label={tr("Agent")} onChange={(event) => setLead({ ...lead(), agent: event.currentTarget.value, account: "" })}>
                <For each={AGENTS}>{(agent) => <option value={agent}>{agent}</option>}</For>
              </select>
              <input placeholder={tr("default model")} value={lead().model} disabled={lead().mode !== "new"} onInput={(event) => setLead({ ...lead(), model: event.currentTarget.value })} />
              <select value={lead().effort} disabled={lead().mode !== "new"} aria-label={tr("Effort")} onChange={(event) => setLead({ ...lead(), effort: event.currentTarget.value })}>
                <For each={EFFORTS}>{(effort) => <option value={effort}>{effort || tr("Default effort")}</option>}</For>
              </select>
              <select value={lead().account} disabled={lead().mode !== "new"} aria-label={tr("Account")} onChange={(event) => setLead({ ...lead(), account: event.currentTarget.value })}>
                <option value="">{tr("Default account")}</option>
                <For each={accountsFor(lead().agent)}>{(account) => <option value={account.id}>{account.signedIn ? account.label || account.id : tr("%1 (signed out)", account.label || account.id)}</option>}</For>
              </select>
            </div>
            <p class="nt-hint">{tr("Nothing is created until you press it. Start planning makes a draft and gives the lead its brief; worktrees and members come only with Create team.")}</p>
          </fieldset>
        </Show>

        <Show when={store.error()}><p class="nt-error nt-block-error">{store.error()}</p></Show>
        <Show when={store.created()}><p class="nt-created">{tr("Team created.")}</p></Show>
      </div>

      <footer class="nt-foot">
        <span class="nt-hint grow">{draft() ? (store.dirty().size ? tr("Draft, unsaved changes") : tr("Draft saved, revision %1", store.revision())) : tr("Nothing is created until you press Start planning.")}</span>
        <Show when={draft()}>
          <Show when={store.view()}><span class="nt-hint">{(store.view()!.validation.errors.length ? tr("Preflight: %1 errors", store.view()!.validation.errors.length) : tr("Preflight: ok"))}</span></Show>
          <Show when={store.dirty().size > 0}>
            <button type="button" class="btn small ghost" title={tr("Drop your unsaved changes and show the draft as the lead last saved it")} onClick={store.useLeadVersion}>{tr("Show the lead's version")}</button>
          </Show>
          <button type="button" class="btn small ghost danger" disabled={store.busy()} onClick={() => setDiscarding(true)}>{tr("Discard draft")}</button>
          <button type="button" class="btn small primary" disabled={store.busy() || !!createWhy()} title={createWhy()} onClick={() => void create()}>{tr("Create team")}</button>
        </Show>
        <Show when={!draft()}>
          <button type="button" class="btn small primary" disabled={store.busy() || starting() || !!startWhy()} title={startWhy()} onClick={() => void start()}>{tr("Start planning")}</button>
        </Show>
      </footer>
      <Show when={discarding()}>
        <DiscardDialog name={fields().name.trim()} close={() => setDiscarding(false)} discard={(stopLead) => void discard(stopLead)} />
      </Show>
    </section>
  );
}
