// @vitest-environment happy-dom
import { afterEach, beforeAll, beforeEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
import { Dropdown, type DropdownOption } from "./Dropdown";
// Task #22 (team ede724c4): the first import of a large module graph is
// transformed inside the 5 s test timeout, which the full client run on a
// loaded machine exceeded ("Test timed out in 5000ms" only under that run).
// It is transformed once here, with its own bound; the tests' own imports then
// resolve from the module cache and test exactly what they did.
beforeAll(async () => { await Promise.all([import("../../store/shortcuts"), import("../../perf/dropdown")]); }, 60_000);

let host: HTMLDivElement, dispose: (() => void) | undefined;
const options: DropdownOption[] = [{ value: "a", label: "Alpha" }, { value: "b", label: "Beta", disabled: true }, { value: "g", label: "Gamma" }, { value: "d", label: "Delta" }];
const flush = async () => { for (let i = 0; i < 5; i++) await Promise.resolve(); };
beforeEach(() => { host = document.createElement("div"); document.body.append(host); });
afterEach(() => { dispose?.(); document.body.replaceChildren(); vi.restoreAllMocks(); });
function setup(start = "a", values = options) {
  const [value, setValue] = createSignal(start), [disabled, setDisabled] = createSignal(false), [choices, setChoices] = createSignal(values);
  const changed = vi.fn((v: string) => setValue(v));
  host.innerHTML = '<form><label for="choice">Choose a value</label><div id="mount"></div><button type="button" id="after">After</button></form>';
  dispose = render(() => createComponent(Dropdown, { id: "choice", get options() { return choices(); }, get value() { return value(); }, get disabled() { return disabled(); }, onChange: changed }), host.querySelector("#mount")!);
  const button = host.querySelector<HTMLButtonElement>("#choice")!;
  return { button, value, changed, setValue, setDisabled, setChoices };
}
function key(button: HTMLElement, value: string, extra: KeyboardEventInit = {}) {
  const event = new KeyboardEvent("keydown", { key: value, bubbles: true, cancelable: true, ...extra }); button.dispatchEvent(event); return event;
}
const list = () => host.querySelector<HTMLElement>('[role="listbox"]');
const option = (v: string) => host.querySelector<HTMLButtonElement>(`[role="option"][data-value="${v}"]`)!;
it("exposes the current selection, chooses once and returns focus without submitting the form", async () => {
  const t = setup(); const submitted = vi.fn((e: Event) => e.preventDefault()); host.querySelector("form")!.addEventListener("submit", submitted);
  t.button.focus(); t.button.click(); await flush();
  expect(t.button.getAttribute("role")).toBe("combobox"); expect(t.button.getAttribute("aria-expanded")).toBe("true");
  expect(t.button.getAttribute("aria-controls")).toBe(list()!.id); expect(option("a").getAttribute("aria-selected")).toBe("true");
  option("g").click(); await flush(); expect(t.changed).toHaveBeenCalledExactlyOnceWith("g"); expect(t.value()).toBe("g");
  expect(t.button.textContent).toContain("Gamma"); expect(document.activeElement).toBe(t.button); expect(list()).toBeNull(); expect(submitted).not.toHaveBeenCalled();
  t.button.click(); await flush(); option("g").click(); expect(t.changed).toHaveBeenCalledTimes(1);
});
it("skips disabled choices with closed arrows, and previews open arrows until Enter", async () => {
  const t = setup(); key(t.button, "ArrowDown"); expect(t.value()).toBe("g");
  key(t.button, " "); await flush(); key(t.button, "ArrowDown"); expect(t.value()).toBe("g");
  expect(t.button.getAttribute("aria-activedescendant")).toBe(option("d").id);
  key(t.button, "Enter"); await flush(); expect(t.value()).toBe("d"); expect(list()).toBeNull();
  key(t.button, "Home"); expect(t.value()).toBe("a"); key(t.button, "End"); expect(t.value()).toBe("d");
});
it("Escape cancels only the popup and returns focus, while a closed Escape reaches its dialog", async () => {
  const t = setup(); const parent = vi.fn(); host.addEventListener("keydown", parent);
  t.button.focus(); t.button.click(); await flush(); key(t.button, "ArrowDown");
  expect(key(t.button, "Escape").defaultPrevented).toBe(true); await flush(); expect(t.value()).toBe("a");
  expect(t.changed).not.toHaveBeenCalled(); expect(list()).toBeNull(); expect(document.activeElement).toBe(t.button); expect(parent).not.toHaveBeenCalled();
  expect(key(t.button, "Escape").defaultPrevented).toBe(false); expect(parent).toHaveBeenCalledOnce();
});
it("supports case-insensitive prefix search and repeated-letter cycling", async () => {
  const t = setup("a", [{ value: "a", label: "Apple" }, { value: "p", label: "Apricot" }, { value: "b", label: "Blueberry" }]);
  key(t.button, "a"); expect(t.value()).toBe("p"); key(t.button, "a"); expect(t.value()).toBe("a");
  vi.spyOn(Date, "now").mockReturnValue(Date.now() + 1000); key(t.button, "B"); expect(t.value()).toBe("b");
  t.button.click(); await flush(); key(t.button, "a"); expect(t.value()).toBe("b"); key(t.button, "Enter"); expect(t.value()).toBe("a");
});
it("preserves empty/numeric values and rejects disabled pointer choices", async () => {
  const t = setup("", [{ value: "", label: "Default" }, { value: 2, label: "Two" }, { value: 3, label: "Unavailable", disabled: true }]);
  t.button.click(); await flush(); expect(option("").getAttribute("aria-selected")).toBe("true");
  option("3").click(); expect(t.changed).not.toHaveBeenCalled(); expect(list()).not.toBeNull();
  option("2").click(); expect(t.changed).toHaveBeenCalledExactlyOnceWith("2");
});
it("closes when disabled, on an outside pointer and on unmount without changing the value", async () => {
  const t = setup(); t.button.click(); await flush(); t.setDisabled(true); await flush();
  expect(list()).toBeNull(); expect(t.button.disabled).toBe(true); expect(t.changed).not.toHaveBeenCalled();
  t.setDisabled(false); t.button.click(); await flush();
  const after = host.querySelector<HTMLButtonElement>("#after")!; after.focus(); after.dispatchEvent(new Event("pointerdown", { bubbles: true })); await flush();
  expect(list()).toBeNull(); expect(document.activeElement).toBe(after); expect(t.changed).not.toHaveBeenCalled();
  t.button.click(); await flush(); dispose?.(); dispose = undefined; expect(document.querySelector('[role="listbox"]')).toBeNull();
});
it("commits Tab without preventing the browser from moving focus", async () => {
  const t = setup(); t.button.click(); await flush(); key(t.button, "ArrowDown");
  const e = key(t.button, "Tab"); expect(e.defaultPrevented).toBe(false); expect(t.value()).toBe("g"); await flush(); expect(list()).toBeNull();
});
it("keeps decorated labels in both the trigger and popup and searches their plain text", async () => {
  const t = setup("x", [{ value: "x", label: () => { const fragment = document.createDocumentFragment(), icon = document.createElement("i"), name = document.createElement("b"); icon.textContent = "Icon"; name.textContent = "Decorated"; fragment.append(icon, name); return fragment; }, textValue: "Decorated" }, { value: "y", label: "Other" }]);
  t.button.click(); await flush(); expect(t.button.querySelector("b")?.textContent).toBe("Decorated"); expect(option("x").querySelector("b")?.textContent).toBe("Decorated");
  key(t.button, "End"); key(t.button, "d"); expect(t.button.getAttribute("aria-activedescendant")).toBe(option("x").id);
});
it("clamps a long popup inside the viewport and opens above a low trigger", async () => {
  const t = setup("0", Array.from({ length: 100 }, (_, i) => ({ value: String(i), label: `Option ${i}` }))); vi.spyOn(t.button, "getBoundingClientRect").mockReturnValue({ left: 700, top: 550, bottom: 580, width: 200, height: 30, right: 900, x: 700, y: 550, toJSON() {} });
  vi.stubGlobal("innerWidth", 800); vi.stubGlobal("innerHeight", 600);
  const original = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "offsetWidth");
  Object.defineProperty(HTMLElement.prototype, "offsetWidth", { configurable: true, get: () => 240 });
  const height = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "offsetHeight");
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", { configurable: true, get: () => 320 });
  try { t.button.click(); await flush(); expect(list()!.style.left).toBe("552px"); expect(list()!.style.top).toBe("226px"); expect(list()!.style.maxHeight).toBe("320px"); }
  finally { if (original) Object.defineProperty(HTMLElement.prototype,"offsetWidth",original); else delete (HTMLElement.prototype as unknown as Record<string, unknown>).offsetWidth; if(height)Object.defineProperty(HTMLElement.prototype,"offsetHeight",height);else delete (HTMLElement.prototype as unknown as Record<string, unknown>).offsetHeight; vi.unstubAllGlobals(); }
});


