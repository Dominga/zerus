// @vitest-environment happy-dom
import { afterEach, beforeAll, expect, it } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import { OutgoingMessage } from "./Activity";
import { loadMarkdown } from "./markdown";

let dispose: (() => void) | undefined;
beforeAll(loadMarkdown);
afterEach(() => { dispose?.(); document.body.replaceChildren(); });
const mount = (body: string, bodyTruncated = false) => {
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(OutgoingMessage, { item: { id: "own", kind: "user", at: "", body, bodyTruncated } }), host);
  return host;
};
it("shows a 1670-character own message completely", () => {
  const text = "A".repeat(1656) + "TRAILING WORDS";
  expect(mount(text).textContent).toContain(text);
});
it("explicitly expands and collapses a large own message", () => {
  const host = mount("A".repeat(8000) + "TRAILING WORDS");
  expect(host.textContent).not.toContain("TRAILING WORDS");
  const button = host.querySelector("button")!;
  expect(button?.textContent).toBe("Show more");
  expect(button?.getAttribute("aria-expanded")).toBe("false");
  button.click();
  expect(host.textContent).toContain("TRAILING WORDS");
  expect(button.getAttribute("aria-expanded")).toBe("true");
  button.click();
  expect(host.textContent).not.toContain("TRAILING WORDS");
});
it("labels an unavailable remainder instead of silently presenting a clipped message as complete", () => {
  expect(mount("Old preview…", true).textContent).toContain("Only part of this message was recorded. Open Terminal to read the complete message.");
});
it("does not split a Unicode character or offer needless expansion", () => {
  const host = mount("😀".repeat(3000) + "TAIL");
  expect(host.textContent).toContain("TAIL");
  expect(host.querySelector("button")).toBeNull();
});
