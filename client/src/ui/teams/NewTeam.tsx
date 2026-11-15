import { For, Show, createEffect, createMemo, createSignal, createUniqueId, on, onCleanup, onMount } from "solid-js";
import type { JSX } from "solid-js";
import { tr } from "../../i18n";
import { selected, setPage } from "../../store/ui";
import { machineCards } from "../../store/machines";
import {
  teams, type TeamCall, type TeamCatalog, type TeamCatalogTemplate, type TeamProjectFacts,
} from "../../store/teams";
import { NewTeamFooter } from "./NewTeamFooter";
import { withoutCode } from "../../store/sentences";
import { NewTeamHowTo, NewTeamIntroSteps } from "./NewTeamGuide";
import { NewTeamPage } from "./NewTeamPage";
import { TextInput } from "../lib/TextField";
import { Icon } from "../lib/icons";
import "../styles/newteam.css";

// New team (Qt's NewTeamWindow, the fork's form adapted to M1): a dialog from a
// catalog template. It shows the roster, the pipelines and the settings the
// person confirms; the folder is checked as a Git folder before anything can be
// created, and Create team is the only thing that creates anything. The fork's
// drafts, plan modes, working modes, accounts and member editing are M2 and are
// not here.

/** The command line's code for a create whose template changed since it was shown. */
export const TEMPLATE_CHANGED = "template_changed";

/** A URL-safe template id from a name. */
export function slug(text: string): string {
  return text.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 60) || "team";
}

/** The settings a person confirms, as the template's own values. Phases and
 * their approval belong to milestone 2 and are not shown. */
export function settingLines(template: TeamCatalogTemplate): [string, string][] {
  const settings = template.settings;
  return [
    [tr("Integration"), settings.integration === "branch" ? tr("an integration branch") : tr("no integration branch")],
    [tr("Queue"), settings.queue ? tr("the team takes tasks as they come") : tr("no queue")],
    [tr("Routing"), settings.routing === "direct" ? tr("straight to a free member, without the lead") : tr("the lead routes first")],
    [tr("Land stage"), settings.land_allowed ? tr("allowed") : tr("not allowed")],
  ];
}

/** A delegation in words: its name and what it lets the lead or the service
 * do without the person. The names and values come from the template (the
 * contract's Delegations); one this version does not know shows its plain
 * name. */
export function delegationWords(key: string): { name: string; sentence: string } {
  switch (key) {
    case "accept_results": return { name: tr("Accept results"), sentence: tr("The lead may accept a team task's result without you.") };
    case "answer_members": return { name: tr("Answer members"), sentence: tr("The lead answers members' questions and permission requests without you.") };
    case "confirm_trust": return { name: tr("Confirm folder trust"), sentence: tr("Zerus confirms the trust prompt of a folder it created for the team; on the member's account the agent records the trust for the team repository's main checkout and its subfolders.") };
    case "resume_exited": return { name: tr("Resume exited members"), sentence: tr("Zerus resumes a member whose agent exited by itself.") };
    case "merge_accepted": return { name: tr("Merge accepted work"), sentence: tr("An accepted result is merged into the integration branch without your approval.") };
    default: return { name: key, sentence: tr("A permission this version does not describe.") };
  }
}

/** Who ends a pipeline: the lead, the person, or the pipeline by itself. */
export function acceptanceWords(accept: string): string {
  switch (accept) {
    case "lead": return tr("the lead accepts");
    case "person": return tr("you accept");
    case "pipeline": return tr("it accepts by itself");
    default: return accept;
  }
}

export function rosterLines(catalog: TeamCatalog, template: TeamCatalogTemplate): string[] {
  return template.roster.map((seat) => {
    const role = catalog.roles.find((candidate) => candidate.id === seat.role);
    const name = role?.name ? tr(role.name) : seat.role;
    return tr("%1 × %2%3", name, seat.count, seat.optional ? tr(" (optional)") : "");
  });
}

/** The pipelines the template resolves to (its default and its seats' roles'
 * defaults) in the catalog's order, its default marked. */