it("uses the real perf selection helper without a native change event", async () => {
  const { chooseDropdown } = await import("../../perf/dropdown");
  const t = setup(); chooseDropdown(t.button, "g"); await flush();
  expect(t.changed).toHaveBeenCalledExactlyOnceWith("g"); expect(t.value()).toBe("g"); expect(list()).toBeNull();
});
it("keeps the popup in a native top-layer dialog when the Popover API is unavailable", async () => {
  const previous = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "showPopover");
  Object.defineProperty(HTMLElement.prototype, "showPopover", { configurable: true, value: undefined });
  try { const t = setup(); t.button.click(); await flush(); expect(host.querySelector<HTMLDialogElement>(".dropdown-layer")?.open).toBe(true);
    expect(list()?.closest("dialog")).not.toBeNull(); key(list()!, "ArrowDown"); key(list()!, "Enter"); await flush();
    expect(t.value()).toBe("g"); expect(document.activeElement).toBe(t.button); expect(list()).toBeNull();
  } finally { if (previous) Object.defineProperty(HTMLElement.prototype,"showPopover",previous); else delete (HTMLElement.prototype as unknown as Record<string,unknown>).showPopover; }
});


it("focuses a pointer-opened combobox even when the browser does not focus clicked buttons", async () => {
  const previous = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "showPopover");
  Object.defineProperty(HTMLElement.prototype, "showPopover", { configurable: true, value() {} });
  try { const t = setup(); host.querySelector<HTMLButtonElement>("#after")!.focus(); t.button.click(); await flush();
    expect(document.activeElement).toBe(t.button); expect(t.button.getAttribute("aria-expanded")).toBe("true");
  } finally { if (previous) Object.defineProperty(HTMLElement.prototype,"showPopover",previous); else delete (HTMLElement.prototype as unknown as Record<string,unknown>).showPopover; }
});


it("preserves text-field shortcut handling while closed and suspends commands only while open", async () => {
  const { contextOf, popupOpen } = await import("../../store/shortcuts");
  const t = setup(); expect(popupOpen(document)).toBe(false); expect(contextOf(t.button)).toBe("text-field");
  t.button.click(); await flush(); expect(popupOpen(document)).toBe(true); expect(contextOf(t.button)).toBe("none");
  key(t.button,"Escape"); await flush(); expect(popupOpen(document)).toBe(false); expect(contextOf(t.button)).toBe("text-field");
});
