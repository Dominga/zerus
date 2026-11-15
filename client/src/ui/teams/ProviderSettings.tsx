import { For, Show, createEffect, createSignal, onCleanup, untrack } from "solid-js";
import { tr } from "../../i18n";
import { newRequestId, type TeamCall, type TeamOutcome, type TeamView } from "../../store/teams";
import { readProviderConfig, type ProviderInspection, type ProviderPort } from "../../store/teamProviders";
import { TextArea } from "../lib/TextField";
import { overrideText, parseOverride, sourceLine } from "./provider-settings";

// Adapted from WhiteWh's Tests section: source, override, read-only Check.
// Builds share this metadata; neither checking nor selecting starts work.
export function ProviderSettings(props: { view: TeamView; locked: boolean; onSave: (call: TeamCall) => Promise<TeamOutcome>; port?: ProviderPort }) {
  const [shown, setShown] = createSignal<ProviderInspection>();
  const [text, setText] = createSignal("");
  const [checked, setChecked] = createSignal<ProviderInspection>();
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  const [unknown, setUnknown] = createSignal(false);
  let generation = 0;
  onCleanup(() => { generation++; });
  const read = async (candidate = false) => {
    const parsed = parseOverride(text());
    if (candidate && parsed.error) { setError(parsed.error); return; }
    const team = props.view.bar.id, home = props.view.bar.home, project = props.view.bar.project, revision = props.view.revision, token = ++generation;
    setBusy(true); setError(""); setChecked(undefined);
    try {
      const value = await (props.port ?? readProviderConfig)({ team, ...(candidate ? { candidate: { override_config: parsed.value } } : {}) });
      if (token !== generation || team !== props.view.bar.id || home !== props.view.bar.home || project !== props.view.bar.project || revision !== props.view.revision) return;
      if (value.schema !== 1 || value.team !== team || value.home !== home || value.project !== project || value.revision !== revision || value.providers.length > 16) throw new Error(tr("The team changed. Read its provider configuration again."));
      setShown(value);
      if (!candidate) setText(overrideText(value.override_config));
      else if (value.valid) setChecked(value);
    } catch (e) { if (token === generation) setError(String(e instanceof Error ? e.message : e).slice(0, 2000)); }
    finally { if (token === generation) setBusy(false); }
  };
  createEffect(() => { props.view.bar.id; props.view.bar.home; props.view.bar.project; props.view.revision; generation++; setShown(undefined); setChecked(undefined); untrack(() => { void read(); }); });
  const save = async () => {
    const value = checked();
    if (!value || busy() || props.locked || unknown() || value.team !== props.view.bar.id || value.revision !== props.view.revision) return;
    setBusy(true); setError("");
    try {
      const outcome = await props.onSave({ team: value.team, team_name: props.view.bar.name, expected_revision: value.revision, request_id: newRequestId(), action: { kind: "validation_config", selection: { source_digest: value.source_digest, digest: value.digest, override_config: value.override_config } } });
      if (outcome.outcome === "done") { setChecked(undefined); await read(); }
      else if (outcome.outcome === "failed") { setError(outcome.detail); setChecked(undefined); }
      else { setUnknown(true); setChecked(undefined); setError(tr("The selection is not confirmed. Check the original action in the Team tab.")); }
    } catch { setUnknown(true); setChecked(undefined); setError(tr("The selection is not confirmed. Check the original action in the Team tab.")); }
    finally { setBusy(false); }
  };
  return <section data-provider-settings>
    <h3>{tr("Tests and Builds")}</h3>
    <p class="hint">{tr("Tests and builds remain tasks for member agents. Opening or checking this section runs nothing and saves nothing.")}</p>
    <Show when={shown()} fallback={<p class="hint">{busy() ? tr("Reading provider configuration…") : tr("Provider inspection is unavailable in this version of hgs.")}</p>}>{(value) => <>
      <p data-provider-source>{sourceLine(value())}</p>
      <p class="hint mono">{value().checkout}</p>
      <p class="hint mono" data-provider-digest>{tr("Configuration digest: %1", value().digest || tr("Unavailable"))}</p>
      <Show when={value().saved && !value().saved_current}><p class="notice">{tr("The saved selection no longer matches this checkout or configuration. Review it again.")}</p></Show>
      <For each={value().diagnostics}>{(d) => <p class="notice danger" data-provider-diagnostic>{d.code}: {d.message}</p>}</For>
      <ul><For each={value().providers}>{(p) => <li>{p.name} ({p.provider}, {p.kind}){p.disabled ? tr(", off") : ""}</li>}</For></ul>
    </>}</Show>
    <p class="hint">{tr("Override as JSON. Providers replace matching names. Empty uses the project file, or detected metadata when the file is absent.")}</p>
    <TextArea rows={7} aria-label={tr("Provider override")} data-provider-override value={text()} disabled={busy() || props.locked || unknown()} onInput={(e) => { setText(e.currentTarget.value); setChecked(undefined); }} />
    <div class="row"><button type="button" class="btn" data-provider-check disabled={busy() || props.locked || unknown()} onClick={() => void read(true)}>{tr("Check")}</button>
      <button type="button" class="btn" data-provider-read disabled={busy() || props.locked || unknown()} onClick={() => void read()}>{tr("Read again")}</button>
      <button type="button" class="btn" data-provider-save disabled={!checked() || busy() || props.locked || unknown()} onClick={() => void save()}>{tr("Save provider selection")}</button></div>
    <p class="hint" data-provider-unsupported>{tr("Unsupported: direct runners, executable templates, validation requests, report import and mandatory gates.")}</p>
    <Show when={error()}><p role="alert" class="notice danger">{error()}</p></Show>
  </section>;
}