export function pipelineLines(catalog: TeamCatalog, template: TeamCatalogTemplate): string[] {
  const used = new Set<string>();
  if (template.default_pipeline) used.add(template.default_pipeline);
  for (const seat of template.roster) {
    if (seat.count === 0) continue;
    const role = catalog.roles.find((candidate) => candidate.id === seat.role);
    if (role?.pipeline) used.add(role.pipeline);
  }
  return catalog.pipelines
    .filter((pipeline) => used.has(pipeline.id))
    .map((pipeline) => {
      const stages = pipeline.stages.map((stage) => `${tr(stage.kind)}: ${stage.role}`).join(" → ");
      const mark = pipeline.id === template.default_pipeline ? ` (${tr("default")})` : "";
      return `${pipeline.name || pipeline.id}${mark}${stages ? ` — ${stages}` : ""} — ${acceptanceWords(pipeline.accept)}`;
    });
}

/** The permissions a create carries: the contract's five, each sent as shown. */
export const CREATE_DELEGATIONS = ["accept_results", "answer_members", "confirm_trust", "resume_exited", "merge_accepted"] as const;

export interface NewTeamInput {
  /** The new team's id and the create intent's request id, made here once. */
  team: string;
  request_id: string;
  template: string;
  /** The digest of the template the form showed, as the catalog read gave it. */
  expected_template: string;
  /** The five permissions exactly as the checkboxes show them. */
  delegations: Record<(typeof CREATE_DELEGATIONS)[number], boolean>;
  project: string;
  name: string;
  goal: string;
  home: string;
  integration_branch: string;
  base_commit: string;
  integration: string;
}

export interface NewTeamProps {
  catalog?: TeamCatalog;
  catalogError?: string;
  busy: boolean;
  machineId: string;
  defaultProject: string;
  defaultFolder: string;
  onCheckFolder: (path: string) => Promise<TeamProjectFacts>;
  onCreate: (input: NewTeamInput) => Promise<import("../../store/teams").TeamOutcome>;
  onSaveTemplate: (template: TeamCatalogTemplate) => Promise<void>;
  /** Reads the catalog again, after the command line said the template changed. */
  onReloadCatalog?: () => Promise<unknown>;
  onClose: () => void;
  onOpenCatalog?: () => void;
  /** This dialog's refusal kept by the store: one that arrived while it was
   * closed, or a Dismiss the core refused. */
  failure?: string;
}

