// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
const port = vi.hoisted(() => ({ extra: (): Record<string, unknown> => ({}), opened: (): boolean => false, open: () => {}, close: () => {}, model: (): string => "first", chooseModel: (_value: string) => {}, effort: (): string => "low", chooseEffort: (_value: string) => {}, apply: vi.fn() }));
vi.mock("../../store/sessionModels", () => ({ sessionModels: {
  view: () => ({ accessible: "Model and effort", tooltip: "Private fixture", shownModel: "First", suffix: "", enabled: true, terminal: true, openCode: false, hint: "Private fixture", changeNotice: "", applyLabel: "Apply", ...port.extra() }),
  opened: () => port.opened(), open: () => port.open(), close: () => port.close(), model: () => port.model(), chooseModel: (v: string) => port.chooseModel(v),
  effort: () => port.effort(), chooseEffort: (v: string) => port.chooseEffort(v), options: () => [{ id: "first", label: "First" }, { id: "second", label: "Second" }],
  efforts: () => [{ value: "low", label: "Low" }, { value: "high", label: "High" }], catalogError: () => "", canApply: () => true, apply: port.apply,
} }));
vi.mock("../../store/terminal", () => ({ terminal: { opened: vi.fn() } }));
vi.mock("../../store/ui", () => ({ setDetailTab: vi.fn() }));
import { ModelSettingsButton } from "./ModelSettings";
let host: HTMLDivElement, dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); document.body.replaceChildren(); vi.clearAllMocks(); port.effort = () => "low"; port.chooseEffort = () => {}; port.extra = () => ({}); });
function setup() {
  const [opened, setOpened] = createSignal(false), [model, setModel] = createSignal("first");
  port.opened = opened; port.open = () => setOpened(true); port.close = () => setOpened(false); port.model = model; port.chooseModel = setModel;
  host = document.createElement("div"); document.body.append(host); dispose = render(() => createComponent(ModelSettingsButton, {}), host);
  return host.querySelector<HTMLButtonElement>(".cmp-model")!;
}
const flush = async () => { await vi.dynamicImportSettled(); for (let i = 0; i < 5; i++) await Promise.resolve(); };
it("lets a fresh Claude session choose an effort while its current effort is unknown", async () => {
  const [effort, setEffort] = createSignal(""); port.effort = effort; port.chooseEffort = setEffort;
  const button = setup(); button.click(); await flush();
  const dialog = host.querySelector<HTMLDialogElement>("dialog.cmp-settings-dialog")!;
  const field = dialog.querySelector<HTMLButtonElement>("#session-effort-choice")!;
  expect(field.disabled).toBe(false);
  field.click(); await flush();
  dialog.querySelector<HTMLButtonElement>('[role="option"][data-value="high"]')!.click();
  expect(effort()).toBe("high"); expect(port.apply).not.toHaveBeenCalled();
});
it("Escape cancels the opening before the lazy dialog resolves, keeping the session control", async () => {
  const button = setup(); button.click(); expect(port.opened()).toBe(true);
  button.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
  await flush(); expect(port.opened()).toBe(false); expect(host.querySelector("dialog")).toBeNull(); expect(host.querySelector(".cmp-model")).toBe(button); expect(port.apply).not.toHaveBeenCalled();
});
it("loads the existing dialog, focuses its themed field and returns to the button without applying", async () => {
  const button = setup(); button.click(); await flush();
  const dialog = host.querySelector<HTMLDialogElement>("dialog.cmp-settings-dialog")!; expect(dialog.open).toBe(true);
  const field = dialog.querySelector<HTMLButtonElement>("#session-model-choice")!; expect(document.activeElement).toBe(field);
  field.click(); await flush(); dialog.querySelector<HTMLButtonElement>('[role="option"][data-value="second"]')!.click();
  expect(port.model()).toBe("second"); expect(port.apply).not.toHaveBeenCalled();
  [...dialog.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Cancel")!.click(); await flush();
  expect(host.querySelector("dialog")).toBeNull(); expect(document.activeElement).toBe(button); expect(port.apply).not.toHaveBeenCalled();
});
it("marks a DeepSeek session whose account defaults were not applied, beside its selector (zerus-fw3g5)", async () => {
  expect(setup().querySelector(".cmp-model-warn")).toBeNull(); dispose?.(); document.body.replaceChildren();
  port.extra = () => ({ defaults: { status: "failed", title: "Account defaults were not applied", text: "Private fixture text",
    choice: "Account default: route/model (max)", detail: "Private fixture reason" } });
  const button = setup(); expect(button.querySelector(".cmp-model-warn")).not.toBeNull();
  button.click(); await flush();
  const notice = host.querySelector<HTMLElement>("dialog.cmp-settings-dialog .cmp-settings-defaults")!;
  expect(notice.textContent).toBe("Account defaults were not appliedPrivate fixture textAccount default: route/model (max)Private fixture reason");
  expect(notice.querySelector(".cmp-settings-defaults-text")?.textContent).toContain("Private fixture reason");
  expect(port.apply).not.toHaveBeenCalled();
});
