import { describe, expect, it } from "vitest";
import native from "./shortcuts-native.json";
import { setSetting, settings } from "./settings";
import {
  COMMANDS, allowed, assignable, candidates, comboFromEvent, conflicts, contextOf, customized, formatSequence, handlerFor, keys,
  nativeText, onCommand, parseChord, parseSequence, popupOpen, portableDefaults, protectedInTextField, reset, resetAll,
  route, sequenceState, setKeys, terminalSafe, type Combo, type Platform,
} from "./shortcuts";

// shortcuts-native.json was recorded by Qt 6.12 itself on Linux and on macOS
// (client/scripts/record-key-sequences.cpp): each portable chord with
// QKeySequence's NativeText and PortableText.

const seq = (text: string) => parseSequence(text)!;
const combo = (text: string) => parseChord(text)!;
const key = (init: Partial<KeyboardEvent> & { key: string; code: string }) =>
  ({ altKey: false, ctrlKey: false, metaKey: false, shiftKey: false, isComposing: false, defaultPrevented: false, keyCode: 0, target: null, ...init }) as KeyboardEvent;
const ctrl = (letter: string, init: Partial<KeyboardEvent> = {}) =>
  key({ key: letter.toLowerCase(), code: /[0-9]/.test(letter) ? `Digit${letter}` : `Key${letter}`, ctrlKey: true, ...init });

/** A focus target as the router reads it: a mark, a tag, xterm, a dialog, a selection. */
function focus(o: { marked?: string; tag?: string; xterm?: boolean; dialog?: boolean; selection?: [number, number] } = {}): EventTarget {
  const found = (yes: boolean | undefined, attribute: string | null = null) => (yes ? { getAttribute: () => attribute } : null);
  return {
    closest: (selector: string) =>
      selector === "dialog[open]" ? found(o.dialog)
      : selector === "[data-shortcut-context]" ? found(!!o.marked, o.marked ?? null)
      : selector === ".xterm" ? found(o.xterm) : null,
    matches: (selector: string) => !!o.tag && selector.split(", ").includes(o.tag.toLowerCase()),
    tagName: o.tag,
    selectionStart: o.selection?.[0],
    selectionEnd: o.selection?.[1],
  } as unknown as EventTarget;
}

describe("chords as Qt writes them", () => {
  it.each(native.linux)("parses and writes $portable in portable text", (row) => {
    expect(formatSequence(seq(row.portable))).toBe(row.roundtrip);
    expect(seq(row.portable)).toHaveLength(row.count);
  });
  it.each(native.linux)("shows $portable as Qt does on Linux", (row) => {
    expect(nativeText(seq(row.portable), "linux")).toBe(row.native);
  });
  it.each(native.macos)("shows $portable as Qt does on macOS", (row) => {
    expect(nativeText(seq(row.portable), "macos")).toBe(row.native);
  });
});

describe("commands and defaults (Shortcuts.cpp)", () => {
  it("keeps Qt's 28 commands in Qt's order", () => {
    expect(COMMANDS).toHaveLength(28);
    expect(COMMANDS[0].id).toBe("go.overview");
    expect(COMMANDS.filter((c) => c.scope === "composer").map((c) => c.id)).toEqual(["message.send", "message.push"]);
  });
  it("uses physical Control for tabs and the push sequence on macOS", () => {
    expect(portableDefaults("tab.next", "macos")).toEqual(["Meta+Tab"]);
    expect(portableDefaults("tab.next", "linux")).toEqual(["Ctrl+Tab"]);
    expect(portableDefaults("message.push", "macos")).toEqual(["Meta+X, Meta+S"]);
    expect(portableDefaults("session.next", "windows")).toEqual(["Alt+Down", "Ctrl+PgDown"]);
    for (const c of COMMANDS) for (const p of ["linux", "macos", "windows"] as Platform[])
      for (const chord of portableDefaults(c.id, p)) expect(parseSequence(chord), `${c.id} ${chord}`).toBeDefined();
  });
});

