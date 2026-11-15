// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it } from "vitest";
import { installNativeMenuGuard } from "./native-menu";

let remove: () => void;
beforeEach(() => {
  document.body.innerHTML = `<div id="plain">Plain text</div><input id="field" value="x"><textarea id="area"></textarea><div id="edit" contenteditable="true">e</div><input id="box" type="checkbox"><div id="own">own menu</div>`;
  remove = installNativeMenuGuard(window);
});
afterEach(() => {
  remove();
  window.getSelection()?.removeAllRanges();
});

const rightClick = (id: string) => {
  const event = new MouseEvent("contextmenu", { bubbles: true, cancelable: true, button: 2 });
  document.getElementById(id)!.dispatchEvent(event);
  return event.defaultPrevented;
};

it("closes the webview menu on ordinary app surfaces and non-text controls", () => {
  expect(rightClick("plain")).toBe(true);
  expect(rightClick("box")).toBe(true);
});

it("keeps the native menu for editable text", () => {
  expect(rightClick("field")).toBe(false);
  expect(rightClick("area")).toBe(false);
  expect(rightClick("edit")).toBe(false);
});

it("keeps the native menu while text is selected so Copy works", () => {
  const range = document.createRange();
  range.selectNodeContents(document.getElementById("plain")!);
  window.getSelection()!.addRange(range);
  expect(rightClick("plain")).toBe(false);
});

it("leaves Zerus menus that already handled the event alone", () => {
  const own = document.getElementById("own")!;
  own.addEventListener("contextmenu", (event) => event.preventDefault());
  expect(rightClick("own")).toBe(true);
});
