import { describe, expect, it } from "vitest";
import { accountTooltip, clock, current, dayTime, exhausted, exhaustedSummary, forecast, fromProjected, limitRecoveryNotice, pace, period, rateLimitNotice, remaining, severest, stateText, summaryText, usageChipPercent, usageRetryWaiting, windowText } from "./usage";
import { NOW, projected, usageOf } from "../../store/testing";

describe("usage windows (inventory 8.4, gaps 1-4 and 7)", () => {
  it("shows the sample age and cooldown without hiding saved windows or changing sign-in", () => {
    const u = usageOf({ status: "rate_limited", auth_status: "signed_in", checked_at: NOW - 120, retry_at: NOW + 100, windows: [projected({ used: 42 })] });
    expect(rateLimitNotice(u, NOW)).toBe("Limits 2 min ago, rate limited. Retry in 100 s");
    expect(usageRetryWaiting(u, NOW)).toBe(true);
    expect(accountTooltip(u, NOW)).toContain("5h: 42% used");
    // A tooltip names the retry's clock time; only the visible notice counts seconds (zerus-5ryrx).
    expect(rateLimitNotice(u, NOW, true)).toBe(`Limits from ${clock(NOW - 120)}, rate limited. Retry at ${clock(NOW + 100)}`);
    // A tooltip names clock times: it reads the same a minute later (zerus-f5gta).
    expect(rateLimitNotice(u, NOW + 60, true)).toBe(rateLimitNotice(u, NOW, true));
    expect(accountTooltip(u, NOW)).toContain(rateLimitNotice(u, NOW, true));
    expect(accountTooltip(u, NOW)).not.toMatch(/Retry in \d+ s/);
    expect(accountTooltip(u, NOW + 1)).toBe(accountTooltip(u, NOW));
    expect(summaryText(u)).toBe("Rate limited");
    expect(stateText(u)).toContain("sign-in is unchanged");
    expect(rateLimitNotice(u, NOW + 100)).toBe("Limits 3 min ago, rate limited. Refresh to check limits.");
    expect(usageRetryWaiting(u, NOW + 100)).toBe(false);
    expect(rateLimitNotice({ ...u, checked_at: null, windows: [] }, NOW)).toBe("No saved limits, rate limited. Retry in 100 s");
    expect(rateLimitNotice({ ...u, status: "ok", retry_at: null }, NOW)).toBe("");
    expect(u.windows[0].used).toBe(42);
  });
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
    expect(summaryText(usageOf({ status: "unavailable" }))).toBe("limits n/a");
    expect(summaryText(usageOf({ status: "ok", refresh_error: true }))).toBe("limits n/a");
    expect(stateText(usageOf({ status: "loading" }))).toBe("Loading account details…");
    expect(stateText(usageOf({ status: "ok", offline: true }))).toBe("Machine is offline. Showing the last available account data.");
    expect(stateText(usageOf({ status: "signed_out", provider: "dsh" })))
      .toBe("DeepSeek account is signed out. API-key connections are configured separately in the native settings.");
  });

  it("formats local dates as Qt's d MMM HH:mm", () => {
    expect(dayTime(new Date(2026, 9, 28, 7, 5).getTime() / 1000)).toBe("28 Oct 07:05");
    const mockup = new Date(2026, 9, 27, 10, 24, 9).getTime() / 1000;
    expect(dayTime(mockup)).toBe("27 Oct 10:24");
    expect(dayTime(mockup, true)).toBe("27 Oct 10:24:09");
  });

  it("shows exhausted limits and cached warnings without a dot separator", () => {
    const windows = [
      projected({ used: 100, resets_at: NOW + 3600 }),
      projected({ id: "week", minutes: 10080, used: 100, resets_at: NOW + 7200 }),
    ];
    const u = usageOf({ windows, refresh_error: true });
    expect(exhausted(u, NOW)).toBe("Last reported usage: 5h limit reached. Resets in 1h 0m\n7d limit reached. Resets in 2h 0m. Refresh to check current limits.");
    expect(exhaustedSummary(u, NOW)).toEqual({ label: "Limit reached", shortLabel: "Limit", remaining: "1h 0m" });
    expect(exhaustedSummary(usageOf({ windows: [projected({ used: 100, resets_at: NOW - 1 })] }), NOW))
      .toEqual({ label: "Refresh usage", shortLabel: "Limit", remaining: "" });
  });

  it("shows the mockup's usage percentage from 50%, ignoring ended windows", () => {
    expect(usageChipPercent(usageOf({ windows: [projected({ used: 49.9 })] }), NOW)).toBeUndefined();
    expect(usageChipPercent(usageOf({ windows: [projected({ used: 50 })] }), NOW)).toBe(50);
    expect(usageChipPercent(usageOf({ windows: [projected({ used: 52 }), projected({ id: "week", minutes: 10080, used: 71.4 })] }), NOW)).toBe(71);
    expect(usageChipPercent(usageOf({ windows: [projected({ used: 100, resets_at: NOW - 1 })] }), NOW)).toBeUndefined();
  });

  it("accepts only a fresh below-limit recovery snapshot", () => {
    const windows = [projected({ used: 40 }), projected({ id: "week", minutes: 10080, used: 60 })];
    const u = usageOf({ windows, checked_at: NOW - 30 });
    expect(limitRecoveryNotice(u, NOW - 60, NOW)).toContain("Latest account usage is below the limit.");
    expect(limitRecoveryNotice({ ...u, checked_at: NOW - 60 }, NOW - 60, NOW)).toBe("");
    expect(limitRecoveryNotice({ ...u, refresh_error: true }, NOW - 60, NOW)).toBe("");
    expect(limitRecoveryNotice({ ...u, windows: [windows[0]] }, NOW - 60, NOW)).toBe("");
    expect(limitRecoveryNotice({ ...u, windows: [projected({ used: 100 }), windows[1]] }, NOW - 60, NOW)).toBe("");
  });
});