describe("rules (Shortcuts.cpp)", () => {
  it("refuses what Qt refuses, with Qt's reasons", () => {
    expect(assignable([], "linux")).toBe("Press a key combination.");
    expect(assignable(seq("Ctrl+A, Ctrl+B, Ctrl+C"), "linux")).toBe("Use one key combination or a sequence of two.");
    expect(assignable(seq("Shift+F10"), "linux")).toBe("This key opens context menus.");
    expect(assignable([{ meta: false, ctrl: true, alt: false, shift: false, key: "Shift" }], "linux")).toBe("Press a key together with the modifiers.");
    expect(assignable(seq("K"), "linux")).toBe("Keys without Ctrl or Alt are used for typing and navigation.");
    expect(assignable(seq("K"), "macos")).toBe("Keys without ⌘, ⌃ or ⌥ are used for typing and navigation.");
    expect(assignable(seq("Ctrl+Shift+C"), "linux")).toBe("Terminal uses this key to copy and paste.");
    expect(assignable(seq("Ctrl+Shift+C"), "macos")).toBeUndefined();
    expect(assignable(seq("Ctrl+C"), "linux")).toBe("Text fields use this key for editing.");
    expect(assignable(seq("Ctrl+Left"), "linux")).toBe("Text fields use this key for editing.");
    // Cut may start a sequence: without a selection it does nothing.
    expect(assignable(seq("Ctrl+X, Ctrl+S"), "linux")).toBeUndefined();
    expect(assignable(seq("Ctrl+S, Ctrl+X"), "linux")).toBe("Text fields use this key for editing.");
    expect(assignable(seq("F5"), "linux")).toBeUndefined();
  });
  it("lets only Qt's chords leave Terminal", () => {
    expect(terminalSafe(combo("Ctrl+1"), "linux")).toBe(true);
    expect(terminalSafe(combo("Ctrl+Shift+Return"), "linux")).toBe(true);
    expect(terminalSafe(combo("Ctrl+Return"), "linux")).toBe(false);
    expect(terminalSafe(combo("Ctrl+K"), "linux")).toBe(false);
    expect(terminalSafe(combo("Ctrl+K"), "macos")).toBe(true);
    expect(terminalSafe(combo("Ctrl+C"), "macos")).toBe(false);
    expect(terminalSafe(combo("Meta+Shift+Tab"), "macos")).toBe(true);
    expect(terminalSafe(combo("Meta+X"), "macos")).toBe(false);
  });
  it("keeps editing keys in text fields", () => {
    expect(protectedInTextField(combo("Ctrl+C"))).toBe(true);
    expect(protectedInTextField(combo("Return"))).toBe(true);
    expect(protectedInTextField(combo("F5"))).toBe(false);
    expect(protectedInTextField(combo("Ctrl+K"))).toBe(false);
  });
  it("routes by focus like ShortcutRouter", () => {
    expect(allowed("terminal", combo("Ctrl+K"), seq("Ctrl+K"), false, "linux")).toBe(false);
    expect(allowed("terminal", combo("Ctrl+1"), seq("Ctrl+1"), false, "linux")).toBe(true);
    expect(allowed("text-field", combo("Ctrl+X"), seq("Ctrl+X, Ctrl+S"), false, "linux")).toBe(true);
    expect(allowed("text-field", combo("Ctrl+X"), seq("Ctrl+X, Ctrl+S"), true, "linux")).toBe(false);
    expect(allowed("composer", combo("Ctrl+C"), seq("Ctrl+C"), false, "linux")).toBe(false);
    expect(allowed("window", combo("Return"), seq("Return"), false, "linux")).toBe(true);
  });
});

