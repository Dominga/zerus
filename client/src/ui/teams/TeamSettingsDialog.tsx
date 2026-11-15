import { For, Show, batch, createEffect, createSignal, createUniqueId, onCleanup, onMount } from "solid-js";
import type { JSX } from "solid-js";
import { tr } from "../../i18n";
import { newRequestId, type TeamCall, type TeamOutcome, type TeamView } from "../../store/teams";
import { Icon } from "../lib/icons";
import { TextInput } from "../lib/TextField";
import { teamCall } from "./actions";
import { delegationWords } from "./NewTeam";
import { CAPS, DELEGATIONS, TIMERS, capMax, completeSettings, wholeNumber, type TeamSettings } from "./team-settings";
import "./team-settings.css";
import { ProviderSettings } from "./ProviderSettings";

// Adapted from WhiteWh's TeamSettingsDialog: one saved settings object, the
// phase choice. Only our settings op is
// used; fork-specific rules, models, tests and intake operations do not map here.
export interface TeamSettingsDialogProps {
  /** The live view; only a stale refusal admits a newer revision into the draft. */
  view: TeamView;
  providerHome?: string;
  busy: boolean;
  onSave: (call: TeamCall) => Promise<TeamOutcome>;
  onClose: () => void;
}

function numberLabel(key: string): string {
  switch (key) {
    case "max_members": return tr("Members besides the lead");
    case "max_phase_tasks": return tr("Tasks per phase");
    case "max_open_tasks": return tr("Open tasks");
    case "max_open_questions": return tr("Open questions per attempt");
    case "report_timeout_seconds": return tr("Report timeout (seconds)");
    case "start_timeout_seconds": return tr("Start timeout (seconds)");
    case "inbox_interval_seconds": return tr("Lead doorbell interval (seconds)");
    case "remind_lead_minutes": return tr("Remind the lead after (minutes)");
    case "remind_person_minutes": return tr("Remind you after (minutes)");
    case "lead_stall_minutes": return tr("Escalate an idle lead after (minutes)");
    case "answer_escalate_minutes": return tr("Escalate unanswered requests after (minutes)");
    default: return tr("Resumes per hour");
  }
}

