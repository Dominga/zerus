import { describe, expect, it } from "vitest";
import { setSetting } from "../../store/settings";
import { compactSessionRows, expandSessionsOnHover, hideEmptyProjects } from "./session-prefs";

// Task 202 (SettingsPage.h:232-233): the Sessions settings are Qt's, with Qt's
// defaults, and the session list reads them.
describe("Settings > Sessions preferences", () => {
  it("starts from Qt's defaults", () => {
    expect([compactSessionRows(), expandSessionsOnHover(), hideEmptyProjects()]).toEqual([false, true, true]);
  });

  it("follows the stored values", async () => {
    await setSetting("workspace/compact", true);
    await setSetting("workspace/expandSessionsOnHover", false);
    await setSetting("workspace/hideEmptyProjects", false);
    expect([compactSessionRows(), expandSessionsOnHover(), hideEmptyProjects()]).toEqual([true, false, false]);
    await setSetting("workspace/compact", false);
    await setSetting("workspace/expandSessionsOnHover", true);
    await setSetting("workspace/hideEmptyProjects", true);
    expect([compactSessionRows(), expandSessionsOnHover(), hideEmptyProjects()]).toEqual([false, true, true]);
  });

  it("the session list reads them (compact density, hover peek)", async () => {
    const fs = await import(/* @vite-ignore */ ["node", "fs"].join(":")) as { readFileSync(path: URL, encoding: "utf8"): string };
    const list = fs.readFileSync(new URL("./SessionList.tsx", import.meta.url), "utf8").replace(/\r\n/g, "\n");
    expect(list).toContain("!expandSessionsOnHover()");
    expect(list).toContain('data-density={compactSessionRows() ? "compact" : "normal"}');
  });
});
