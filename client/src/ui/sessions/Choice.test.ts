// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
import { Choice } from "./NewSessionDialog";

// Task #433: every dropdown of the New session dialog opens, lists its options,
// picks by mouse and keyboard and closes. Choice is the dialog's own listbox
// (project, computer, folder); the model, account and effort controls have
// their own tests, and Dropdown.test.ts covers the shared one.
let host: HTMLDivElement, dispose: (() => void) | undefined;
const flush = async () => { for (let i = 0; i < 5; i++) await Promise.resolve(); };
afterEach(() => { dispose?.(); document.body.replaceChildren(); vi.restoreAllMocks(); });

function setup() {
  host = document.createElement("div");
  document.body.append(host);
  const [open, setOpen] = createSignal(false);
  const picked = vi.fn();
  dispose = render(() => createComponent(Choice, {
    id: "choice", label: "Project", keys: ["a", "b", "c"], selected: "a", disabled: false,
    get open() { return open(); }, onOpen: (next: boolean) => setOpen(next), onPick: picked,
    option: (key: string) => `Option ${key}`, dense: true, header: "Project", check: true,
  }), host);
  const button = host.querySelector<HTMLButtonElement>('[role="combobox"]')!;
  return { button, picked, open, setOpen };
}

it("opens by mouse, lists every option, picks one and closes", async () => {
  const t = setup();
  t.button.click(); await flush();
  expect(t.open()).toBe(true);
  const list = host.querySelector('[role="listbox"]');
  expect(list).not.toBeNull();
  expect(list!.querySelectorAll('[role="option"]')).toHaveLength(3);
  expect(list!.textContent).toContain("Option a");
  (list!.querySelector('[data-key="b"]') as HTMLElement).click(); await flush();
  expect(t.picked).toHaveBeenCalledExactlyOnceWith("b");
  expect(t.open()).toBe(false);
  expect(host.querySelector('[role="listbox"]')).toBeNull();
});

it("opens and picks with the keyboard, and leaves Escape to the dialog", async () => {
  const t = setup();
  const key = (name: string, extra: KeyboardEventInit = {}) => {
    const e = new KeyboardEvent("keydown", { key: name, bubbles: true, cancelable: true, ...extra });
    t.button.dispatchEvent(e);
    return e;
  };
  key("ArrowDown", { altKey: true }); await flush();
  expect(t.open()).toBe(true);
  key("ArrowDown"); await flush();
  key("Enter"); await flush();
  expect(t.picked).toHaveBeenCalledTimes(1);
  expect(t.picked).toHaveBeenCalledWith("b");
  // Escape closes an open list and keeps the value.
  t.button.click(); await flush();
  key("Escape"); await flush();
  // Escape stays native (the dialog\u0027s cancel closes the list); it never picks.
  expect(t.picked).toHaveBeenCalledTimes(1);
});

it("does not open when disabled and closes on an outside press", async () => {
  const t = setup();
  t.setOpen(true); await flush();
  expect(host.querySelector('[role="listbox"]')).not.toBeNull();
  document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true })); await flush();
  expect(t.open()).toBe(false);
  expect(t.picked).not.toHaveBeenCalled();
});