/** The form alone, from a catalog: the render tests pass their own. */
export function NewTeam(props: NewTeamProps): JSX.Element {
  const templateGroup = createUniqueId();
  const [templateId, setTemplateId] = createSignal(props.catalog?.templates[0]?.id ?? "");
  const [name, setName] = createSignal("");
  const [goal, setGoal] = createSignal("");
  const [project, setProject] = createSignal(props.defaultProject);
  const [folder, setFolder] = createSignal(props.defaultFolder);
  const [branch, setBranch] = createSignal("");
  const [facts, setFacts] = createSignal<{ path: string; generation: number; facts: TeamProjectFacts }>();
  const [checking, setChecking] = createSignal(false);
  const [error, setError] = createSignal("");
  const [creating, setCreating] = createSignal(false);
  let checkGeneration = 0;
  // The one create intent lives in the store, so closing the dialog keeps it
  // and a reopened dialog adopts it, also one that becomes unresolved after
  // the reopen. Only the store settles it: done, a refusal, or a Dismiss the
  // core took. A Dismiss that fails or is refused changes nothing.
  const intent = () => teams.createIntent();
  /** The unresolved call of the create intent, when it has one. */
  const intentCall = () => {
    const current = intent();
    return current ? teams.unresolved().find((entry) => entry.call.request_id === current.request_id) : undefined;
  };
  /** The create intent's invoke is in flight (perhaps from a closed dialog). */
  const sendingCreate = () => {
    const current = intent();
    return !!current && teams.sending().includes(current.request_id);
  };
  const locked = () => props.busy || creating() || sendingCreate() || !!intentCall();
  const checkedFolder = () => {
    const checked = facts();
    return checked && checked.path === folder().trim() ? checked.facts : undefined;
  };
  const [saving, setSaving] = createSignal(false);
  const [saved, setSaved] = createSignal("");
  const [howto, setHowto] = createSignal(false);
  const template = createMemo(() => props.catalog?.templates.find((candidate) => candidate.id === templateId()));
  const proposed = createMemo(() => branch() || `zerus/team/${slug(name() || template()?.name || "team")}`);
  // The permissions as the person sets them here: the shown template's own
  // values with the person's choices over them. A choice is kept while its
  // permission exists in the template as it is now, also after the catalog is
  // read again; choosing another template starts from that template's values.
  // The create carries exactly these, with the digest of the template shown.
  const [choices, setChoices] = createSignal<Record<string, boolean>>({});
  createEffect(on(templateId, () => setChoices({}), { defer: true }));
  const delegations = createMemo<Record<string, boolean>>(() => {
    const given = template()?.settings.delegations ?? {};
    return { ...given, ...Object.fromEntries(Object.entries(choices()).filter(([key]) => key in given)) };
  });
  /** The five permissions to send, or undefined when the shown ones are not exactly the five. */
  const sent = createMemo(() => {
    const shown = delegations();
    const keys = Object.keys(shown);
    if (keys.length !== CREATE_DELEGATIONS.length || !CREATE_DELEGATIONS.every((key) => typeof shown[key] === "boolean")) return undefined;
    return Object.fromEntries(CREATE_DELEGATIONS.map((key) => [key, shown[key]])) as NewTeamInput["delegations"];
  });
  /** The row holds what it asks to confirm and this version does not know:
   * it can be neither created nor saved, so nothing it holds is dropped. */
  const unknownConfirmed = () => !!(template()?.unknown_permissions?.length || template()?.unknown_fields?.length);
  /** The command line said the template changed: it is shown as it is now. */
  const [changed, setChanged] = createSignal(false);
  const createWhy = createMemo(() => {
    if (!template()) return tr("Choose a template.");
    if (!template()!.digest) return tr("This hgs cannot confirm the template you see. Update hgs to create a team.");
    // What the person is asked to confirm and this version does not know is a
    // fact of the row (the core keeps it): nothing is created from a template
    // this window cannot show whole.
    if (template()!.unknown_permissions?.length || !sent()) return tr("This version does not know every permission of the template. Update Zerus to create a team.");
    if (template()!.unknown_fields?.length) return tr("This version cannot show everything the template sets. Update Zerus to create a team.");
    if (!name().trim()) return tr("Name the team.");
    if (!project().trim()) return tr("Name the project.");
    if (!checkedFolder()?.git) return tr("Check the project folder: it must be a Git folder.");
    if (locked()) return tr("Wait for the team to be created.");
    return "";
  });

  // A check belongs to the exact path it asked for: an answer for another path
  // (or an older generation) never writes the facts Create reads.
  const check = async () => {
    const path = folder().trim();
    const generation = ++checkGeneration;
    setChecking(true); setError("");
    try {
      const answer = await props.onCheckFolder(path);
      if (generation !== checkGeneration || folder().trim() !== path) return;
      setFacts({ path, generation, facts: answer });
    } catch (failure) {
      if (generation !== checkGeneration || folder().trim() !== path) return;
      setFacts(undefined); setError(String(failure));
    } finally {
      if (generation === checkGeneration) setChecking(false);
    }
  };
  /** A refusal of the create: a changed template is shown again as it is now,
   * never sent again by itself; any other refusal keeps its reason. */
  const refused = (outcome: Extract<import("../../store/teams").TeamOutcome, { outcome: "failed" }>) => {
    if (outcome.code !== TEMPLATE_CHANGED) { setError(outcome.detail); return; }
    // The form says it in its own words; the store's copy of the refusal goes.
    teams.clearFailure("new-team");
    setChanged(true);
    void props.onReloadCatalog?.();
  };
  const create = async () => {
    const chosen = template();
    const checked = checkedFolder();
    const permissions = sent();
    if (!chosen?.digest || !permissions || !checked?.git || createWhy() || locked()) return;
    // The held intent when there is one: never a second team id.
    const current = teams.claimCreate();
    setCreating(true);
    setError("");
    setChanged(false);
    teams.clearFailure("new-team");
    try {
      const outcome = await props.onCreate({
        team: current.team, request_id: current.request_id, template: chosen.id, expected_template: chosen.digest, delegations: permissions,
        project: project().trim(), name: name().trim(), goal: goal().trim(),
        home: props.machineId, integration_branch: proposed(), base_commit: checked.head, integration: chosen.settings.integration,
      });
      if (outcome.outcome === "failed") refused(outcome);
      // uncertain and pending keep the intent; the store's unresolved entry
      // offers Check and Dismiss until it ends.
    } catch (failure) {
      // A lost reply is the store's uncertain call, not a refusal.
      setError(String(failure));
    } finally { setCreating(false); }
  };
  const checkCreate = async () => {
    const entry = intentCall();
    if (!entry) return;
    setError("");
    const outcome = await teams.runAgain(entry.call, "new-team");
    if (outcome.outcome === "done") props.onClose();
    else if (outcome.outcome === "failed") refused(outcome);
  };
  const dismissCreate = async () => {
    const entry = intentCall();
    if (entry) await teams.dismiss(entry, "new-team");
  };
  // A refusal's sentence, without the code hgs puts before it.
  const shownError = () => withoutCode(error() || props.failure || "");
  const save = async () => {
    const chosen = template();
    if (!chosen) return;
    setSaving(true); setError("");
    try {
      // The template's own fields only: a shown row also carries its digest and
      // facts of the catalog read, which a template save (strict) refuses.
      const saving: TeamCatalogTemplate = {
        id: slug(`${chosen.id}-${name() || chosen.name}`), name: name().trim() || chosen.name, description: chosen.description,
        lead: chosen.lead, roster: chosen.roster, default_pipeline: chosen.default_pipeline,
        settings: { ...chosen.settings, delegations: { ...delegations() } },
      };
      await props.onSaveTemplate(saving);
      setSaved(tr("Template saved."));
      // The saved template carries the permissions shown: create from it.
      if (props.catalog?.templates.some((candidate) => candidate.id === saving.id)) setTemplateId(saving.id);
    } catch (failure) { setError(String(failure)); }
    finally { setSaving(false); }
  };

  return (
    <div class="nt-body" data-new-team-form>
      <Show when={props.catalogError}><div class="notice danger nt-error" role="alert"><Icon name="triangle-alert" size={16} /><span>{withoutCode(props.catalogError!)}</span></div></Show>
      <fieldset class="nt-templates">
        <legend>{tr("Template")}</legend>
        <Show when={props.onOpenCatalog}><button type="button" class="btn ghost small" disabled={locked()} onClick={() => props.onOpenCatalog?.()}>{tr("Catalog")}</button></Show>
        <div class="nt-template-list">
          <For each={props.catalog?.templates ?? []}>
            {(candidate) => (
              <label class="nt-template-row" classList={{ on: templateId() === candidate.id }} data-template={candidate.id}>
                <input type="radio" name={templateGroup} value={candidate.id} checked={templateId() === candidate.id} disabled={locked()} onChange={() => { if (!locked()) setTemplateId(candidate.id); }} />
                <span class="nt-template-copy"><span class="nt-template-name">{tr(candidate.name || candidate.id)}</span><span class="nt-hint">{tr(candidate.description)}</span></span>
              </label>
            )}
          </For>
        </div>
      </fieldset>
      <section class="nt-section" aria-label={tr("Team")}>
        <h2>{tr("Team")}</h2>
        <div class="nt-pair">
          <label class="nt-field">{tr("Team name")}
            <TextInput class="field" value={name()} disabled={locked()} onInput={(event) => setName(event.currentTarget.value)} placeholder={tr("Fixes")} historyKey="team-name" />
          </label>
          <label class="nt-field">{tr("Goal")}
            <TextInput class="field" value={goal()} disabled={locked()} onInput={(event) => setGoal(event.currentTarget.value)} placeholder={tr("What should the team work on?")} historyKey="team-goal" />
          </label>
        </div>
        <label class="nt-field">{tr("Project")}
          <TextInput class="field" value={project()} disabled={locked()} onInput={(event) => setProject(event.currentTarget.value)} historyKey="team-project" />
        </label>
      </section>
      <Show when={template()}>
        {(chosen) => (
          <section class="nt-section" aria-label={tr("Settings")}>
            <h2>{tr("Settings")}</h2>
            <div class="box nt-template-summary">
              <div class="nt-pair">
                <section class="nt-field" aria-label={tr("Roster")}>
                  <b>{tr("Roster")}</b>
                  <ul class="nt-list"><For each={rosterLines(props.catalog!, chosen())}>{(line) => <li>{line}</li>}</For></ul>
                </section>
                <section class="nt-field" aria-label={tr("Pipelines")}>
                  <b>{tr("Pipelines")}</b>
                  <ul class="nt-list"><For each={pipelineLines(props.catalog!, chosen())}>{(line) => <li>{line}</li>}</For></ul>
                </section>
              </div>
              <dl class="nt-caps"><For each={settingLines(chosen())}>{([label, value]) => <><dt>{label}</dt><dd>{value}</dd></>}</For></dl>
              <Show when={Object.keys(delegations()).length}>
                <fieldset class="nt-permissions" data-permissions>
                  <legend>{tr("Permissions")}</legend>
                  <span class="nt-hint">{tr("What the lead and the service may do without you.")}</span>
                  <For each={Object.keys(delegations())}>
                    {(key) => (
                      <label class="nt-template-row" classList={{ on: delegations()[key] }} data-delegation={key}>
                        <input type="checkbox" checked={delegations()[key]} disabled={locked()}
                          onChange={(event) => setChoices({ ...choices(), [key]: event.currentTarget.checked })} />
                        <span class="nt-template-copy"><span class="nt-template-name">{delegationWords(key).name}</span><span class="nt-hint">{delegationWords(key).sentence}</span></span>
                      </label>
                    )}
                  </For>
                </fieldset>
              </Show>
            </div>
          </section>
        )}
      </Show>
      <section class="nt-section" aria-label={tr("Project folder")}>
        <h2>{tr("Project folder")}</h2>
        <div class="nt-inline">
          <TextInput class="field mono" aria-label={tr("Project folder")} value={folder()} disabled={locked()} onInput={(event) => { setFolder(event.currentTarget.value); setFacts(undefined); }} historyKey="team-folder" />
          <button type="button" class="btn" data-check-folder disabled={checking() || locked() || !folder().trim()} onClick={() => void check()}>{tr("Check folder")}</button>
        </div>
        <Show when={checkedFolder()}>
          {(checked) => checked().git
            ? <span class="nt-ok" role="status" data-folder-ok>{tr("A Git folder at %1, base commit %2.", checked().branch || tr("detached"), checked().head.slice(0, 12))}</span>
            : <span class="nt-foot-bad" role="status" data-folder-bad>{tr("Not a Git folder: %1", checked().reason)}</span>}
        </Show>
        <label class="nt-field">{tr("Integration branch")}
          <TextInput class="field mono" value={proposed()} onInput={(event) => setBranch(event.currentTarget.value)}
            disabled={locked() || template()?.settings.integration !== "branch"} historyKey="team-branch" />
        </label>
      </section>
      <Show when={changed()}>
        <div class="notice warn nt-error" role="status" data-template-changed><Icon name="triangle-alert" size={16} /><span>{tr("The template changed since you saw it, and nothing was created: review it as it is now, then create the team again.")}</span></div>
      </Show>
      <Show when={shownError()}><div class="notice danger nt-error" role="alert" data-new-team-error><Icon name="triangle-alert" size={16} /><span>{shownError()}</span></div></Show>
      <Show when={sendingCreate() && !intentCall()}>
        <div class="notice nt-note" role="status" data-create-sending><Icon name="loader-circle" size={16} /><span>{tr("The team is being created.")}</span></div>
      </Show>
      <Show when={intentCall()}>
        {(entry) => (
          <div class="notice danger nt-error" role="alert" data-create-uncertain>
            <Icon name="triangle-alert" size={16} />
            <span>{entry().state === "pending" ? tr("The team is being created and waits for the core.") : tr("The create's result is unknown. Check it with the same request, or dismiss it.")}</span>
            <span class="nt-inline">
              <button type="button" class="btn small" disabled={creating() || sendingCreate()} onClick={() => void checkCreate()}>{tr("Check")}</button>
              <button type="button" class="btn small" disabled={creating() || sendingCreate()} onClick={() => void dismissCreate()}>{tr("Dismiss")}</button>
            </span>
          </div>
        )}
      </Show>
      <Show when={saved()}><div class="notice nt-note" role="status"><Icon name="check" size={16} /><span>{saved()}</span></div></Show>
      <section class="nt-section">
        <NewTeamIntroSteps />
        <NewTeamHowTo open={howto()} onToggle={setHowto} />
      </section>
      <NewTeamFooter busy={props.busy || creating() || sendingCreate()} createWhy={createWhy()} error={shownError()} canSave={!!template() && !locked() && !unknownConfirmed()}
        saving={saving()} showCreate={!intentCall() && !sendingCreate()} onSaveTemplate={() => void save()} onCancel={props.onClose} onCreate={() => void create()} />
    </div>
  );
}

