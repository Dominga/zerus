// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";

const fixture = vi.hoisted(() => ({
  canAttach: vi.fn(() => true), image: vi.fn(async (_file: Blob) => {}), send: vi.fn(),
  paste: vi.fn(), input: vi.fn(), show: vi.fn(), hide: vi.fn(),
}));
vi.mock("../../store/composer", () => ({ composer: { canAttach: fixture.canAttach, pasteImage: fixture.image, send: fixture.send } }));
vi.mock("../../store/terminal", () => ({
  screenShowsLatest: () => true,
  terminal: { paste: fixture.paste, input: fixture.input, show: fixture.show, hide: fixture.hide,
    connected: () => false, ready: () => false, texts: () => undefined, status: () => "Fixture terminal", resized() {} },
}));
// Only xterm's canvas/OS renderer is inert. The actual tab and capture listener run.
vi.mock("@xterm/xterm", () => ({ Terminal: class {
  cols = 80; rows = 24; options: Record<string, unknown>; textarea?: HTMLTextAreaElement;
  constructor(options: Record<string, unknown>) { this.options = options; }
  loadAddon() {} onData() {} onBinary() {} onResize() {} write(_bytes: unknown, done?: () => void) { done?.(); }
  open(host: HTMLElement) { this.textarea = document.createElement("textarea"); host.append(this.textarea); }
  focus() { this.textarea?.focus(); } dispose() { this.textarea?.remove(); } reset() {} paste() {}
} }));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: class { fit() {} } }));

import { TerminalTab } from "./Terminal";
import { detailTab, setDetailTab } from "../../store/ui";

let dispose: (() => void) | undefined, host: HTMLDivElement;
const settle = async () => { for (let i = 0; i < 20; i++) await Promise.resolve(); };
beforeEach(async () => {
  vi.clearAllMocks(); fixture.canAttach.mockReturnValue(true);
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  setDetailTab("terminal");
  host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(TerminalTab, {}), host);
  await settle();
});
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); vi.unstubAllGlobals(); });

function paste(data: Partial<DataTransfer>, target = host.querySelector(".term-host")!) {
  const event = new Event("paste", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "clipboardData", { value: { items: [], files: [], types: [], getData: () => "", ...data } });
  target.dispatchEvent(event);
  return event;
}
const image = () => new File(["synthetic image fixture"], "clipboard.png", { type: "image/png" });

it("pastes an image into the existing draft once and opens Activity without sending or terminal input", () => {
  const file = image(), getAsFile = vi.fn(() => file), bubbled = vi.fn();
  document.addEventListener("paste", bubbled, { once: true });
  try {
    const event = paste({ items: [{ kind: "file", type: file.type, getAsFile }] as unknown as DataTransferItemList,
      getData: () => "text alongside the screenshot must not be typed" });
    expect(event.defaultPrevented).toBe(true);
    expect(getAsFile).toHaveBeenCalledTimes(1);
    expect(fixture.image).toHaveBeenCalledExactlyOnceWith(file);
    expect(detailTab()).toBe("activity");
    expect(fixture.paste).not.toHaveBeenCalled(); expect(fixture.input).not.toHaveBeenCalled();
    expect(fixture.send).not.toHaveBeenCalled(); expect(bubbled).not.toHaveBeenCalled();
  } finally { document.removeEventListener("paste", bubbled); }
});

it("also recognizes an image exposed only as a clipboard file", () => {
  const file = image();
  paste({ files: [file] as unknown as FileList });
  expect(fixture.image).toHaveBeenCalledExactlyOnceWith(file);
  expect(detailTab()).toBe("activity"); expect(fixture.paste).not.toHaveBeenCalled();
});

it("shows an actionable fallback if the composer cannot accept attachments", () => {
  fixture.canAttach.mockReturnValue(false);
  paste({ files: [image()] as unknown as FileList, getData: () => "do not paste this fallback text" });
  expect(detailTab()).toBe("terminal");
  expect(host.querySelector('[role="status"]')?.textContent).toContain("paste again when the message field is ready");
  expect(fixture.image).not.toHaveBeenCalled(); expect(fixture.paste).not.toHaveBeenCalled();
  const button = [...host.querySelectorAll("button")].find((button) => button.textContent === "Open Activity")!;
  button.click(); expect(detailTab()).toBe("activity"); expect(fixture.send).not.toHaveBeenCalled();
});

it("reports image clipboard data that cannot be read, instead of pasting its accompanying text", () => {
  paste({ items: [{ kind: "file", type: "image/png", getAsFile: () => null }] as unknown as DataTransferItemList, getData: () => "not image bytes" });
  expect(host.querySelector('[role="status"]')).not.toBeNull();
  expect(fixture.image).not.toHaveBeenCalled(); expect(fixture.paste).not.toHaveBeenCalled();
  host.querySelector<HTMLButtonElement>('[aria-label="Dismiss"]')!.click();
  expect(host.querySelector('[role="status"]')).toBeNull();
});

it("keeps text-only paste on the original terminal path", () => {
  const text = "plain\r\ntext";
  expect(paste({ getData: (type) => type === "text/plain" ? text : "" }).defaultPrevented).toBe(true);
  expect(fixture.paste).toHaveBeenCalledExactlyOnceWith(text);
  expect(fixture.image).not.toHaveBeenCalled(); expect(detailTab()).toBe("terminal");
});

it("removes the image paste listener with the original tab", () => {
  const old = host.querySelector(".term-host")!;
  dispose!(); dispose = undefined;
  paste({ files: [image()] as unknown as FileList }, old);
  expect(fixture.image).not.toHaveBeenCalled(); expect(fixture.paste).not.toHaveBeenCalled();
});
