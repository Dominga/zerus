import { createRoot, createSignal } from "solid-js";
import { describe, expect, it, vi } from "vitest";
import { createSubagentActivity, type SubagentActivityTarget } from "./subagentActivity";
import type { SubagentInspection } from "../bridge";
// The real CLI's answer to the core's own call, pinned by
// tests/subagent_activity_contract.rs (zerus-mnck6).
import contract from "../../core/tests/fixtures/subagent-activity.json";

const target = (agentId = "child"): SubagentActivityTarget => ({ id: "\nrun:parent", computer: "", name: "dsh/example/review", runId: "run-parent", conversationId: "conv-parent", archiveId: null, agentId });
const page = (agentId = "child", seq = 1): SubagentInspection => ({
  agent_id: agentId, conversation_id: `conv-parent/${agentId}`, parent_conversation_id: "conv-parent", run_id: "run-parent", send_supported: true,
  history_scope: "Native subagent history", history_truncated: false, events: [{ seq, type: "Stop", detail: `done ${seq}` }],
});
const tick = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };

describe("bounded child Activity", () => {
  it("adopts the real CLI answer for the exact target it was asked for", async () => {
    await createRoot(async (dispose) => {
      const [showing] = createSignal(true);
      const answer = contract.answer as unknown as SubagentInspection;
      const read = vi.fn().mockResolvedValue(answer);
      const store = createSubagentActivity({ read, showing, later: () => () => {} });
      store.follow(contract.target); await tick();
      expect(read).toHaveBeenCalledExactlyOnceWith(contract.target);
      expect(store.error()).toBeUndefined();
      expect(store.loaded()).toBe(true);
      expect(store.summary()).toEqual(answer);
      // The same answer for another run of the parent is still refused.
      store.follow({ ...contract.target, runId: "another-run" }); await tick();
      expect(store.error()).toContain("unavailable");
      dispose();
    });
  });

  it("replaces the page and scopes attachments to the exact child", async () => {
    await createRoot(async (dispose) => {
      const [showing] = createSignal(true), pending: Array<() => void> = [];
      const later = (callback: () => void) => { pending.push(callback); return () => { pending.length = 0; }; };
      const read = vi.fn().mockResolvedValueOnce(page()).mockResolvedValueOnce(page("child", 2));
      const store = createSubagentActivity({ read, showing, later });
      store.follow(target()); await tick();
      expect(store.events().map((event) => event.seq)).toEqual([1]);
      expect(store.attachmentScope?.()).toEqual({ computer: "", name: "dsh/example/review", conversation_id: "conv-parent", archive_id: null, agent_id: "child" });
      pending.shift()!(); await tick();
      expect(read).toHaveBeenCalledTimes(2);
      expect(store.events().map((event) => event.seq)).toEqual([2]);
      expect(store.readOnly).toBe(true);
      store.follow(undefined);
      expect(store.events()).toEqual([]);
      dispose();
    });
  });

  it("drops a late answer and refuses a mismatched child", async () => {
    await createRoot(async (dispose) => {
      const [showing] = createSignal(true), pending: Array<() => void> = [];
      const later = (callback: () => void) => { pending.push(callback); return () => { pending.length = 0; }; };
      let resolveFirst!: (value: SubagentInspection) => void;
      const read = vi.fn().mockImplementationOnce(() => new Promise<SubagentInspection>((resolve) => { resolveFirst = resolve; }))
        .mockResolvedValueOnce(page("other", 2)).mockResolvedValueOnce(page("other", 3)).mockResolvedValueOnce(page("other", 4));
      const store = createSubagentActivity({ read, showing, later });
      store.follow(target()); await tick();
      expect(read).toHaveBeenCalledTimes(1);
      store.follow(target("other")); await tick();
      expect(read).toHaveBeenCalledTimes(2);
      expect(store.events().map((event) => event.seq)).toEqual([2]);
      resolveFirst(page()); await tick();
      expect(store.target()?.agentId).toBe("other");
      expect(store.events().map((event) => event.seq)).toEqual([2]);
      pending.shift()!(); await tick();
      expect(read).toHaveBeenCalledTimes(3);
      expect(store.events().map((event) => event.seq)).toEqual([3]);
      store.follow(target("wrong")); await tick();
      expect(store.error()).toContain("unavailable");
      expect(store.events()).toEqual([]);
      dispose();
    });
  });
});
