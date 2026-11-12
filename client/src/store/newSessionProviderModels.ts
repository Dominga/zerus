import { tr } from "../i18n";
import { effortOptions, normalizeModelCatalog, selectModel } from "../ui/provider-selection";
import { ModelPicker, type ModelOption } from "../ui/sessions/opencode-models";
import { string } from "../ui/lib/json";
import type { EffortOption, OpenCodeHost, OpenCodeView } from "./newSessionOpencode";

const nativeModel = (): ModelOption => ({ value: "", text: tr("Native default model"), label: "", efforts: [], effortLabels: [] });
const errorText = (error: unknown) => typeof error === "string" ? error : error instanceof Error ? error.message : String(error);

export function createStandardModels(h: OpenCodeHost) {
  let closed = false;
  let ticket = 0;
  let contextKey = "";
  let models: ModelOption[] = [nativeModel()];
  let modelValue = "";
  let efforts: EffortOption[] = [{ value: "", label: tr("Default") }];
  let effortValue = "";
  let effortVisible = false;
  const picker = new ModelPicker();

  const currentOption = () => models.find((model) => model.value === modelValue);
  const accountId = () => string(h.accountProfile().id);

  function updateEfforts() {
    const current = currentOption();
    const catalog = normalizeModelCatalog({ models: current ? [{
      id: current.value, label: current.label, effort_options: current.efforts, effort_labels: current.effortLabels, default_effort: current.defaultEffort,
    }] : [] });
    const selected = selectModel(catalog, modelValue, effortValue);
    const enabled = h.agent() !== "kimi" && modelValue !== "" && (current?.efforts.length ?? 0) > 0;
    efforts = effortOptions(catalog, modelValue, selected.effortId, tr("Default")).map((effort) => ({ value: effort.id, label: effort.label }));
    effortValue = h.agent() === "kimi" ? "" : selected.effortId;
    effortVisible = enabled;
  }

  function updateModelButton() {
    picker.setCurrent(modelValue);
  }

  function requestCatalog(key: string, id: string) {
    const request = ++ticket;
    h.bridge.teamRoleModels(h.host(), h.agent(), id).then((rows) => {
      if (closed || request !== ticket || key !== contextKey) return;
      models = [nativeModel(), ...normalizeModelCatalog(rows).models.map((model) => ({
        value: model.id,
        text: model.label,
        label: model.label,
        efforts: model.efforts.map((effort) => effort.id),
        effortLabels: model.efforts.map((effort) => effort.label),
        ...(model.defaultEffort ? { defaultEffort: model.defaultEffort } : {}),
      }))];
      updateEfforts();
      updateModelButton();
      h.emit();
    }, (failure) => {
      if (!closed && request === ticket && key === contextKey) h.setAccountStatus(errorText(failure));
    });
  }

  function updateModels() {
    const id = accountId();
    const key = `${h.host()}\n${h.agent()}\n${id}`;
    if (key !== contextKey) {
      contextKey = key;
      models = [nativeModel()];
      modelValue = "";
      effortValue = "";
      if (id) requestCatalog(key, id);
    }
    updateEfforts();
    updateModelButton();
  }

  const service = (): OpenCodeView["service"] => ({ visible: false, reason: "", offer: false, text: "", enabled: false, tooltip: "" });

  return {
    updateModels,
    model: () => modelValue,
    effort: () => effortValue,
    dropUnready() {},
    restore(model: string, effort: string) {
      modelValue = model;
      effortValue = effort;
      if (model && !models.some((option) => option.value === model)) {
        models.push({ value: model, text: model, label: model, efforts: effort ? [effort] : [], effortLabels: effort ? [effort] : [] });
      }
      updateEfforts();
      updateModelButton();
    },
    view(): OpenCodeView {
      return {
        models, model: modelValue, modelVisible: true,
        modelButton: modelValue === "" ? tr("Native default model") : currentOption()?.label || modelValue,
        modelEnabled: !!accountId(), pickerOpen: false, picker: picker.view(),
        efforts, effort: effortValue, effortVisible, service: service(), openCode: false,
      };
    },
    actions: {
      chooseModel(value: string) {
        if (value === modelValue || !models.some((model) => model.value === value)) return;
        modelValue = value;
        updateEfforts();
        updateModelButton();
        h.updateForm();
      },
      chooseEffort(value: string) {
        if (h.agent() !== "kimi" && efforts.some((effort) => effort.value === value)) { effortValue = value; h.updateForm(); }
      },
      openModelPicker() {},
      closeModelPicker() {},
      useNativeModel() {
        modelValue = "";
        effortValue = "";
        updateEfforts();
        updateModelButton();
        h.updateForm();
      },
      pickerClick() {},
      pickerSelect() {},
      pickerToggleAll() {},
      pickerSearch() {},
      startOpenCode() {},
    },
    dispose() { closed = true; ticket++; },
  };
}