describe("key events", () => {
  it("maps Command to Qt's Ctrl and Control to Meta on macOS", () => {
    const e = key({ key: "k", code: "KeyK", metaKey: true });
    expect(comboFromEvent(e, "macos")).toEqual<Combo>({ meta: false, ctrl: true, alt: false, shift: false, key: "K" });
    expect(comboFromEvent(key({ key: "Tab", code: "Tab", ctrlKey: true }), "macos")).toEqual<Combo>({ meta: true, ctrl: false, alt: false, shift: false, key: "Tab" });
    expect(comboFromEvent(key({ key: "k", code: "KeyK", ctrlKey: true }), "linux")!.ctrl).toBe(true);
  });
  it("names keys by the layout's Latin character, else by position as Qt falls back", () => {
    // Russian: letters and symbols fall back to the Latin key at that position.
    expect(comboFromEvent(key({ key: "л", code: "KeyK", ctrlKey: true }), "linux")!.key).toBe("K");
    expect(comboFromEvent(key({ key: "б", code: "Comma", ctrlKey: true }), "linux")!.key).toBe(",");
    expect(comboFromEvent(key({ key: "ё", code: "Backquote", ctrlKey: true }), "linux")!.key).toBe("`");
    expect(comboFromEvent(key({ key: "Б", code: "Comma", ctrlKey: true, shiftKey: true }), "linux")!.key).toBe(",");
    expect(comboFromEvent(key({ key: "Ё", code: "Backquote", ctrlKey: true, shiftKey: true }), "linux")!.key).toBe("`");
    expect(comboFromEvent(key({ key: "№", code: "Digit3", ctrlKey: true, shiftKey: true }), "linux")!.key).toBe("3");
    // AZERTY and Dvorak: the character the layout types, not the US position.
    expect(comboFromEvent(key({ key: "a", code: "KeyQ", ctrlKey: true }), "linux")!.key).toBe("A");
    expect(comboFromEvent(key({ key: "&", code: "Digit1", ctrlKey: true }), "linux")!.key).toBe("&");
    expect(comboFromEvent(key({ key: "k", code: "KeyV", ctrlKey: true }), "linux")!.key).toBe("K");
    expect(comboFromEvent(key({ key: "w", code: "Comma", ctrlKey: true }), "linux")!.key).toBe("W");
    expect(comboFromEvent(key({ key: "Enter", code: "NumpadEnter" }), "linux")!.key).toBe("Enter");
    expect(comboFromEvent(key({ key: "Enter", code: "Enter" }), "linux")!.key).toBe("Return");
    expect(comboFromEvent(key({ key: "PageDown", code: "PageDown" }), "linux")!.key).toBe("PgDown");
    expect(comboFromEvent(key({ key: "≠", code: "Equal", altKey: true }), "macos")!.key).toBe("=");
    expect(comboFromEvent(key({ key: "Shift", code: "ShiftLeft", shiftKey: true }), "linux")).toBeUndefined();
  });
  it("tries the base key with Shift first, then the shifted character without it, as Qt does", () => {
    const tried = (init: Partial<KeyboardEvent> & { key: string; code: string }, on: Platform = "linux") =>
      candidates(key(init), on).map((c) => formatSequence([c]));
    expect(tried({ key: "+", code: "Equal", ctrlKey: true, shiftKey: true })).toEqual(["Ctrl+Shift+=", "Ctrl++"]);
    // US Ctrl+Shift+1 and Ctrl+Shift+3.
    expect(tried({ key: "!", code: "Digit1", ctrlKey: true, shiftKey: true })).toEqual(["Ctrl+Shift+1", "Ctrl+!"]);
    expect(tried({ key: "#", code: "Digit3", ctrlKey: true, shiftKey: true })).toEqual(["Ctrl+Shift+3", "Ctrl+#"]);
    // Russian Ctrl+Shift+2 types ", and Ctrl+Shift+Б has no Latin character.
    expect(tried({ key: "\"", code: "Digit2", ctrlKey: true, shiftKey: true })).toEqual(["Ctrl+Shift+2", "Ctrl+\""]);
    expect(tried({ key: "Б", code: "Comma", ctrlKey: true, shiftKey: true })).toEqual(["Ctrl+Shift+,", "Ctrl+<"]);
    // macOS ⌃⇧` is Meta+Shift+` first.
    expect(tried({ key: "~", code: "Backquote", ctrlKey: true, shiftKey: true }, "macos")).toEqual(["Meta+Shift+`", "Meta+~"]);
    // A layout's own shifted symbol on another key is also tried without Shift.
    expect(tried({ key: "|", code: "IntlBackslash", ctrlKey: true, shiftKey: true })).toEqual(["Ctrl+Shift+|", "Ctrl+|"]);
    expect(tried({ key: "A", code: "KeyA", ctrlKey: true, shiftKey: true })).toEqual(["Ctrl+Shift+A"]);
  });
  it("records what Qt records, so Terminal's Ctrl+Shift digits and ⌃⇧` keep their ✓", () => {
    const one = comboFromEvent(key({ key: "!", code: "Digit1", ctrlKey: true, shiftKey: true }), "linux")!;
    expect(formatSequence([one])).toBe("Ctrl+Shift+1");
    expect(terminalSafe(one, "linux")).toBe(true);
    const tick = comboFromEvent(key({ key: "~", code: "Backquote", ctrlKey: true, shiftKey: true }), "macos")!;
    expect(nativeText([tick], "macos")).toBe("⌃⇧`");
    expect(terminalSafe(tick, "macos")).toBe(true);
  });
});

