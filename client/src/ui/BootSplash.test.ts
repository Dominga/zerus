import { describe, expect, it } from "vitest";
import { SPLASH_DELAY_MS, splashInstallLine, splashTitle } from "./BootSplash";

describe("the startup splash", () => {
  it("uses Qt's StartupNotice delay and texts", () => {
    expect(SPLASH_DELAY_MS).toBe(1500);
    expect(splashTitle()).toBe("Starting Zerus…");
    expect(splashInstallLine()).toBe("Updating the hgs command line for local sessions");
  });
});
