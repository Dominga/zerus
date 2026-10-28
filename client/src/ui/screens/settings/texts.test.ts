import { describe, expect, it } from "vitest";

// Every text Settings shows for a Qt control is Qt's own: it appears in the
// Qt source it comes from and in the section that shows it.

// Read from disk, as styles/motion.test.ts does: the specifier is built so
// TypeScript (no Node types here) leaves it alone.
const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as {
  readFileSync(path: URL, encoding: "utf8"): string;
};
const here = new URL("./", import.meta.url);
const repo = new URL("../../../../../", import.meta.url);
const qt = ["tray/src/SettingsPage.h", "tray/src/UiLanguage.h"].map((file) => fs.readFileSync(new URL(file, repo), "utf8")).join("\n");
const source = (file: string) => fs.readFileSync(new URL(file, here), "utf8");
const layer = fs.readFileSync(new URL("tray/src/WindowLayer.cpp", repo), "utf8");
const keyboard = fs.readFileSync(new URL("tray/src/KeyboardSettings.cpp", repo), "utf8");
const shortcuts = fs.readFileSync(new URL("tray/src/Shortcuts.cpp", repo), "utf8");
const store = fs.readFileSync(new URL("../../../store/shortcuts.ts", here), "utf8");

const APPEARANCE = [
  "Follow system appearance", "Dark", "Light", "Interface language",
  "Restart Zerus to apply the interface language. Agent sessions keep running.",
  "Russian is a preview. Untranslated text stays in English.", "Russian (preview)",
  "Usage limit indicator", "Usage and elapsed time bars", "Pace marker", "Pace marker and reset countdown",
  "Compares used quota with the elapsed part of each limit window. The bars show usage above elapsed time; the marker shows elapsed time on the usage bar. Colors warn when the current pace would use up the limit before it resets.",
  "Content scale",
  "Adjusts Activity, the message field and Terminal from 75% to 200%. The session list and the side panel keep their size.",
  "Appearance and session list preferences apply to this Zerus.",
  "Activity width", "Maximum Activity width", "Full-width Activity", "Keep Zerus above other windows",
];
// Qt texts with an argument, as Appearance fills it in.
const APPEARANCE_FILLED: [string, string][] = [
  ["Reset to %1 px", "Reset to 900 px"],
  ["In a wide window, Activity, questions and the message field stay centered at this width, which grows with content scale. Drag the column edges in Activity to adjust it, or double-click an edge to restore %1 px. Full-width Activity fills the whole pane.",
    "In a wide window, Activity, questions and the message field stay centered at this width, which grows with content scale. Drag the column edges in Activity to adjust it, or double-click an edge to restore 900 px. Full-width Activity fills the whole pane."],
];
const WINDOW_LAYER = [
  "Files and terminals opened from Zerus may appear behind it while this is on.",
  "Keep Above is unavailable in this Zerus session. On Wayland, KDE Plasma 6 with KWin scripting is required. You can use your desktop's window menu or rules.",
];
const SESSIONS = [
  "Expand the collapsed session list on hover", "Hide projects without active sessions", "Animate working sessions",
  "Show completed subagents",
  "Include agents whose latest turn has finished in the session list and its counts. Plan & agents always shows all reported agents. Agents with unknown status remain visible.",
  "Notify about optional questions",
  "Off by default. Shows a system notification once for each new optional question, which the agent keeps working without. It is withdrawn when the questions are answered or skipped, or the session is marked as read. Questions that need an answer always notify.",
  "Closing Zerus keeps agents running. Projects and the arrangement of your workspace are saved on this device.",
];

const KEYBOARD = [
  "Keyboard", "Command", "Shortcut", "Alternate", "Reset", "Reset all", "Restore the default chords of the selected command",
  "Chords marked ✓ work in Terminal.", "Double-click a shortcut, or press Enter on it, to change it.", "Works in Terminal",
  "Reset all shortcuts", "Restore the default chords of every command?", "Change shortcut", "Clear", "Shortcut in use",
];
const KEYBOARD_FILLED: [string, string][] = [
  ["Press the new shortcut for %1.", "Press the new shortcut for {title}."],
  ["Already used by %1. Reassign?", "Already used by ${separatedList("],
];

const ABOUT_FILLED: [string, string][] = [
  ["Zerus %1", "`Zerus ${installed()}`"],
  ["hgs CLI %1", "`hgs CLI ${version}`"],
];
const ABOUT = ["About", "hgs CLI version unavailable", "hgs CLI: checking…", "Open Zerus repository on GitHub"];

describe("Settings texts", () => {
  it.each(ABOUT)("About shows Qt's %s", (text) => {
    expect(qt).toContain(`"${text}"`);
    expect(source("About.tsx")).toContain(text);
  });
  it.each(ABOUT_FILLED)("About fills in Qt's %s", (template, shown) => {
    expect(qt).toContain(`"${template}"`);
    expect(source("About.tsx")).toContain(shown);
  });
  it.each(KEYBOARD)("Keyboard shows Qt's %s", (text) => {
    expect(keyboard).toContain(`"${text}"`);
    expect(source("Keyboard.tsx")).toContain(text);
  });
  it.each(KEYBOARD_FILLED)("Keyboard fills in Qt's %s", (template, shown) => {
    expect(keyboard).toContain(`"${template}"`);
    expect(source("Keyboard.tsx")).toContain(shown);
  });
  it("uses Shortcuts.cpp's titles, groups, reasons and Terminal rules", () => {
    const qtTexts = [...shortcuts.matchAll(/(?:QT_TRANSLATE_NOOP\("Shortcuts", |translate\("Shortcuts", )"((?:[^"\\]|\\.)+)"/g)].map((m) => m[1]);
    expect(qtTexts.length).toBeGreaterThan(40);
    for (const text of qtTexts) expect(store, text).toContain(`"${text}"`);
  });
  it.each(APPEARANCE)("Appearance shows Qt's %s", (text) => {
    expect(qt).toContain(`"${text}"`);
    expect(source("Appearance.tsx")).toContain(text);
  });
  it.each(APPEARANCE_FILLED)("Appearance fills in Qt's %s", (template, shown) => {
    expect(qt).toContain(`"${template}"`);
    expect(source("Appearance.tsx")).toContain(shown);
  });
  it.each(WINDOW_LAYER)("Appearance shows WindowLayer's %s", (text) => {
    expect(layer).toContain(`"${text}"`);
    expect(source("Appearance.tsx")).toContain(text);
  });
  it.each(SESSIONS)("Sessions shows Qt's %s", (text) => {
    expect(qt).toContain(`"${text}"`);
    expect(source("Sessions.tsx")).toContain(text);
  });
  it("lists the sections in Qt's order", () => {
    const labels = [...source("sections.ts").matchAll(/label: "([^"]+)"/g)].map((m) => m[1]);
    const nav = /nav->addItems\(\{([^}]*)\}\)/.exec(qt)![1];
    const qtOrder = [...nav.matchAll(/tr\("([^"]+)"\)/g)].map((m) => m[1]);
    // Worktrees goes in before Automatic approvals (SettingsPage.h:140).
    qtOrder.splice(qtOrder.indexOf("Automatic approvals"), 0, "Worktrees");
    expect(labels).toEqual(qtOrder);
  });
});