describe("the registry and matching", () => {
  it("stores overrides, and the defaults as an absence", async () => {
    expect(keys("go.anywhere", "linux").map(formatSequence)).toEqual(["Ctrl+K"]);
    setKeys("go.anywhere", [seq("Ctrl+J")], "linux");
    expect(customized("go.anywhere")).toBe(true);
    expect(keys("go.anywhere", "linux").map(formatSequence)).toEqual(["Ctrl+J"]);
    setKeys("go.anywhere", [seq("Ctrl+K")], "linux");
    await Promise.resolve();
    expect(customized("go.anywhere")).toBe(false);
    setKeys("session.rename", [], "linux");
    expect(keys("session.rename", "linux")).toEqual([]);
    reset("session.rename");
    expect(keys("session.rename", "linux").map(formatSequence)).toEqual(["F2"]);
  });
  it("finds conflicts, prefixes either way included", () => {
    expect(conflicts("go.overview", seq("Ctrl+K"), "linux")).toEqual(["go.anywhere"]);
    expect(conflicts("go.overview", seq("Ctrl+X"), "linux")).toEqual(["message.push"]);
    expect(conflicts("go.overview", seq("Ctrl+F12"), "linux")).toEqual([]);
  });
  it("resets every shortcuts/ override, as Qt removes the group", async () => {
    await setSetting("shortcuts/go.anywhere", ["Ctrl+J"]);
    await setSetting("shortcuts/session.rename", []);
    resetAll();
    await Promise.resolve();
    expect(Object.keys(settings).filter((k) => k.startsWith("shortcuts/") && settings[k] != null)).toEqual([]);
  });
  it("routes window commands only, holds a partial and starts over on Escape", async () => {
    await setSetting("shortcuts/go.settings", ["Ctrl+G, Ctrl+S"]);
    let ran = 0;
    const off = onCommand("go.settings", () => ran++);
    const state = sequenceState();
    expect(route(key({ key: "g", code: "KeyG", ctrlKey: true }), state, "linux")).toEqual({ consume: true });
    expect(route(key({ key: "Escape", code: "Escape" }), state, "linux")).toEqual({ consume: true });
    expect(state.typed).toEqual([]);
    route(key({ key: "g", code: "KeyG", ctrlKey: true }), state, "linux");
    expect(route(key({ key: "s", code: "KeyS", ctrlKey: true }), state, "linux")).toEqual({ run: "go.settings", consume: true });
    // Composer commands run only from a message field.
    const send = onCommand("message.send", () => ran += 100);
    expect(route(key({ key: "Enter", code: "Enter", ctrlKey: true }), state, "linux")).toEqual({ consume: false });
    send();
    off();
    expect(route(key({ key: "g", code: "KeyG", ctrlKey: true }), state, "linux")).toEqual({ consume: false });
    await setSetting("shortcuts/go.settings", null);
    expect(ran).toBe(0);
  });
  it("keeps a newer handler first and restores the one before", () => {
    const calls: string[] = [];
    const first = onCommand("view.refresh", () => calls.push("first"));
    const second = onCommand("view.refresh", () => calls.push("second"));
    const state = sequenceState();
    expect(route(key({ key: "F5", code: "F5" }), state, "linux")).toEqual({ run: "view.refresh", consume: true });
    second();
    first();
    expect(route(key({ key: "F5", code: "F5" }), state, "linux")).toEqual({ consume: false });
    expect(calls).toEqual([]);
  });
});

