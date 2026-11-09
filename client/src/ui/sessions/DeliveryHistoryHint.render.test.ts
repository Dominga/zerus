// @vitest-environment happy-dom
import { afterEach, expect, it } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
import { DeliveryHistoryHint } from "./DeliveryHistoryHint";
import { translateWithCatalog } from "../../i18n";
import russian from "../../i18n.ru.json";
import type { ComposerView } from "../../types";

const source = "This text may already have been delivered";
const snapshot = (extra: Partial<ComposerView> = {}) => ({ text: "exact draft", uncertain: true, history_text_match: true, ...extra }) as ComposerView;
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); document.body.replaceChildren(); });
function setup(view = snapshot()) {
  const [native, setNative] = createSignal<ComposerView | undefined>(view);
  const [text, setText] = createSignal(view.text), [shown, setShown] = createSignal(true);
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(DeliveryHistoryHint, { get view() { return native(); }, get text() { return text(); }, get shown() { return shown(); } }), host);
  return { host, setNative, setText, setShown };
}
it("shows a quiet advisory while preserving the uncertain view", () => {
  const view = snapshot(); const t = setup(view);
  expect(t.host.querySelector('[role="status"]')?.textContent).toBe(source);
  expect(t.host.querySelector('[role="alert"]')).toBeNull();
  expect(view).toMatchObject({ text: "exact draft", uncertain: true, history_text_match: true });
  expect(t.host.querySelector("button")).toBeNull();
});
it("hides immediately on a local edit while the native snapshot is still old", () => {
  const t = setup(); t.setText("edited"); expect(t.host.textContent).toBe("");
  t.setNative(snapshot({ text: "edited", history_text_match: false })); expect(t.host.textContent).toBe("");
  t.setNative(snapshot({ text: "edited" })); expect(t.host.textContent).toBe(source);
});
it("hides after Allow retry, when input is hidden, or for an older host without the advisory", () => {
  const t = setup(); t.setNative(snapshot({ uncertain: false })); expect(t.host.textContent).toBe("");
  t.setNative(snapshot()); t.setShown(false); expect(t.host.textContent).toBe("");
  t.setShown(true); t.setNative(snapshot({ history_text_match: undefined })); expect(t.host.textContent).toBe("");
});
it("has the requested English and Russian wording", () => {
  expect(translateWithCatalog(russian, "en", source)).toBe(source);
  expect(translateWithCatalog(russian, "ru", source)).toBe("Возможно, этот текст уже был доставлен");
});
