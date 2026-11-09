import { afterEach, describe, expect, it } from "vitest";
import rail from "./Rail.tsx?raw";
import about from "../screens/settings/About.tsx?raw";
import { setSetting } from "../../store/settings";
import { shortcutTooltip } from "../../store/shortcuts";
import { attentionBadgeText } from "./rail-badge";

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

describe("Rail installed-build identity", () => {
  it("does not label every build Dev or read the frontend package version", () => {
    expect(rail).not.toContain("__APP_VERSION__");
    expect(rail).not.toContain("development build");
    expect(rail).not.toMatch(/>\{short\} Dev</);
  });
  it("uses the same native identity as About and loads it when the rail mounts", () => {
    expect(rail).toContain('from "../../store/buildInfo"');
    expect(rail).toContain('onMount(() => { void refreshAppBuildInfo(); })');
    expect(rail).toContain('railVersion(appBuildInfo())');
    expect(rail).toContain('installedAppName(appBuildInfo())');
    expect(about).toContain('from "../../../store/buildInfo"');
    expect(about).toContain('refreshAppBuildInfo()');
    expect(about).toContain('installedAppName(appBuildInfo())');
    expect(about).not.toContain('__APP_VERSION__');
    expect(about).not.toContain('invoke<BuildInfo>("build_info")');
  });
});

describe("Rail attention badge (Qt SessionsWindow.cpp:3374-3381)", () => {
  it("caps the count at 99+, and the Sessions button names the exact count", () => {
    expect([1, 99, 100, 12345].map(attentionBadgeText)).toEqual(["1", "99", "99+", "99+"]);
    expect(rail).toContain('tr("Sessions — %1 need attention", counts().attention)');
    expect(rail).toContain("attentionBadgeText(counts().attention)");
  });
});
