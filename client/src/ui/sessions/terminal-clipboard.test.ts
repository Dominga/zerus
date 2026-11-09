import { afterEach, describe, expect, it, vi } from "vitest";
import { clipboardKeyHandler, copyKey, IMAGE_BYTES, normalizeTerminalImage, terminalMenu } from "./terminal-clipboard";

afterEach(() => vi.unstubAllGlobals());

const png = (width = 2, height = 3) => {
  const bytes = Uint8Array.from([137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 0, 0, 0, 0, 0]);
  const view = new DataView(bytes.buffer); view.setUint32(16, width); view.setUint32(20, height); return bytes;
};

describe("the event's bounded image data", () => {
  it("preserves PNG bytes exactly without reading an ambient clipboard or decoding", async () => {
    const decode = vi.fn(); vi.stubGlobal("createImageBitmap", decode);
    const bytes = png(); expect(await normalizeTerminalImage(new Blob([bytes], { type: "image/png" }))).toEqual(bytes);
    expect(decode).not.toHaveBeenCalled();
  });
  it("refuses an overbound input before reading or decoding it, and rejects malformed dimensions", async () => {
    const source = new Blob(["x"], { type: "image/png" });
    Object.defineProperty(source, "size", { value: IMAGE_BYTES + 1 });
    const read = vi.spyOn(source, "arrayBuffer");
    await expect(normalizeTerminalImage(source)).rejects.toThrow("10 MiB"); expect(read).not.toHaveBeenCalled();
    for (const bytes of [new Uint8Array(24), png(0), png(8000, 8000)]) {
      await expect(normalizeTerminalImage(new Blob([bytes], { type: "image/png" }))).rejects.toThrow();
    }
  });
  it("converts one bounded non-PNG bitmap and closes its original resource", async () => {
    const bitmap = { width: 2, height: 3, close: vi.fn() }, draw = vi.fn(), bytes = png();
    const decode = vi.fn(async () => bitmap); vi.stubGlobal("createImageBitmap", decode);
    vi.stubGlobal("OffscreenCanvas", class { getContext() { return { drawImage: draw }; } async convertToBlob() { return new Blob([bytes]); } });
    expect(await normalizeTerminalImage(new Blob(["jpeg"], { type: "image/jpeg" }))).toEqual(bytes);
    expect(decode).toHaveBeenCalledTimes(1); expect(draw).toHaveBeenCalledExactlyOnceWith(bitmap, 0, 0); expect(bitmap.close).toHaveBeenCalledTimes(1);
  });
  it("closes failed conversions and refuses excessive output without reading it", async () => {
    const bitmap = { width: 2, height: 3, close: vi.fn() }; vi.stubGlobal("createImageBitmap", async () => bitmap);
    const output = new Blob([png()]); Object.defineProperty(output, "size", { value: IMAGE_BYTES + 1 }); const read = vi.spyOn(output, "arrayBuffer");
    vi.stubGlobal("OffscreenCanvas", class { getContext() { return { drawImage() {} }; } async convertToBlob() { return output; } });
    await expect(normalizeTerminalImage(new Blob(["jpeg"], { type: "image/jpeg" }))).rejects.toThrow();
    expect(read).not.toHaveBeenCalled(); expect(bitmap.close).toHaveBeenCalledTimes(1);
  });
});

const key = (key: string, code: string, mods: Partial<Record<"ctrlKey" | "metaKey" | "altKey" | "shiftKey", boolean>> = {}) =>
  ({ key, code, ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, ...mods });

