import { describe, expect, it, vi } from "vitest";
import { createHgsSetting, type HgsSettingPort } from "./hgsSettings";
import type { HgsSettingOutcome } from "../types";

// Task 250 G2 (SettingsPage.h:242-250): hgs owns "Ask before an agent starts
// subagents" and "Block DeepSeek session-log upload". The model shows what hgs
// confirmed (the default until the first read), sends the person's write with
// a request id, and a refusal changes nothing and shows hgs's own words.

function fake(...outcomes: HgsSettingOutcome[]) {
  const calls: { setting: string; value?: boolean; request_id?: string | null }[] = [];
  const port: HgsSettingPort = {
    call: vi.fn(async (call) => {
      calls.push(call);
      const next: HgsSettingOutcome = outcomes.shift() ?? { outcome: "done", view: { setting: call.setting, value: call.value ?? true } };
      return next;
    }),
  };
  return { calls, port };
}

describe("the hgs-owned settings", () => {
  it("shows the value hgs read", async () => {
    const f = fake({ outcome: "done", view: { setting: "subagents-confirm", value: false } });
    const model = createHgsSetting("subagents-confirm", true, f.port);
    expect(model.value()).toBe(true);
    await model.load();
    expect(f.calls).toEqual([{ setting: "subagents-confirm" }]);
    expect(model.value()).toBe(false);
    expect(model.error()).toBe("");
    expect(model.busy()).toBe(false);
  });

  it("keeps the default and says why when hgs cannot be read", async () => {
    const f = fake({ outcome: "failed", detail: "hgs: permission denied" });
    const model = createHgsSetting("session-upload-block", true, f.port);
    await model.load();
    expect(model.value()).toBe(true);
    expect(model.error()).toBe("hgs: permission denied");
  });

  it("writes the person's value with a request id and takes hgs's answer", async () => {
    const f = fake({ outcome: "done", view: { setting: "session-upload-block", value: false } });
    const model = createHgsSetting("session-upload-block", true, f.port);
    await model.set(false);
    expect(f.calls).toHaveLength(1);
    expect(f.calls[0].setting).toBe("session-upload-block");
    expect(f.calls[0].value).toBe(false);
    expect(f.calls[0].request_id).toMatch(/^[0-9a-f-]{36}$/i);
    expect(model.value()).toBe(false);
    expect(model.error()).toBe("");
  });

  it("changes nothing when hgs refuses the write", async () => {
    const f = fake({ outcome: "failed", detail: "hgs: only the owner changes this setting" });
    const model = createHgsSetting("subagents-confirm", true, f.port);
    await model.set(false);
    expect(model.value()).toBe(true);
    expect(model.error()).toBe("hgs: only the owner changes this setting");
  });

  it("says why when the desktop cannot run hgs at all", async () => {
    const port: HgsSettingPort = { call: async () => { throw new Error("hgs settings need the desktop app."); } };
    const model = createHgsSetting("subagents-confirm", true, port);
    await model.set(false);
    expect(model.value()).toBe(true);
    expect(model.error()).toBe("hgs settings need the desktop app.");
  });
});
