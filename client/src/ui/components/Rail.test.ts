import { afterEach, describe, expect, it } from "vitest";
import { setSetting } from "../../store/settings";
import { shortcutTooltip } from "../../store/shortcuts";

const overridden = ["go.sessions", "session.new"];

afterEach(async () => {
  for (const id of overridden) await setSetting(`shortcuts/${id}`, null);
});

describe("Rail shortcut tooltips", () => {
  it("uses the registry defaults", () => {
    expect(shortcutTooltip("Home", "go.overview")).toBe("Home (Ctrl+1)");
    expect(shortcutTooltip("Sessions", "go.sessions")).toBe("Sessions (Ctrl+2)");
    expect(shortcutTooltip("New session", "session.new")).toBe("New session (Ctrl+N)");
    expect(shortcutTooltip("Settings", "go.settings")).toBe("Settings (Ctrl+,)");
  });

  it("follows an override and an explicitly unbound command", async () => {
    await setSetting("shortcuts/go.sessions", ["Ctrl+Shift+S"]);
    expect(shortcutTooltip("Sessions", "go.sessions")).toBe("Sessions (Ctrl+Shift+S)");
    await setSetting("shortcuts/session.new", []);
    expect(shortcutTooltip("New session", "session.new")).toBe("New session");
  });
});