describe("Terminal's copy key (Qt's copyKey)", () => {
  it("is Ctrl+Shift+C on Linux and Windows and leaves Ctrl+C to the agent", () => {
    for (const on of ["linux", "windows"] as const) {
      expect(copyKey(key("C", "KeyC", { ctrlKey: true, shiftKey: true }), on)).toBe(true);
      expect(copyKey(key("c", "KeyC", { ctrlKey: true }), on)).toBe(false);
      expect(copyKey(key("C", "KeyC", { ctrlKey: true, shiftKey: true, altKey: true }), on)).toBe(false);
      expect(copyKey(key("V", "KeyV", { ctrlKey: true, shiftKey: true }), on)).toBe(false);
    }
  });

  it("is Command-C on macOS and leaves Control to the agent", () => {
    expect(copyKey(key("c", "KeyC", { metaKey: true }), "macos")).toBe(true);
    expect(copyKey(key("c", "KeyC", { ctrlKey: true }), "macos")).toBe(false);
    expect(copyKey(key("C", "KeyC", { metaKey: true, shiftKey: true }), "macos")).toBe(false);
  });

  it("reads the layout's letter, and the key position only for another script", () => {
    expect(copyKey(key("С", "KeyC", { ctrlKey: true, shiftKey: true }), "linux")).toBe(true);
    // Dvorak: the key that types J sits where QWERTY has C.
    expect(copyKey(key("J", "KeyC", { ctrlKey: true, shiftKey: true }), "linux")).toBe(false);
    expect(copyKey(key("C", "KeyI", { ctrlKey: true, shiftKey: true }), "linux")).toBe(true);
  });
});

describe("xterm's key handler", () => {
  const event = (type: string, ...init: Parameters<typeof key>) =>
    ({ ...key(...init), type, preventDefault: vi.fn() }) as unknown as KeyboardEvent & { preventDefault: ReturnType<typeof vi.fn> };

  it("copies once on keydown and keeps the copy key from the agent", () => {
    const copy = vi.fn();
    const handle = clipboardKeyHandler("linux", copy);
    const down = event("keydown", "C", "KeyC", { ctrlKey: true, shiftKey: true });
    expect(handle(down)).toBe(false);
    expect(down.preventDefault).toHaveBeenCalled();
    expect(handle(event("keyup", "C", "KeyC", { ctrlKey: true, shiftKey: true }))).toBe(false);
    expect(copy).toHaveBeenCalledTimes(1);
  });

  it("leaves Ctrl+C, Ctrl+V and the paste keys to xterm and the engine", () => {
    const copy = vi.fn();
    const handle = clipboardKeyHandler("linux", copy);
    for (const [k, code, mods] of [["c", "KeyC", { ctrlKey: true }], ["v", "KeyV", { ctrlKey: true }], ["V", "KeyV", { ctrlKey: true, shiftKey: true }],
      ["Insert", "Insert", { shiftKey: true }]] as const) {
      const down = event("keydown", k, code, mods);
      expect(handle(down)).toBe(true);
      expect(down.preventDefault).not.toHaveBeenCalled();
    }
    expect(copy).not.toHaveBeenCalled();
  });
});

describe("Terminal's right-click menu (Qt's contextMenuEvent)", () => {
  it("offers Copy selection only with a selection, and Paste", () => {
    const copy = vi.fn(), paste = vi.fn();
    const items = terminalMenu({ selection: true, mouseApp: false, shift: false, on: "linux" }, copy, paste)!;
    expect(items.map((item) => "label" in item && [item.label, item.kbd, !!item.disabled])).toEqual([
      ["Copy selection", "Ctrl+Shift+C", false], ["Paste", "Ctrl+Shift+V", false]]);
    for (const item of items) if ("action" in item) item.action?.();
    expect(copy).toHaveBeenCalledTimes(1);
    expect(paste).toHaveBeenCalledTimes(1);
    const empty = terminalMenu({ selection: false, mouseApp: false, shift: false, on: "macos" }, copy, paste)!;
    expect(empty[0]).toMatchObject({ label: "Copy selection", kbd: "⌘C", disabled: true, action: undefined });
    expect(empty[1]).toMatchObject({ label: "Paste", kbd: "⌘V" });
  });

  it("leaves the click to an agent that reads the mouse, unless Shift is held", () => {
    expect(terminalMenu({ selection: true, mouseApp: true, shift: false, on: "linux" }, vi.fn(), vi.fn())).toBeUndefined();
    expect(terminalMenu({ selection: true, mouseApp: true, shift: true, on: "linux" }, vi.fn(), vi.fn())).toHaveLength(2);
  });
});
