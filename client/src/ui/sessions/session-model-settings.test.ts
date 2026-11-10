import { describe, expect, it } from "vitest";
import { accountDefaultsNotice, settingsView, type ModelContext } from "./session-model-settings";
import { toSession } from "../../store/catalog";

function context(defaults?: unknown): ModelContext {
  const row = toSession("", "run:r1", { name: "dsh/sample-project/a", run_id: "r1", conversation_id: "session-1", agent: "dsh",
    model: "deepseek-flash", effort: "high", state: "running", activity: "idle", phase: "idle", process_state: "running" });
  return { row, online: true, busy: false, details: { run_id: "r1", conversation_id: "session-1", model: "deepseek-flash",
    settings_model: "deepseek-official/deepseek-flash", effort: "high", settings_change_supported: true,
    model_options: [{ id: "deepseek-official/deepseek-flash", label: "DeepSeek-V4.1-Flash (DeepSeek)", effort_options: ["off", "low", "high", "max"] }],
    ...(defaults === undefined ? {} : { account_defaults: defaults }) } };
}

describe("DeepSeek account launch defaults (zerus-fw3g5)", () => {
  it("names a failed application with the account's choice and hgs's own reason", () => {
    const notice = accountDefaultsNotice({ model: "deepseek-official/deepseek-flash", effort: "max", status: "failed",
      detail: "This model is unavailable for the native account" })!;
    expect(notice).toEqual({ status: "failed", title: "Account defaults were not applied",
      text: "The account's model and effort were not applied when this session started. It began with the DeepSeek default.",
      choice: "Account default: deepseek-official/deepseek-flash (max)", detail: "This model is unavailable for the native account" });
    const view = settingsView(context({ model: "deepseek-official/deepseek-flash", effort: "max", status: "failed" }));
    expect(view.defaults?.status).toBe("failed");
    expect(view.tooltip.split("\n").at(-1)).toBe("Account defaults were not applied");
    expect(view.accessible).toContain("Account defaults were not applied");
    // The selector itself keeps working: nothing here blocks or retries a change.
    expect(view.enabled).toBe(true);
  });
  it("asks to check an uncertain application", () => {
    const notice = accountDefaultsNotice({ model: "deepseek-official/deepseek-flash", effort: "", status: "uncertain",
      detail: "Native request delivery uncertain:\ntimed out" })!;
    expect(notice.title).toBe("Account defaults may not have been applied");
    expect(notice.text).toContain("Check the model and effort above.");
    expect(notice.choice).toBe("Account default: deepseek-official/deepseek-flash");
    expect(notice.detail).toBe("Native request delivery uncertain: timed out");
    expect(accountDefaultsNotice({ status: "uncertain", detail: "x".repeat(900) })!.detail).toHaveLength(600);
  });
  it("shows nothing when applied, still pending, absent or malformed", () => {
    for (const value of [{ status: "applied", model: "a/b" }, { status: "pending" }, undefined, null, "failed", ["failed"], { status: 1 }])
      expect(accountDefaultsNotice(value)).toBeUndefined();
    const plain = settingsView(context()), applied = settingsView(context({ status: "applied", model: "a/b", effort: "max" }));
    expect(plain.defaults).toBeUndefined(); expect(applied.defaults).toBeUndefined();
    expect(applied.tooltip).toBe(plain.tooltip); expect(applied.accessible).toBe(plain.accessible);
  });
});
