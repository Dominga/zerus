import { describe, expect, it } from "vitest";
import { accountTooltip, current, dayTime, forecast, fromProjected, pace, period, remaining, severest, stateText, summaryText, windowText } from "./usage";
import { NOW, projected, usageOf } from "../../store/testing";

describe("usage windows (inventory 8.4, gaps 1-4 and 7)", () => {
  it("1: adapts a projected window to minutes from now", () => {
    expect(fromProjected(projected(), NOW)).toEqual({ minutes: 300, used: 34, label: "", resetsIn: 120 });
    expect(fromProjected(projected({ resets_at: null }), NOW).resetsIn).toBeNull();
  });

  it("2: a missing reset counts as current and keeps the threshold tone", () => {
    const w = fromProjected(projected({ resets_at: null, used: 75 }), NOW);
    expect(current(w)).toBe(true);
    expect(pace(w)).toMatchObject({ known: false, tone: "warning" });
    expect(windowText(w)).toBe("5h 75%");
    expect(current(fromProjected(projected({ resets_at: NOW - 1 }), NOW))).toBe(false);
  });

  it("3: period reads the label and a missing length", () => {
    const at = (minutes: number | null, label = "") => period({ minutes, used: 0, resetsIn: null, label });
    expect([at(300), at(10080), at(2880), at(45), at(null), at(null, "Monthly"), at(10080, "Sonnet"), at(0)])
      .toEqual(["5h", "7d", "2d", "45m", "Usage", "Monthly", "Sonnet 7d", "Usage"]);
  });

  it("4: an unknown reset reads 'reset unknown'", () => {
    expect(remaining(null)).toBe("reset unknown");
    expect(windowText(fromProjected(projected({ label: "Monthly", minutes: null, used: 100, resets_at: null }), NOW))).toBe("Monthly 100% (reset unknown)");
    expect(remaining(0)).toBe("refresh needed");
    expect(remaining(61)).toBe("1h 1m");
  });

  // Review Focus 3.
  it("ISO-parsed, null and unlengthed windows never show NaN", () => {
    for (const w of [projected({ resets_at: null }), projected({ minutes: null, label: "Monthly" }), projected({ minutes: null, resets_at: null }), projected({ used: 100 })]) {
      const at = fromProjected(w, NOW);
      expect(`${windowText(at)} ${forecast(at)} ${period(at)}`).not.toMatch(/NaN|undefined|null/);
    }
  });

  it("7: the tooltip has identity lines, windows, balances and the refresh notes", () => {
    const u = usageOf({
      identity: { email: "person@example.com", name: "Example Person", plan: "max", organization: "Example Org", account_id: "acct-1", auth_method: "oauth" },
      windows: [projected({ resets_at: null }), projected({ id: "m", label: "Monthly", minutes: null, used: 50, resets_at: null })],
      balances: [{ kind: "Topped up", balance: "18.40", currency: "USD" }],
      checked_at: new Date(2026, 9, 28, 17, 5).getTime() / 1000, refresh_error: true, offline: true,
    });
    expect(accountTooltip(u, NOW).split("\n")).toEqual([
      "Claude", "Work", "Example Person", "person@example.com", "max", "Example Org",
      "5h: 34% used", "Monthly: 50% used", "Topped up: 18.40 USD", "Updated 17:05",
      "Update failed. Showing the last available values.", "Machine is offline.", "Updates every 5 minutes. Open account details.",
    ]);
    const resetting = accountTooltip(usageOf({ windows: [projected()] }), NOW).split("\n");
    expect(resetting[3]).toBe(`Resets ${dayTime(NOW + 7200)} (in 2h 0m)`);
    expect(accountTooltip(usageOf({ status: "signed_out", refreshing: true, label: "Claude" }), NOW).split("\n")).toEqual([
      "Claude", "Updating… Previous values remain visible.",
      "No native subscription sign-in found. API-key sessions may still work.", "Updates every 5 minutes. Open account details.",
    ]);
  });

  it("severest takes the worst pace tone of the current windows", () => {
    const fast = fromProjected(projected({ used: 60, resets_at: NOW + 240 * 60 }), NOW);
    const calm = fromProjected(projected({ used: 10, resets_at: NOW + 60 * 60 }), NOW);
    expect(pace(fast).tone).toBe("critical");
    expect(severest([calm, fast])).toBe("critical");
    expect(severest([calm])).toBe("normal");
    expect(severest([fromProjected(projected({ resets_at: NOW - 60 }), NOW)])).toBeUndefined();
    expect(severest([])).toBeUndefined();
  });

  it("summaries and states follow Qt", () => {
    expect(summaryText(usageOf({ balances: [{ kind: "Topped up", balance: "18.40", currency: "USD" }] }))).toBe("18.40 USD");
    expect(summaryText(usageOf({ status: "configured", provider: "dsh" }))).toBe("API key");
    expect(summaryText(usageOf({ status: "configured", provider: "opencode" }))).toBe("Configured");
    expect(summaryText(usageOf({ status: "signed_out" }))).toBe("Sign in required");
    expect(summaryText(usageOf({ status: "expired" }))).toBe("Sign-in expired");
    expect(summaryText(usageOf({ status: "loading" }))).toBe("Unavailable");
    expect(stateText(usageOf({ status: "loading" }))).toBe("Loading account details…");
    expect(stateText(usageOf({ status: "ok", offline: true }))).toBe("Machine is offline. Showing the last available account data.");
    expect(stateText(usageOf({ status: "signed_out", provider: "dsh" })))
      .toBe("DeepSeek account is signed out. API-key connections are configured separately in the native settings.");
  });

  it("formats local dates as Qt's d MMM HH:mm", () => {
    expect(dayTime(new Date(2026, 9, 28, 7, 5).getTime() / 1000)).toBe("28 Oct 07:05");
  });
});
