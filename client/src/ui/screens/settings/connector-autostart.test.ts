import { describe, expect, it } from "vitest";
import { autostartStatus, autostartSaveWarning, type AutostartLine } from "./connector-autostart";

describe("Automatic connector start line", () => {
  it("says nothing where there is no automatic start", () => {
    expect(autostartStatus(undefined)).toBeUndefined();
    expect(autostartStatus(null)).toBeUndefined();
  });
  it.each<[AutostartLine["kind"], string, RegExp]>([
    ["running", "ok", /Starts automatically when Zerus starts/],
    ["running_not_armed", "muted", /won't start automatically/],
    ["starting", "busy", /Starting automatically/],
    ["will_start", "muted", /next time Zerus starts/],
    ["not_armed", "muted", /Choose Start connector once/],
    ["stopped_by_you", "muted", /Stopped by you/],
    ["unavailable", "warn", /no packaged connector/],
  ])("%s", (kind, tone, text) => {
    const shown = autostartStatus({ kind } as AutostartLine)!;
    expect(shown.kind).toBe(tone);
    expect(shown.text).toMatch(text);
  });
  it("names why an automatic start failed", () => {
    const shown = autostartStatus({ kind: "failed", reason: "Not confirmed." })!;
    expect(shown.kind).toBe("warn");
    expect(shown.text).toBe("Automatic start failed: Not confirmed.");
  });
  it("warns only when the choice could not be saved", () => {
    expect(autostartSaveWarning(false)).toBeUndefined();
    expect(autostartSaveWarning(undefined)).toBeUndefined();
    expect(autostartSaveWarning(true)).toMatch(/may start again/);
  });
});
