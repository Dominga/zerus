import { For, Show, createEffect, createSignal, onMount } from "solid-js";
import type { JSX } from "solid-js";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";
import { teamAction, teamRoleModels } from "../../bridge";
import { createModelCatalogs, effortOptions, modelOptions } from "./catalogControls";
import { normalizeModelCatalog, selectModel } from "../provider-selection";
import type { TeamMemberView, TeamView } from "../../types";
import { checkTestConfig, overrideText, parseOverride, readTestConfig, sourceLine, testsLines, testsPatch, type TestConfig } from "./testsSettings";
import "../styles/newteam.css";
import { TextArea, TextInput } from "../lib/TextField";

type TeamSettings = Record<string, unknown>;

/** Qt TeamSettingsDialog: the owner's iron rules, branches, landing, restart,
 * the build broker, phases and each member's iron rules, one `settings` call. */
export interface TeamSettingsDialogProps {
  team: TeamView;
  onClose: () => void;
  onSaved?: () => void;
}

function lines(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((line): line is string => typeof line === "string") : [];
}

function text(value: unknown): string {
  return typeof value === "string" ? value : "";
}

export function TeamSettingsDialog(props: TeamSettingsDialogProps): JSX.Element {
  const current = props.team;
  const settings = current.settings as TeamSettings;
  const [rules, setRules] = createSignal(lines(settings.iron_rules).join("\n"));
  const [memberRules, setMemberRules] = createSignal<Record<string, string>>(
    Object.fromEntries(
      current.members
        .filter((member) => member.state !== "removed")
        .map((member) => [member.id, lines(member.iron_rules).join("\n")]),
    ),
  );
  const branch = settings.branch_per_task as { template?: string } | null | undefined;
  const [branchOn, setBranchOn] = createSignal(Boolean(branch?.template));
  const [template, setTemplate] = createSignal(branch?.template || "task/{id}");
  const [base, setBase] = createSignal(text(settings.base_branch));
  const [restart, setRestart] = createSignal(Boolean(settings.restart_crashed));
  const [land, setLand] = createSignal(Boolean(settings.land_allowed));
  const [broker, setBroker] = createSignal(Boolean(settings.build_broker));
  const [leadUnpause, setLeadUnpause] = createSignal(Boolean(settings.lead_removes_own_pause));
  const [acceptance, setAcceptance] = createSignal(settings.phase_acceptance === "owner");
  // Task 199 (Qt TeamSettingsDialog U45): the models each member may switch to before a task.
  const modelMembers = current.members.filter((member) => !member.lead && member.state !== "removed");
  const [models, setModels] = createSignal<Record<string, { model: string; effort: string }[]>>(
    Object.fromEntries(modelMembers.map((member) => [member.id, [...(member.models ?? [])]])),
  );
  const [adding, setAdding] = createSignal<Record<string, { model: string; effort: string }>>({});
  const [modelTick, setModelTick] = createSignal(0);
  const catalogs = createModelCatalogs((computer, agent, account) => teamRoleModels(computer, agent, account), () => setModelTick((n) => n + 1));
  const catalogItem = (member: TeamMemberView) => ({
    computer: member.computer ?? "", agent: member.agent, account: member.account ?? "",
    model: adding()[member.id]?.model ?? "", effort: adding()[member.id]?.effort ?? "",
  });
  createEffect(() => {
    for (const member of modelMembers) catalogs.request({ ...catalogItem(member), model: "" });
  });
  const modelChoicesFor = (member: TeamMemberView) => (modelTick(), modelOptions(catalogItem(member), catalogs.get(catalogItem(member))));
  const effortChoicesFor = (member: TeamMemberView) => (modelTick(), effortOptions(catalogItem(member).model, catalogs.get(catalogItem(member)), catalogItem(member).effort));
  const changeAddingModel = (member: TeamMemberView, value: string) => {
    const item = { ...catalogItem(member), model: value };
    const next = selectModel(normalizeModelCatalog(catalogs.get(item) ?? []), value, item.effort);
    setAdding({ ...adding(), [member.id]: { model: next.modelId, effort: next.effortId } });
  };
  const addModel = (id: string) => {
    const entry = adding()[id];
    const model = entry?.model.trim() ?? "";
    if (!model || (models()[id] ?? []).some((choice) => choice.model === model && choice.effort === (entry?.effort ?? ""))) return;
    setModels({ ...models(), [id]: [...(models()[id] ?? []), { model, effort: entry?.effort ?? "" }] });
    setAdding({ ...adding(), [id]: { model: "", effort: "" } });
  };
  const modelsChanged = (member: TeamMemberView) => JSON.stringify(models()[member.id] ?? []) !== JSON.stringify(member.models ?? []);
  // Task 199 (U51): intake keys act at once, as Qt; a new key's secret shows once.
  const [intakes, setIntakes] = createSignal(current.intakes ?? []);
  const [intakeLabel, setIntakeLabel] = createSignal("");
  const [newKey, setNewKey] = createSignal<{ label: string; key: string } | undefined>();
  const [intakeError, setIntakeError] = createSignal("");
  const intake = async (action: { kind: "intake_key"; label?: string; revoke?: string }) => {
    setIntakeError("");
    try {
      let outcome = await teamAction({ team: current.id, team_name: current.name, action });
      if (outcome.outcome === "confirm") {
        if (!window.confirm(outcome.confirmation.text)) return undefined;
        outcome = await teamAction({ team: current.id, team_name: current.name, action, confirmed: true });
      }
      if (outcome.outcome !== "done") { setIntakeError(outcome.outcome === "confirm" ? outcome.confirmation.text : outcome.detail); return undefined; }
      return outcome.receipt as Record<string, unknown>;
    } catch (cause) {
      setIntakeError(String(cause));
      return undefined;
    }
  };
  const createIntake = async () => {
    const label = intakeLabel().trim();
    if (!label || intakes().length >= 8) return;
    const receipt = await intake({ kind: "intake_key", label });
    if (!receipt) return;
    setIntakes([...intakes(), { id: String(receipt.id ?? ""), label: String(receipt.label ?? label), created: Date.now() / 1000 }]);
    setNewKey({ label, key: String(receipt.key ?? "") });
    setIntakeLabel("");
  };
  const revokeIntake = async (id: string, label: string) => {
    if (await intake({ kind: "intake_key", revoke: id, label })) setIntakes(intakes().filter((found) => found.id !== id));
  };
  const [asking, setAsking] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  // Task 88: the project's test configuration and the owner's override.
  const [tests, setTests] = createSignal(overrideText(settings.tests));
  const [testConfig, setTestConfig] = createSignal<TestConfig | undefined>();
  const [testCheck, setTestCheck] = createSignal<{ message: string; good: boolean } | undefined>();
  const [checking, setChecking] = createSignal(false);
  const [proposal, setProposal] = createSignal(false);
  onMount(() => {
    void readTestConfig(teamAction, current.id, current.name).then(setTestConfig).catch((cause) => setTestConfig({ ok: false, error: String(cause) }));
  });
  const checkTests = async () => {
    setChecking(true);
    try {
      const result = await checkTestConfig(teamAction, current.id, current.name, tests());
      setTestCheck({ message: result.message, good: result.good });
      if (result.good && result.config) setTestConfig(result.config);
    } catch (cause) {
      setTestCheck({ message: String(cause), good: false });
    } finally {
      setChecking(false);
    }
  };

  const rulesChanged = () => lines(rules()).join("\n") !== lines(settings.iron_rules).join("\n");
  const memberChanged = (member: TeamMemberView) =>
    (memberRules()[member.id] ?? "") !== lines(member.iron_rules).join("\n");
  const changed = () => {
    const patch: TeamSettings = {};
    if (rulesChanged()) patch.iron_rules = rules().split("\n").map((line) => line.trim()).filter(Boolean);
    const beforeTemplate = branch?.template || "";
    const afterTemplate = branchOn() ? template().trim() : "";
    if (afterTemplate !== beforeTemplate) patch.branch_per_task = afterTemplate ? { template: afterTemplate } : null;
    if (base().trim() !== text(settings.base_branch)) patch.base_branch = base().trim();
    if (restart() !== Boolean(settings.restart_crashed)) patch.restart_crashed = restart();
    if (land() !== Boolean(settings.land_allowed)) patch.land_allowed = land();
    if (broker() !== Boolean(settings.build_broker)) patch.build_broker = broker();
    if (leadUnpause() !== Boolean(settings.lead_removes_own_pause)) patch.lead_removes_own_pause = leadUnpause();
    if (acceptance() !== (settings.phase_acceptance === "owner")) patch.phase_acceptance = acceptance() ? "owner" : "lead";
    Object.assign(patch, testsPatch(settings.tests, tests()));
    return patch;
  };
  const members = () =>
    current.members
      .filter((member) => member.state !== "removed" && (memberChanged(member) || modelsChanged(member)))
      .map((member) => {
        const update: Record<string, unknown> = { id: member.id };
        if (memberChanged(member)) update.iron_rules = (memberRules()[member.id] ?? "").split("\n").map((line) => line.trim()).filter(Boolean);
        if (modelsChanged(member)) update.models = (models()[member.id] ?? []).map((choice) => (choice.effort ? choice : { model: choice.model }));
        return update;
      });

  const save = async () => {
    const invalid = parseOverride(tests()).error;
    if (invalid) {
      setError(invalid);
      return;
    }
    const patch = changed();
    const memberPatch = members();
    if (Object.keys(patch).length === 0 && memberPatch.length === 0) {
      props.onClose();
      return;
    }
    setBusy(true);
    setError("");
    try {
      const outcome = await teamAction({
        team: current.id,
        team_name: current.name,
        action: { kind: "settings", settings: patch, members_update: memberPatch },
      });
      if (outcome.outcome === "done") {
        props.onSaved?.();
        props.onClose();
      } else {
        setError(outcome.outcome === "confirm" ? outcome.confirmation.text : outcome.detail);
      }
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(false);
    }
  };
  const requestSave = () => {
    if (!land() || Boolean(settings.land_allowed)) {
      void save();
      return;
    }
    setAsking(true);
  };

  return (
    <aside class="nt-settings" aria-label={tr("Team settings")}>
      <header class="nt-head">
        <h2>{tr("Team settings")}</h2>
        <span class="grow" />
        <button type="button" class="icon-btn small" aria-label={tr("Close")} onClick={props.onClose}>
          <Icon name="x" size={14} />
        </button>
      </header>
      <div class="nt-body">
        <h3>{tr("Team iron rules")}</h3>
        <p class="nt-note">{tr("Mandatory additions to project rules. Only you, or the lead quoting your verified word, may change them.")}</p>
        <TextArea class="nt-rules" rows={4} data-team-rules placeholder={tr("One additional rule per line (20 maximum)")}
          value={rules()} onInput={(event) => setRules(event.currentTarget.value)} />
        {/* Task 324 (Qt TeamSettingsDialog): the member rules come right after the team's rules. */}
        <For each={current.members.filter((member) => member.state !== "removed")}>
          {(member) => (
            <label class="nt-field">
              <span>{tr("%1: member iron rules", member.role || member.id)}</span>
              <TextArea
                class="nt-rules"
                rows={2}
                placeholder={tr("One additional rule per line (20 maximum)")}
                value={memberRules()[member.id] ?? ""}
                onInput={(event) => setMemberRules({ ...memberRules(), [member.id]: event.currentTarget.value })}
              />
            </label>
          )}
        </For>

        <h3>{tr("Branches")}</h3>
        <label class="nt-check">
          <input type="checkbox" checked={branchOn()} onChange={(event) => setBranchOn(event.currentTarget.checked)} />
          {tr("A branch per task")}
        </label>
        <div class="nt-row">
          <TextInput value={template()} onInput={(event) => setTemplate(event.currentTarget.value)} placeholder={tr("Branch name template")} />
          <TextInput value={base()} onInput={(event) => setBase(event.currentTarget.value)} placeholder={tr("the lead's branch")} />
        </div>
        <p class="nt-note">{tr("The branch name: {id} is the task, {member} the member and {team8} the team's first eight characters")}</p>

        <h3>{tr("Landing")}</h3>
        <label class="nt-check">
          <input type="checkbox" checked={land()} onChange={(event) => setLand(event.currentTarget.checked)} />
          {tr("Let land stages land tasks (land_allowed)")}
        </label>

        <h3>{tr("Members")}</h3>
        <label class="nt-check">
          <input type="checkbox" checked={restart()} onChange={(event) => setRestart(event.currentTarget.checked)} />
          {tr("Restart a member whose session crashed")}
        </label>

        <h3>{tr("Builds")}</h3>
        <label class="nt-check">
          <input type="checkbox" checked={broker()} onChange={(event) => setBroker(event.currentTarget.checked)} />
          {tr("Build broker for members sharing a folder")}
        </label>

        {/* Task 88: the project's test providers and templates, of any stack, and your override; nothing is written before Save. */}
        <h3>{tr("Tests")}</h3>
        <p class="nt-note" data-tests-source>{testConfig() ? (testConfig()!.ok ? sourceLine(testConfig()!) : tr("The project's test configuration does not load: %1", testConfig()!.error ?? "")) : tr("Reading the project's test configuration…")}</p>
        <Show when={testConfig()?.ok}>
          <ul class="nt-list" data-tests-list>
            <For each={testsLines(testConfig()!)}>{(line) => <li>{line}</li>}</For>
          </ul>
        </Show>
        <p class="nt-note">{tr("Your override, as JSON shaped like .zerus/tests.toml: providers merge by name, templates replace the project's list. Empty: the project's file alone.")}</p>
        <TextArea rows={8} class="nt-mono" data-tests-override value={tests()} placeholder={'{"provider": [{"name": "python", "kind": "pytest", "results": {"format": "junit", "path": "out/pytest.xml"}}]}'}
          onInput={(event) => { setTests(event.currentTarget.value); setTestCheck(undefined); }} />
        <div class="nt-row">
          <button type="button" class="btn ghost small" data-tests-check disabled={checking()} title={tr("Ask hgs whether the project's file with this override loads; nothing is written")}
            onClick={() => void checkTests()}>{checking() ? tr("Checking with hgs…") : tr("Check")}</button>
          <button type="button" class="btn ghost small" data-tests-proposal-toggle onClick={() => setProposal(!proposal())}>
            {proposal() ? tr("Hide the detected proposal") : tr("Show the detected proposal")}
          </button>
        </div>
        <Show when={testCheck()}>{(result) => <p classList={{ "nt-note": true, "nt-error": !result().good }} data-tests-check-result>{result().message}</p>}</Show>
        <Show when={proposal()}><pre class="nt-mono" data-tests-proposal>{testConfig()?.proposal ?? ""}</pre></Show>

        <h3>{tr("Phases")}</h3>
        <label class="nt-line">
          <span>{tr("Who removes the lead's pause")}</span>
          <select value={leadUnpause() ? "lead" : "owner"} onChange={(event) => setLeadUnpause(event.currentTarget.value === "lead")} data-setting="lead-unpause">
            <option value="owner">{tr("Only with my consent")}</option>
            <option value="lead">{tr("The lead removes its own pause")}</option>
          </select>
        </label>
        <label class="nt-check">
          <input type="checkbox" checked={acceptance()} onChange={(event) => setAcceptance(event.currentTarget.checked)} />
          {tr("New phases require my acceptance")}
        </label>
        <p class="nt-note">{tr("When every task of such a phase is decided, it waits for you to accept it or send it back to the lead with a comment.")}</p>

        <h3>{tr("Member models")}</h3>
        <p class="nt-note">{tr("Before a task that names one of these, Zerus switches the member to it. None: only the model it launched with. A change applies from the member's next task.")}</p>
        <For each={modelMembers} fallback={<p class="nt-note">{tr("No members yet.")}</p>}>{(member) => (
          <div class="nt-models" data-member={member.id}>
            <span><b>{member.role || member.id}</b> <span class="nt-note">{member.model ? tr("launches with %1", member.model) : tr("launches with its agent's model")}</span></span>
            <div class="nt-model-chips">
              <For each={models()[member.id] ?? []}>{(choice, index) => (
                <span class="team-chip" data-model={choice.model}>{choice.effort ? `${choice.model} ${choice.effort}` : choice.model}
                  <button type="button" class="icon-btn small" aria-label={tr("Remove %1", choice.model)}
                    onClick={() => setModels({ ...models(), [member.id]: (models()[member.id] ?? []).filter((_, at) => at !== index()) })}><Icon name="x" size={10} /></button>
                </span>
              )}</For>
            </div>
            <div class="nt-line">
              <TextInput placeholder={tr("+ model")} value={adding()[member.id]?.model ?? ""} data-field="model-add" list={`nt-member-models-${member.id}`}
                onInput={(event) => changeAddingModel(member, event.currentTarget.value)} />
              <datalist id={`nt-member-models-${member.id}`}><For each={modelChoicesFor(member)}>{(option) => <option value={option.id}>{option.label}</option>}</For></datalist>
              <select value={adding()[member.id]?.effort ?? ""}
                onChange={(event) => setAdding({ ...adding(), [member.id]: { model: adding()[member.id]?.model ?? "", effort: event.currentTarget.value } })}>
                <For each={effortChoicesFor(member)}>{(level) => <option value={level}>{level || tr("its effort")}</option>}</For>
              </select>
              <button type="button" class="btn ghost small" disabled={!(adding()[member.id]?.model ?? "").trim()} onClick={() => addModel(member.id)}>{tr("Add")}</button>
            </div>
          </div>
        )}</For>

        <h3>{tr("Intake keys")}</h3>
        <p class="nt-note">{tr("A process presenting a key, such as a health watcher, may add up to five urgent tasks as one phase, and nothing else.")}</p>
        <For each={intakes()} fallback={<p class="nt-note">{tr("No intake keys: only you and the lead change the plan.")}</p>}>{(key) => (
          <div class="nt-line" data-intake={key.id}>
            <b>{key.label}</b>
            <span class="nt-note grow">{key.created ? tr("%1, created %2", key.id, new Date(key.created * 1000).toLocaleString()) : key.id}</span>
            <button type="button" class="btn ghost small danger" onClick={() => void revokeIntake(key.id, key.label)}>{tr("Revoke")}</button>
          </div>
        )}</For>
        <Show when={newKey()}>{(created) => (
          <div class="nt-key" data-new-key>
            <p class="nt-note">{tr("The key for %1, shown once: copy it now.", created().label)}</p>
            <code class="nt-mono">{created().key}</code>
          </div>
        )}</Show>
        <div class="nt-line">
          <TextInput placeholder={tr("Key name, such as health watcher")} value={intakeLabel()} maxLength={60} onInput={(event) => setIntakeLabel(event.currentTarget.value)} data-field="intake-label" />
          <button type="button" class="btn ghost small" disabled={!intakeLabel().trim() || intakes().length >= 8}
            title={intakes().length >= 8 ? tr("A team keeps at most eight keys; revoke one first") : undefined} onClick={() => void createIntake()}>{tr("Create key")}</button>
        </div>
        <Show when={intakeError()}><p class="nt-error">{intakeError()}</p></Show>
        <Show when={error()}><p class="nt-error">{error()}</p></Show>
      </div>
      <Show when={asking()}>
        <div class="nt-confirm" role="dialog" aria-label={tr("Allow landing")}>
          <p>{tr("Let land stages land this team's tasks? A land stage runs the catalog's command and may merge or push.")}</p>
          <div class="nt-confirm-actions">
            <button type="button" class="btn ghost small" autofocus onClick={() => setAsking(false)}>{tr("Cancel")}</button>
            <button type="button" class="btn small" disabled={busy()} onClick={() => { setAsking(false); void save(); }}>{tr("Allow landing")}</button>
          </div>
        </div>
      </Show>
      <footer class="nt-foot">
        <button type="button" class="btn ghost small" onClick={props.onClose}>{tr("Cancel")}</button>
        <button type="button" class="btn small" disabled={busy()} onClick={requestSave}>{tr("Save")}</button>
      </footer>
    </aside>
  );
}
