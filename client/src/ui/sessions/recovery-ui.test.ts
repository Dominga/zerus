import { describe, expect, it } from "vitest";
import { providerFailure, recoveryInspection, recoveryStatus, recoveryUsageNotice, recoveryView } from "./recovery-ui";
import { toSession } from "../../store/catalog";
import { NOW, projected, rowOut, usageOf } from "../../store/testing";
import { clock, dayTime } from "../lib/usage";

const session = () => toSession("", "run:1", rowOut("", 1, { run_id: "run-1", conversation_id: "conv-1", agent: "claude" }).row);
const waiting = () => ({
  id: "11111111-1111-4111-8111-111111111111", name: "claude/sample-project/review", state: "waiting", class: "session_limit",
  session_limit_mode: "reset", due_at: NOW + 3600, not_before: NOW - 1, attempt: 2, class_attempt: 2, delays: [300, 0],
  reason: "Temporary rate limit", action: "send_message", history: [{ state: "waiting", at: NOW - 10, attempt: 1, reason: "Temporary rate limit" }],
});

describe("recovery presentation", () => {
  it("shows a reset wait with actions, budget and attempt history", () => {
    const job = waiting();
    expect(recoveryStatus(job, NOW)).toBe(`Waiting for reset at ${dayTime(NOW + 3600)}`);
    const view = recoveryView({ phase: "idle", recovery: job }, true, NOW, false, "", "");
    expect(view).toMatchObject({
      active: true, failure: false, tone: "warning", label: `Retry ${clock(NOW + 3600)}`, shortLabel: clock(NOW + 3600),
      detail: "Temporary rate limit", jobId: job.id,
      now: { visible: true, enabled: true }, cancel: { visible: true, enabled: true },
      history: { visible: true, enabled: true }, settingsVisible: true,
    });
    expect(view.status).toContain("(2 attempts)");
    expect(view.attempts).toHaveLength(1);
    expect(view.attempts[0]).toMatchObject({ label: "Retry scheduled", attempts: 1, reason: "Temporary rate limit" });
  });

  it("uses provider errors and their exact explanations", () => {
    expect(providerFailure({ error_kind: "provider_policy" })).toBe("Request blocked by provider");
    const quota = recoveryView({ phase: "error", provider_error: { error_kind: "quota", detail: "limit" } }, true, NOW, false, "", "");
    expect(quota).toMatchObject({ active: true, failure: true, label: "Provider error", shortLabel: "Error", status: "Usage limit reached" });
    expect(quota.help).toContain("Your draft is kept; Zerus will not retry automatically.");
    expect(quota.now.visible).toBe(false);
    expect(quota.history.visible).toBe(false);
    expect(quota.terminal).toEqual({ visible: true, enabled: true });
    expect(quota.usage).toEqual({ visible: true, enabled: true });
    expect(quota.settingsVisible).toBe(false);

    const limited = recoveryView({ phase: "error", provider_error: { error_kind: "session_limit", detail: "stop" }, recovery: { ...waiting(), class: "rate_limit" } }, true, NOW, false, "", "Fresh usage");
    expect(limited.status).toBe("Session limit reached");
    expect(limited.help.startsWith("Fresh usage\n")).toBe(true);
    expect(limited.settingsVisible).toBe(true);
  });

  it("keeps an action error and disables repeated clicks", () => {
    const view = recoveryView({ phase: "idle", recovery: { ...waiting(), session_limit_mode: "interval", due_at: NOW + 20 } }, true, NOW, true, "Recovery timed out", "");
    expect(view.detail).toBe("Recovery timed out");
    expect(view.now.enabled).toBe(false);
    expect(view.cancel.enabled).toBe(false);
    expect(view.label).toBe(`Retry ${clock(NOW + 20)}`);
    expect(view.shortLabel).toBe(clock(NOW + 20));
  });

  it("hides a resumed job and rejects another run", () => {
    const job = { ...waiting(), state: "retrying", acknowledged: true, resumed_at: NOW - 1 };
    expect(recoveryView({ phase: "idle", recovery: job }, true, NOW, false, "", "").active).toBe(false);
    const s = session();
    expect(recoveryInspection(s, { value: { run_id: "run-1", conversation_id: "conv-1", recovery: waiting() }, settled: true }).recovery).toBeDefined();
    expect(recoveryInspection(s, { value: { run_id: "run-2", conversation_id: "conv-1" }, settled: true })).toEqual({});
  });
});

describe("recovery usage notice", () => {
  it("adds a below-limit explanation only for the current Claude failure", () => {
    const s = session();
    const usage = usageOf({ checked_at: NOW - 20, windows: [projected({ used: 40 }), projected({ id: "week", minutes: 10080, used: 50 })] });
    const details = { phase: "error", run_id: "run-1", conversation_id: "conv-1", provider_error: { error_kind: "session_limit", at: NOW - 60 } };
    expect(recoveryUsageNotice(s, details, usage, true, NOW)).toContain("Latest account usage is below the limit.");
    expect(recoveryUsageNotice(s, { ...details, phase: "idle" }, usage, true, NOW)).toBe("");
    expect(recoveryUsageNotice(s, details, { ...usage, refresh_error: true }, true, NOW)).toBe("");
  });
});
