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

const APPEARANCE = [
  "Follow system appearance", "Dark", "Light", "Interface language",
  "Restart Zerus to apply the interface language. Agent sessions keep running.",
  "Russian is a preview. Untranslated text stays in English.", "Russian (preview)",
  "Usage limit indicator", "Usage and elapsed time bars", "Pace marker", "Pace marker and reset countdown",
  "Compares used quota with the elapsed part of each limit window. The bars show usage above elapsed time; the marker shows elapsed time on the usage bar. Colors warn when the current pace would use up the limit before it resets.",
  "Content scale",
  "Adjusts Activity, the message field and Terminal from 75% to 200%. The session list and the side panel keep their size.",
  "Appearance and session list preferences apply to this Zerus.",
];
const SESSIONS = [
  "Expand the collapsed session list on hover", "Hide projects without active sessions", "Animate working sessions",
  "Show completed subagents",
  "Include agents whose latest turn has finished in the session list and its counts. Plan & agents always shows all reported agents. Agents with unknown status remain visible.",
  "Notify about optional questions",
  "Off by default. Shows a system notification once for each new optional question, which the agent keeps working without. It is withdrawn when the questions are answered or skipped, or the session is marked as read. Questions that need an answer always notify.",
  "Closing Zerus keeps agents running. Projects and the arrangement of your workspace are saved on this device.",
];

describe("Settings texts", () => {
  it.each(APPEARANCE)("Appearance shows Qt's %s", (text) => {
    expect(qt).toContain(`"${text}"`);
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
