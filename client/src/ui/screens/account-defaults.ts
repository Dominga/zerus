import type { AccountEditView,AccountModelCatalog } from "../../bridge";
import { providerName } from "../lib/usage";
import { tr, uiLanguage } from "../../i18n";
import { effortOptions, modelOptions, normalizeModelCatalog, type ModelOption } from "../provider-selection";

/** Computer names as one phrase in the UI language: "arch, mac and va-kids". */
export function computerList(names: readonly string[], language: string = uiLanguage()) {
  try { return new Intl.ListFormat(language, { style: "long", type: "conjunction" }).format(names); }
  catch { return names.join(", "); }
}
/** Where an account edit saves: the selected computer, or every one it reaches (zerus-f0uz8). */
export function savesOn(view: Pick<AccountEditView, "label" | "machine" | "machines">) {
  const machines = view.machines?.length ? view.machines : [view.machine];
  return machines.length > 1 ? tr("%1 on %2", view.label, computerList(machines)) : tr("%1 on %2", view.label, machines[0]);
}

export function permissionExplanation(view:AccountEditView,mode:string) {
  return mode==="provider" ? tr("Inherit %1 settings on %2. %3", providerName(view.provider), view.machine, view.permission_mode==="bypass" ? "" : view.permission_detail??"")
    : view.provider==="kimi" ? tr("Run tools and answer questions automatically (Never Ask).")
    : ["codex","dsh"].includes(view.provider) ? tr("Full access without approval prompts or a sandbox.")
    : tr("Run tools without permission prompts.");
}
/** Account-specific choices; a missing saved model remains visible and cannot be saved. */
export function accountModelChoices(catalog:AccountModelCatalog): ModelOption[] {
  return modelOptions(normalizeModelCatalog(catalog), catalog.defaults.model).map((option) => option.unavailable
    ? { ...option, label: `${option.id} (currently unavailable)` }
    : option);
}
export function accountModelView(catalog:AccountModelCatalog,model:string) {
  const normalized = normalizeModelCatalog(catalog);
  const chosen = normalized.models.find((option) => option.id === model);
  return {
    efforts: normalized.effortSupported ? effortOptions(normalized, model) : [],
    effortEnabled:normalized.effortSupported && !!model && !!chosen?.efforts.length,
    canSave:!model || !!chosen,
    hint:!normalized.effortSupported ? tr("Kimi manages effort in its native settings. This choice sets the model for new conversations.")
      : !normalized.models.length ? tr("Open this account's agent once, then refresh to load its models.")
      : model && !chosen ? tr("This model is absent from the current account catalog. Choose another model or Provider settings.")
      : tr("Choose the reasoning effort for the selected model, or keep its provider default."),
  };
}