export function TeamSettingsDialog(props: TeamSettingsDialogProps): JSX.Element {
  const snapshot = (view: TeamView) => ({ ...view, bar: { ...view.bar } });
  const written = (view: TeamView) => completeSettings(view.settings) ? structuredClone(view.settings) : undefined;
  const numberStrings = (value: TeamSettings | undefined): Record<string, string> => value ? Object.fromEntries([...Object.entries(value.caps), ...Object.entries(value.timers)].map(([key, number]) => [key, String(number)])) : {};
  const [current, setCurrent] = createSignal(snapshot(props.view));
  const [initial, setInitial] = createSignal(written(props.view));
  const [settings, setSettings] = createSignal(initial());
  const [numbers, setNumbers] = createSignal(numberStrings(initial()));
  const [busy, setBusy] = createSignal(false);
  const [stale, setStale] = createSignal(false);
  const [reloaded, setReloaded] = createSignal(false);
  const [discardedEdits, setDiscardedEdits] = createSignal(false);
  const [error, setError] = createSignal("");
  const [errorField, setErrorField] = createSignal<string>();
  const knownField = (field?: string) => [...CAPS.map((key) => `settings.caps.${key}`), ...TIMERS.map((key) => `settings.timers.${key}`)].includes(field ?? "") ? field : undefined;
  const fieldError = (section: "caps" | "timers", key: string) => {
    if (errorField() === `settings.${section}.${key}`) return error();
    const maximum = section === "caps" ? capMax(key as typeof CAPS[number]) : 0xffffffff;
    return wholeNumber(numbers()[key], section === "caps" ? 1 : 0, maximum) === undefined ? tr("Enter whole numbers within the shown limits.") : "";
  };
  const editNumber = (section: "caps" | "timers", key: string, text: string) => batch(() => {
    setNumbers({ ...numbers(), [key]: text });
    if (errorField() === `settings.${section}.${key}`) { setError(""); setErrorField(undefined); }
  });
  const title = createUniqueId();
  let dialog!: HTMLDialogElement;
  const origin = document.activeElement instanceof HTMLElement ? document.activeElement : undefined;
  const locked = () => busy() || props.busy || stale();
  const close = () => { if (!busy()) props.onClose(); };
  onMount(() => { dialog.showModal(); queueMicrotask(() => dialog.querySelector<HTMLElement>("[data-initial]")?.focus()); });
  onCleanup(() => { if (dialog.open) dialog.close(); if (origin?.isConnected) origin.focus(); });
  const set = <K extends keyof TeamSettings>(key: K, value: TeamSettings[K]) => setSettings((old) => old && ({ ...old, [key]: value }));
  const value = () => {
    const draft = settings();
    if (!draft) return undefined;
    const caps = { ...draft.caps }, timers = { ...draft.timers };
    for (const key of CAPS) {
      const number = wholeNumber(numbers()[key], 1, capMax(key));
      if (number === undefined) return undefined;
      caps[key] = number;
    }
    for (const key of TIMERS) {
      // The fixed standing-resume rule has no configurable reader (.158.1).
      if (key === "resume_limit_per_hour") { timers[key] = initial()!.timers[key]; continue; }
      const number = wholeNumber(numbers()[key], 0, 0xffffffff);
      if (number === undefined) return undefined;
      timers[key] = number;
    }
    return { ...draft, land_allowed: initial()!.land_allowed, caps, timers };
  };
  const changed = () => !!initial() && JSON.stringify(value()) !== JSON.stringify(initial());
  createEffect(() => {
    const fresh = props.view;
    if (!stale() || fresh.bar.id !== current().bar.id || fresh.revision <= current().revision) return;
    const next = written(fresh);
    batch(() => {
      setDiscardedEdits(changed());
      setCurrent(snapshot(fresh)); setInitial(next); setSettings(next); setNumbers(numberStrings(next));
      setError(""); setErrorField(undefined); setReloaded(true); setStale(false);
    });
  });
  const save = async () => {
    const next = value();
    if (locked() || !next || !changed()) return;
    setErrorField(undefined);
    if (next.integration === "branch" && !current().bar.integration_branch) {
      setError(tr("An integration branch is required.")); return;
    }
    setBusy(true); setError(""); setErrorField(undefined);
    try {
      // The native action checks this whole value with crates/team-plan before
      // spawning hgs. The store alone keeps uncertain intents and their IDs.
      const outcome = await props.onSave(teamCall(current(), newRequestId(), "settings", { settings: next }));
      if (outcome.outcome === "done" || outcome.outcome === "uncertain" || outcome.outcome === "pending") props.onClose();
      else if (outcome.outcome === "failed") { setError(outcome.detail); setErrorField(knownField(outcome.field)); setStale(outcome.code === "stale_revision"); }
      else setError(outcome.confirmation.text);
    } finally { setBusy(false); }
  };
  return <dialog ref={dialog} class="dlg team-settings-dialog" aria-labelledby={title} data-team-settings
    onCancel={(event) => { event.preventDefault(); close(); }}>
    <div class="dlg-head"><Icon name="settings" size={18} /><h2 id={title}>{tr("Team settings")}</h2><span class="grow" />
      <button type="button" class="icon-btn" aria-label={tr("Close")} disabled={busy()} onClick={close}><Icon name="x" size={16} /></button></div>
    <div class="dlg-body">
      <p class="hint">{current().bar.name}</p>
      <Show when={reloaded()}><p class="notice" role="status" data-settings-reloaded>{tr("The team changed; the settings were reloaded")}
        <Show when={discardedEdits()}><span> {tr("Your unsaved edits were replaced. Review the reloaded settings before saving.")}</span></Show></p></Show>
      <Show when={settings()} fallback={<p role="alert">{tr("This version cannot show all team settings. Update Zerus and hgs before changing them.")}</p>}>
        <fieldset class="team-settings-fields" disabled={locked()}>
          <section><h3>{tr("Delegations")}</h3><For each={DELEGATIONS}>{(key) => <label class="check team-setting-check">
            <input type="checkbox" data-setting={key} checked={settings()!.delegations[key]} onChange={(event) => set("delegations", { ...settings()!.delegations, [key]: event.currentTarget.checked })} />
            <span>{delegationWords(key).name}<small class="hint">{delegationWords(key).sentence}</small></span>
          </label>}</For></section>
          <section><h3>{tr("Task limits")}</h3><div class="team-settings-grid"><For each={CAPS}>{(key) => <label>
            <span id={`${title}-${key}-label`}>{numberLabel(key)}</span><TextInput aria-labelledby={`${title}-${key}-label`} type="number" min={1} max={capMax(key)} step={1} aria-invalid={!!fieldError("caps", key)} aria-describedby={fieldError("caps", key) ? `${title}-${key}-error` : undefined} data-setting={key} value={numbers()[key]} onInput={(event) => editNumber("caps", key, event.currentTarget.value)} />
            <small class="hint">{tr("From %1 to %2", 1, capMax(key))}</small>
            <Show when={fieldError("caps", key)}>{(message) => <small class="danger" role="alert" id={`${title}-${key}-error`}>{message()}</small>}</Show>
          </label>}</For></div></section>
          <section><h3>{tr("Timers")}</h3><div class="team-settings-grid"><For each={TIMERS}>{(key) => <label>
            <span id={`${title}-${key}-label`}>{numberLabel(key)}</span><TextInput aria-labelledby={`${title}-${key}-label`} type="number" min={0} max={0xffffffff} step={1} disabled={key === "resume_limit_per_hour"} aria-invalid={!!fieldError("timers", key)} aria-describedby={fieldError("timers", key) ? `${title}-${key}-error` : undefined} data-setting={key} value={numbers()[key]} onInput={(event) => editNumber("timers", key, event.currentTarget.value)} />
            <small class="hint">{key === "resume_limit_per_hour" ? tr("Fixed at 3 per hour") : tr("From %1 to %2", 0, 0xffffffff)}</small>
            <Show when={fieldError("timers", key)}>{(message) => <small class="danger" role="alert" id={`${title}-${key}-error`}>{message()}</small>}</Show>
          </label>}</For></div></section>
          <section><h3>{tr("Workflow")}</h3>
            <label class="check"><input type="checkbox" checked={settings()!.queue} disabled />{tr("The team takes tasks as they come")}</label>
            <p class="hint">{tr("Queue enablement is fixed when the team is created.")}</p>
            <label><span>{tr("Routing")}</span><select data-setting="routing" value={settings()!.routing} onChange={(event) => set("routing", event.currentTarget.value as TeamSettings["routing"])}>
              <option value="direct">{tr("Straight to a free member")}</option><option value="lead">{tr("The lead routes first")}</option></select></label>
            <label><span>{tr("Plan approval")}</span><select data-setting="plan_approval" value={settings()!.plan_approval} onChange={(event) => set("plan_approval", event.currentTarget.value as TeamSettings["plan_approval"])}>
              <option value="review">{tr("Only with my consent")}</option><option value="auto">{tr("The lead approves plans")}</option></select></label>
            <label><span>{tr("Phase acceptance")}</span><select data-setting="phase_acceptance" value={settings()!.phase_acceptance} onChange={(event) => set("phase_acceptance", event.currentTarget.value as TeamSettings["phase_acceptance"])}>
              <option value="lead">{tr("The lead accepts")}</option><option value="person">{tr("I accept each phase")}</option></select></label>
          </section>
          <section><h3>{tr("Integration")}</h3>
            <label><span>{tr("Accepted work")}</span><select data-setting="integration" value={settings()!.integration} onChange={(event) => set("integration", event.currentTarget.value as TeamSettings["integration"])}>
              <option value="branch" disabled={!current().bar.integration_branch}>{tr("Merge into the integration branch")}</option><option value="none">{tr("Keep each task's branch")}</option></select></label>
            <Show when={current().bar.integration_branch}><p class="hint mono">{current().bar.integration_branch}</p></Show>
            <label class="check"><input type="checkbox" data-setting="land_allowed" checked={settings()!.land_allowed} disabled />{tr("Allow land stages")}</label>
            <p class="hint" data-landing-note>{tr("Saved with the team; landing is not gated by this setting yet")}</p>
          </section>
        </fieldset>
      </Show>
      <ProviderSettings view={props.view} home={props.providerHome} locked={locked()} onSave={props.onSave} />
      <Show when={stale()}><p class="hint">{tr("Waiting for the current team settings…")}</p></Show>
    </div>
    <Show when={error() && !errorField()}><p class="notice danger" role="alert" data-settings-error>{error()}</p></Show>
    <div class="dlg-foot"><span class="spacer" /><button type="button" class="btn" data-initial disabled={busy()} onClick={close}>{tr("Cancel")}</button>
      <button type="button" class="btn primary" data-settings-save disabled={locked() || !value() || !changed()} onClick={() => void save()}>{tr("Save")}</button></div>
  </dialog>;
}
