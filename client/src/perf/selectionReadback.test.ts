import { createRoot, createSignal } from "solid-js";
import { describe, expect, it } from "vitest";
import { observeSelection, type SelectionObservation } from "./selectionReadback";

const selected = (name: string): SelectionObservation => ({ page: "sessions", detail_tab: "activity", session: { computer: "", name, run_id: `run-${name}`, conversation_id: `conversation-${name}` } });
const flush = () => new Promise<void>((resolve) => queueMicrotask(resolve));

describe("acceptance selection readback", () => {
  it("writes nothing without native opt-in; observes actual session changes and clearing", async () => {
    const sent: SelectionObservation[] = [];
    const [enabled, enable] = createSignal(false), [value, select] = createSignal(selected("first"));
    const dispose = createRoot((dispose) => { observeSelection(enabled, value, async (v) => { sent.push(v); }); return dispose; });
    await flush(); expect(sent).toEqual([]);
    enable(true); await flush(); expect(sent[0].session?.run_id).toBe("run-first");
    select(selected("second")); await flush(); expect(sent.at(-1)?.session?.conversation_id).toBe("conversation-second");
    select({ page: "projects", detail_tab: "activity", session: null }); await flush();
    expect(sent.at(-1)?.session).toBeNull();
    dispose();
  });
  it("retains one in-flight write and only the newest pending observation", async () => {
    const sent: (SelectionObservation & { sequence: number })[] = [];
    let release!: () => void;
    const held = new Promise<void>((resolve) => { release = resolve; });
    const [value, select] = createSignal(selected("first"));
    const dispose = createRoot((dispose) => {
      observeSelection(() => true, value, async (v) => { sent.push(v); if (sent.length === 1) await held; }); return dispose;
    });
    await flush(); select(selected("obsolete")); await flush(); select(selected("latest")); await flush();
    expect(sent).toHaveLength(1);
    release(); await flush(); await flush();
    expect(sent.map((v) => v.session?.name)).toEqual(["first", "latest"]);
    expect(sent[1].sequence).toBeGreaterThan(sent[0].sequence);
    select(selected("latest")); await flush(); expect(sent).toHaveLength(2);
    dispose(); select(selected("after-close")); await flush(); expect(sent).toHaveLength(2);
  });
});