/** The dialog the rail and the command table open; the catalog is read on open. */
export function NewTeamDialog() {
  let dialog: HTMLDialogElement | undefined;
  const origin = document.activeElement instanceof HTMLElement ? document.activeElement : undefined;
  const open = createMemo(() => teams.newTeamOpen());
  onMount(() => {
    if (open()) {
      dialog?.showModal();
      queueMicrotask(() => dialog?.querySelector<HTMLInputElement>('input[type="radio"]:checked')?.focus());
    }
  });
  onCleanup(() => { if (dialog?.open) dialog.close(); if (origin?.isConnected) origin.focus(); });
  const close = () => teams.closeNewTeam();
  return (
    <Show when={open()}>
      <dialog ref={dialog} class="dlg newteam-page-dialog" aria-labelledby="new-team-title"
        onCancel={(event) => { event.preventDefault(); close(); }}>
        <NewTeamPage onClose={close}>
          <NewTeam
            catalog={teams.catalog()}
            catalogError={teams.catalogError()}
            busy={teams.busy()}
            machineId={machineCards[""]?.machine_id ?? ""}
            defaultProject={selected()?.project ?? ""}
            defaultFolder={selected()?.gitRoot || selected()?.cwdPath || ""}
            onCheckFolder={(path) => teams.project(path)}
            onCreate={async (input) => {
              const call: TeamCall = {
                team: input.team, team_name: input.name, request_id: input.request_id,
                action: {
                  kind: "create", template: input.template, expected_template: input.expected_template, delegations: input.delegations,
                  project: input.project, name: input.name, goal: input.goal,
                  home: input.home, integration_branch: input.integration_branch, base_commit: input.base_commit,
                  integration: input.integration,
                },
              };
              const outcome = await teams.run(call, "new-team");
              if (outcome.outcome === "done") close();
              return outcome;
            }}
            onOpenCatalog={() => { close(); setPage("team-catalog"); }}
            onSaveTemplate={(template) => teams.saveTemplate(template)}
            onReloadCatalog={() => teams.reloadCatalog()}
            onClose={close}
            failure={teams.failureFor("new-team")?.message}
          />
        </NewTeamPage>
      </dialog>
    </Show>
  );
}
