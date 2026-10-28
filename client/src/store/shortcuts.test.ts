import { describe, expect, it } from "vitest";
import native from "./shortcuts-native.json";
import { setSetting } from "./settings";
import {
  COMMANDS, allowed, assignable, candidates, comboFromEvent, conflicts, customized, formatSequence, keys, matches,
  nativeText, onCommand, parseChord, parseSequence, portableDefaults, protectedInTextField, reset, route, sequenceState,
  setKeys, terminalSafe, type Combo, type Platform,
} from "./shortcuts";

// shortcuts-native.json was recorded by Qt 6.12 itself on Linux and on macOS
// (client/scripts/record-key-sequences.cpp): each portable chord with
// QKeySequence's NativeText and PortableText.

const seq = (text: string) => parseSequence(text)!;
const combo = (text: string) => parseChord(text)!;
const key = (init: Partial<KeyboardEvent> & { key: string; code: string }) =>
  ({ altKey: false, ctrlKey: false, metaKey: false, shiftKey: false, isComposing: false, defaultPrevented: false, target: null, ...init }) as KeyboardEvent;

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
  it("names keys as Qt does, by position for letters and digits", () => {
    expect(comboFromEvent(key({ key: "л", code: "KeyK", ctrlKey: true }), "linux")!.key).toBe("K");
    expect(comboFromEvent(key({ key: "Enter", code: "NumpadEnter" }), "linux")!.key).toBe("Enter");
    expect(comboFromEvent(key({ key: "Enter", code: "Enter" }), "linux")!.key).toBe("Return");
    expect(comboFromEvent(key({ key: "PageDown", code: "PageDown" }), "linux")!.key).toBe("PgDown");
    expect(comboFromEvent(key({ key: "≠", code: "Equal", altKey: true }), "macos")!.key).toBe("=");
    expect(comboFromEvent(key({ key: "Shift", code: "ShiftLeft", shiftKey: true }), "linux")).toBeUndefined();
  });
  it("also tries a shifted symbol without Shift", () => {
    const plus = comboFromEvent(key({ key: "+", code: "Equal", ctrlKey: true, shiftKey: true }), "linux")!;
    expect(candidates(plus).map((c) => formatSequence([c]))).toEqual(["Ctrl+Shift++", "Ctrl++"]);
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
  it("matches a two-chord sequence with a held state", () => {
    const state = sequenceState();
    expect(matches("message.push", key({ key: "x", code: "KeyX", ctrlKey: true }), state, "linux")).toBe("partial");
    expect(matches("message.push", key({ key: "s", code: "KeyS", ctrlKey: true }), state, "linux")).toBe("full");
    expect(state.typed).toEqual([]);
    expect(matches("message.push", key({ key: "s", code: "KeyS", ctrlKey: true }), state, "linux")).toBe(false);
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
    // Composer commands are the composer's.
    onCommand("message.send", () => ran += 100);
    expect(route(key({ key: "Enter", code: "Enter", ctrlKey: true }), state, "linux")).toEqual({ consume: false });
    off();
    expect(route(key({ key: "g", code: "KeyG", ctrlKey: true }), state, "linux")).toEqual({ consume: false });
    await setSetting("shortcuts/go.settings", null);
    expect(ran).toBe(0);
  });
});