describe("the focus (ShortcutRouter)", () => {
  it("knows Terminal by its mark and by xterm itself", () => {
    expect(contextOf(focus({ marked: "terminal", tag: "TEXTAREA" }), false)).toBe("terminal");
    // xterm's text area without the mark is still Terminal, never a text field.
    expect(contextOf(focus({ xterm: true, tag: "TEXTAREA" }), false)).toBe("terminal");
    expect(contextOf(focus({ tag: "TEXTAREA" }), false)).toBe("text-field");
    expect(contextOf(focus({ marked: "composer", tag: "TEXTAREA" }), false)).toBe("composer");
    expect(contextOf(focus({ marked: "capture" }), false)).toBe("none");
    expect(contextOf(focus({ dialog: true }), false)).toBe("none");
    expect(contextOf(focus(), false)).toBe("window");
    // A modal dialog or popup takes every key, wherever the focus fell.
    expect(contextOf(focus(), true)).toBe("none");
  });
  it("sees an open modal dialog or marked popup", () => {
    const doc = (found: string[]) => ({
      querySelector: (selector: string) => {
        if (selector.includes(":modal") && !found.includes(":modal")) throw new SyntaxError("unsupported");
        return found.some((f) => selector.includes(f)) ? ({} as Element) : null;
      },
    });
    expect(popupOpen(doc([]))).toBe(false);
    expect(popupOpen(doc([":modal"]))).toBe(true);
    expect(popupOpen(doc(["dialog[open]"]))).toBe(true);
    expect(popupOpen(undefined)).toBe(false);
  });
  it("lets Terminal keep Ctrl+- and Ctrl+= (readline undo)", () => {
    const off = onCommand("zoom.out", () => {});
    const state = sequenceState();
    const term = focus({ marked: "terminal", tag: "TEXTAREA" });
    expect(route(key({ key: "-", code: "Minus", ctrlKey: true, target: term }), state, "linux")).toEqual({ consume: false });
    expect(route(key({ key: "-", code: "Minus", ctrlKey: true, target: focus({ xterm: true, tag: "TEXTAREA" }) }), state, "linux")).toEqual({ consume: false });
    expect(route(key({ key: "-", code: "Minus", ctrlKey: true, target: focus() }), state, "linux")).toEqual({ run: "zoom.out", consume: true });
    off();
  });
  it("leaves keys to an input method, AltGr and a drop-down list", () => {
    const off = [onCommand("help.shortcuts", () => {}), onCommand("session.next", () => {}), onCommand("go.overview", () => {})];
    const state = sequenceState();
    expect(route(key({ key: "Process", code: "KeyK", ctrlKey: true }), state, "linux")).toEqual({ consume: false });
    expect(route(key({ key: "1", code: "Digit1", ctrlKey: true, keyCode: 229 }), state, "linux")).toEqual({ consume: false });
    const altGr = key({ key: "1", code: "Digit1", ctrlKey: true, altKey: false, target: focus({ tag: "INPUT" }) });
    expect(route({ ...altGr, getModifierState: (k: string) => k === "AltGraph" }, state, "windows")).toEqual({ consume: false });
    expect(route({ ...altGr, getModifierState: () => false }, state, "windows")).toEqual({ run: "go.overview", consume: true });
    const list = focus({ tag: "SELECT" });
    expect(route(key({ key: "F4", code: "F4", target: list }), state, "linux")).toEqual({ consume: false });
    expect(route(key({ key: "ArrowDown", code: "ArrowDown", altKey: true, target: list }), state, "linux")).toEqual({ consume: false });
    expect(route(key({ key: "F1", code: "F1", target: list }), state, "linux")).toEqual({ run: "help.shortcuts", consume: true });
    off.forEach((f) => f());
  });
});

