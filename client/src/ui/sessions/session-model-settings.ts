import type { Session } from "./types";
import { effortValid } from "./opencode-models";
import { tr } from "../../i18n";

export interface ModelChoice { id: string; label: string; effort_options: string[]; effort_labels: string[] }
export interface ModelContext {
  row: Session;
  details?: Record<string, unknown>;
  online: boolean;
  busy: boolean;
  inspectError?: boolean;
  answering?: boolean;
}
export const text = (value: unknown) => typeof value === "string" ? value : "";
const short = (value: unknown, limit: number) => { const s = text(value); return s.length <= limit && !/\p{Cc}/u.test(s) ? s : ""; };

/** The exact account's inspected catalog, in native order, with bounded choices. */
export function modelChoices(value: unknown): ModelChoice[] {
  if (!Array.isArray(value)) return [];
  const seen = new Set<string>();
  return value.slice(0, 256).flatMap((entry) => {
    if (!entry || typeof entry !== "object") return [];
    const id = short(entry.value ?? entry.id, 256);
    if (!id || seen.has(id)) return [];
    seen.add(id);
    const variants: unknown[] = Array.isArray(entry.effort_options) ? entry.effort_options.slice(0, 64) : [];
    const labels = Array.isArray(entry.effort_labels) ? entry.effort_labels : [];
    const efforts = variants.flatMap((v, i) => short(v, 40) ? [{ value: text(v), label: short(labels[i], 80) || text(v) }] : []);
    return [{ id, label: short(entry.label, 160) || id, effort_options: efforts.map((e) => e.value), effort_labels: efforts.map((e) => e.label) }];
  });
}

/** Untrusted text on one line: control, line/paragraph separator and bidi
 * control characters become spaces, so a reason cannot add lines or reorder. */
const oneLine = (value: string) =>
  value.replace(/[\p{Cc}\p{Zl}\p{Zp}\u061C\u200E\u200F\u202A-\u202E\u2066-\u2069]+/gu, " ").trim();

/** A new DeepSeek session whose account launch defaults were not applied
 * (`account_defaults` from `hgs inspect`, zerus-fw3g5). Applied, pending and
 * absent outcomes show nothing, and nothing here retries the application. */
export function accountDefaultsNotice(value: unknown) {
  const o: Record<string, unknown> = value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {};
  const status = text(o.status);
  if (status !== "failed" && status !== "uncertain") return undefined;
  const model = oneLine(short(o.model, 256)), effort = oneLine(short(o.effort, 40));
  const choice = model ? effort ? `${model} (${effort})` : model : "";
  return {
    status,
    title: status === "failed" ? tr("Account defaults were not applied") : tr("Account defaults may not have been applied"),
    text: status === "failed"
      ? tr("The account's model and effort were not applied when this session started. It began with the DeepSeek default.")
      : tr("DeepSeek may not have applied the account's model and effort when this session started. Check the model and effort above."),
    choice: choice ? tr("Account default: %1", choice) : "",
    // hgs's own refusal text, bounded and on one line.
    detail: oneLine(text(o.detail)).slice(0, 600),
  };
}

/** MessageComposer::setModelSettings/updateSettingsButton/updateSettingsApply. */
export function settingsView(input: ModelContext, changing = false) {
  const { row, details: d, online, busy } = input;
  const model = typeof d?.settings_model === "string" ? d.settings_model : text(d?.model) || row.model;
  const effort = typeof d?.effort === "string" ? d.effort : row.effort ?? "";
  const pendingModel = text(d?.pending_model), pendingEffort = text(d?.pending_effort);
  const pending = !!pendingModel || !!pendingEffort;
  const applyWhen = text(d?.settings_apply_when);
  const reason = !online ? tr("Machine unavailable. Reconnect to change model settings.")
    : changing ? tr("Updating model settings…")
    : busy ? tr("Wait for the session update to finish.")
    : input.answering ? tr("Wait for the answer to be delivered.")
    : input.inspectError || !d ? tr("Refresh the session before changing model settings.")
    : d.settings_change_supported !== true ? text(d.settings_change_reason) || tr("Model settings are not available for this session.") : "";
  const shownModel = model || "Model", suffix = (effort ? `  ${effort}` : "") + (pending ? "  ◷" : "");
  let hint = reason || (applyWhen === "resume" ? tr("Saved for the next resume. The session stays stopped.")
    : applyWhen === "ready" ? tr("Applies when the agent is ready. Messages sent while it is working use the current model.") : tr("Applies to this session only."));
  const changeNotice = text(d?.settings_change_notice) || (row.cmd === "claude" ? tr("Claude re-reads the conversation with the new model on its next turn.") : "");
  if (pending) hint = `Pending: ${pendingModel || model}${pendingEffort ? `  ${pendingEffort}` : ""}\nCurrent: ${model}${effort ? `  ${effort}` : ""}\n${hint}`;
  const defaults = accountDefaultsNotice(d?.account_defaults);
  let tooltip = `Current: ${shownModel}${effort ? `  ${effort}` : ""}`;
  if (pending) tooltip += `\nPending: ${pendingModel || model}${pendingEffort ? `  ${pendingEffort}` : ""}`;
  if (reason) tooltip += `\n${reason}`;
  if (defaults) tooltip += `\n${defaults.title}`;
  return { model, effort, shownModel, suffix, tooltip, defaults,
    accessible: tr("Model and reasoning effort: %1%2", shownModel, suffix) + (defaults ? `\n${defaults.title}` : ""),
    pending, pendingModel, pendingEffort, applyWhen, reason, hint, changeNotice, enabled: !reason,
    intendedModel: pendingModel || model, intendedEffort: pending ? pendingEffort : effort,
    applyLabel: applyWhen === "resume" ? tr("Save for resume") : applyWhen === "ready" ? tr("Save change") : "Apply",
    openCode: row.cmd === "opencode", terminal: row.cmd === "claude" && online && row.state === "running" && !text(d?.expected_id),
    options: modelChoices(d?.model_options) };
}

/** Preserve the previous effort when offered; OpenCode's empty Default is explicit. */
export function effortChoices(options: ModelChoice[], model: string, preferred: string, openCode: boolean, currentModel: string, currentEffort: string) {
  const option = options.find((m) => m.id === model);
  const choices: { value: string; label: string; disabled?: boolean }[] = openCode ? [{ value: "", label: "Default" }] : [];
  option?.effort_options.forEach((effort, i) => {
    if ((!openCode || effortValid(effort)) && !choices.some((c) => c.value === effort)) choices.push({ value: effort, label: openCode ? option.effort_labels[i] || effort : effort });
  });
  if (openCode && model === currentModel && effortValid(currentEffort) && !choices.some((c) => c.value === currentEffort)) choices.push({ value: currentEffort, label: currentEffort });
  // Missing native metadata is not the first catalog effort. Keep it
  // unselected until the user explicitly chooses an available level.
  if (!openCode && !preferred && choices.length) choices.unshift({ value: "", label: tr("Unknown"), disabled: true });
  if (!choices.length) choices.push({ value: "", label: tr("Provider default") });
  return { choices, value: choices.some((c) => c.value === preferred) ? preferred : choices[0].value };
}