describe("composer commands in the one map", () => {
  it("run the handler of the field that has the focus, before a window command on the same chord", async () => {
    const fieldA = focus({ marked: "composer", tag: "TEXTAREA", selection: [0, 0] });
    const fieldB = focus({ marked: "composer", tag: "TEXTAREA", selection: [0, 0] });
    const root = (field: EventTarget) => ({ contains: (target: Node | null) => target === (field as unknown as Node) });
    const ran: string[] = [];
    await setSetting("shortcuts/go.anywhere", ["Ctrl+Return"]);
    const off = [
      onCommand("go.anywhere", () => ran.push("window")),
      onCommand("message.send", () => ran.push("A"), root(fieldA)),
      onCommand("message.send", () => ran.push("B"), root(fieldB)),
    ];
    const state = sequenceState();
    const send = (target: EventTarget) => key({ key: "Enter", code: "Enter", ctrlKey: true, target });
    expect(route(send(fieldA), state, "linux")).toEqual({ run: "message.send", consume: true });
    expect(handlerFor("message.send", fieldA)).toBeDefined();
    handlerFor("message.send", fieldA)!();
    handlerFor("message.send", fieldB)!();
    expect(ran).toEqual(["A", "B"]);
    // Outside a message field the window command keeps the chord.
    expect(route(send(focus()), state, "linux")).toEqual({ run: "go.anywhere", consume: true });
    off.forEach((f) => f());
    await setSetting("shortcuts/go.anywhere", null);
  });
  const composer = (selection?: [number, number]) => focus({ marked: "composer", tag: "TEXTAREA", selection: selection ?? [2, 2] });
  it("sends and pushes from a message field only", () => {
    const ran: string[] = [];
    const off = [onCommand("message.send", () => ran.push("send")), onCommand("message.push", () => ran.push("push"))];
    const state = sequenceState();
    expect(route(key({ key: "Enter", code: "Enter", ctrlKey: true, target: composer() }), state, "linux")).toEqual({ run: "message.send", consume: true });
    expect(route(ctrl("X", { target: composer() }), state, "linux")).toEqual({ consume: true });
    expect(route(ctrl("S", { target: composer() }), state, "linux")).toEqual({ run: "message.push", consume: true });
    expect(route(key({ key: "Enter", code: "Enter", ctrlKey: true, target: focus() }), state, "linux")).toEqual({ consume: false });
    off.forEach((f) => f());
  });
  it("leaves Ctrl+X with a selection to Cut", () => {
    const off = onCommand("message.push", () => {});
    const state = sequenceState();
    expect(route(ctrl("X", { target: composer([0, 4]) }), state, "linux")).toEqual({ consume: false });
    expect(state.typed).toEqual([]);
    off();
  });
  it("swallows the key that ends a partial push, and Escape cancels it", () => {
    let zoomed = 0;
    const off = [onCommand("message.push", () => {}), onCommand("zoom.reset", () => zoomed++)];
    const state = sequenceState();
    route(ctrl("X", { target: composer() }), state, "linux");
    expect(route(ctrl("0", { target: composer() }), state, "linux")).toEqual({ consume: true });
    expect(zoomed).toBe(0);
    route(ctrl("X", { target: composer() }), state, "linux");
    expect(route(key({ key: "Escape", code: "Escape", target: composer() }), state, "linux")).toEqual({ consume: true });
    expect(route(ctrl("S", { target: composer() }), state, "linux")).toEqual({ consume: false });
    off.forEach((f) => f());
  });
});
