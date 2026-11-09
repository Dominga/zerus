import { createRoot, createSignal } from "solid-js";
import { describe, expect, it, vi } from "vitest";
import { createSubagentActivity, SUBAGENT_POLL_MS, SUBAGENT_READ_DEADLINE_MS, type SubagentActivityTarget } from "./subagentActivity";
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
/** Timers a test fires by hand; a cancelled one leaves the list. */
const clock = () => {
  const timers: Array<{ callback: () => void; ms: number }> = [];
  return {
    timers,
    later: (callback: () => void, ms: number) => { const timer = { callback, ms }; timers.push(timer); return () => { timers.splice(timers.indexOf(timer), 1); }; },
    fire: (ms: number) => { const at = timers.findIndex((timer) => timer.ms === ms); expect(at, `a ${ms} ms timer`).toBeGreaterThanOrEqual(0); timers.splice(at, 1)[0].callback(); },
  };
};

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

  it("gives each child its own display identity, apart from its parent's row", async () => {
    await createRoot(async (dispose) => {
      const [showing] = createSignal(true);
      const store = createSubagentActivity({ read: vi.fn().mockResolvedValue(page()), showing, later: () => () => {} });
      expect(store.displayKey()).toBeUndefined();
      store.follow(target("child"));
      const first = store.displayKey();
      expect(first).toBeTruthy();
      expect(first).not.toBe(target("child").id);
      store.follow(target("other"));
      expect(store.displayKey()).not.toBe(first);
      store.follow(undefined);
      expect(store.displayKey()).toBeUndefined();
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

// zerus-l7n54: "Reading activity…" stays until the store is loaded, so every way
// of not reading has to end in a loaded state with a reason.
describe("a child Activity that cannot be read says so", () => {
  it("reports an offline computer without reading, and reads when it returns", async () => {
    await createRoot(async (dispose) => {
      const [showing] = createSignal(true), [online, setOnline] = createSignal(false);
      const read = vi.fn().mockResolvedValue(page());
      const store = createSubagentActivity({ read, showing, online, later: () => () => {} });
      store.follow(target()); await tick();
      expect(read).not.toHaveBeenCalled();
      expect(store.loaded()).toBe(true);
      expect(store.error()).toContain("offline");
      setOnline(true); await tick();
      expect(read).toHaveBeenCalledTimes(1);
      expect(store.error()).toBeUndefined();
      expect(store.events().map((event) => event.seq)).toEqual([1]);
      dispose();
    });
  });

  it("keeps the page it has and says why it stopped when the computer goes offline", async () => {
    await createRoot(async (dispose) => {
      const [showing] = createSignal(true), [online, setOnline] = createSignal(true), time = clock();
      const read = vi.fn().mockResolvedValue(page());
      const store = createSubagentActivity({ read, showing, online, later: time.later });
      store.follow(target()); await tick();
      setOnline(false); await tick();
      expect(store.error()).toContain("offline");
      expect(store.events().map((event) => event.seq)).toEqual([1]);
      time.fire(SUBAGENT_POLL_MS); await tick();
      expect(read).toHaveBeenCalledTimes(1);
      dispose();
    });
  });

  it("says the computer went offline at once with a read out, and starts no second read", async () => {
    await createRoot(async (dispose) => {
      const [showing] = createSignal(true), [online, setOnline] = createSignal(true), time = clock();
      let answer!: (value: SubagentInspection) => void;
      const read = vi.fn().mockImplementationOnce(() => new Promise<SubagentInspection>((resolve) => { answer = resolve; })).mockResolvedValue(page("child", 2));
      const store = createSubagentActivity({ read, showing, online, later: time.later });
      store.follow(target()); await tick();
      expect(store.loaded()).toBe(false);
      setOnline(false); await tick();
      expect(store.error()).toContain("offline");
      expect(store.loaded()).toBe(true);
      expect(read).toHaveBeenCalledTimes(1);
      // Its answer is still this child's page, and it doesn't end the reason.
      answer(page("child", 1)); await tick();
      expect(store.events().map((event) => event.seq)).toEqual([1]);
      expect(store.error()).toContain("offline");
      time.fire(SUBAGENT_POLL_MS); await tick();
      expect(read).toHaveBeenCalledTimes(1);
      setOnline(true); await tick();
      expect(read).toHaveBeenCalledTimes(2);
      expect(store.error()).toBeUndefined();
      expect(store.events().map((event) => event.seq)).toEqual([2]);
      dispose();
    });
  });

  it("keeps one read when the computer returns with that read still out", async () => {
    await createRoot(async (dispose) => {
      const [showing] = createSignal(true), [online, setOnline] = createSignal(true);
      let answer!: (value: SubagentInspection) => void;
      const read = vi.fn(() => new Promise<SubagentInspection>((resolve) => { answer = resolve; }));
      const store = createSubagentActivity({ read, showing, online, later: () => () => {} });
      store.follow(target()); await tick();
      setOnline(false); await tick();
      setOnline(true); await tick();
      expect(read).toHaveBeenCalledTimes(1);
      expect(store.error()).toBeUndefined();
      // Nothing has answered yet, so it is reading again, not loaded.
      expect(store.loaded()).toBe(false);
      answer(page()); await tick();
      expect(store.loaded()).toBe(true);
      expect(store.events().map((event) => event.seq)).toEqual([1]);
      dispose();
    });
  });

  it("keeps the deadline of a read that outlasts going offline", async () => {
    await createRoot(async (dispose) => {
      const [showing] = createSignal(true), [online, setOnline] = createSignal(true), time = clock();
      let late!: (value: SubagentInspection) => void;
      const read = vi.fn().mockImplementationOnce(() => new Promise<SubagentInspection>((resolve) => { late = resolve; })).mockResolvedValue(page("child", 2));
      const store = createSubagentActivity({ read, showing, online, later: time.later });
      store.follow(target()); await tick();
      setOnline(false); await tick();
      time.fire(SUBAGENT_READ_DEADLINE_MS); await tick();
      expect(store.error()).toContain("offline");
      setOnline(true); await tick();
      expect(read).toHaveBeenCalledTimes(2);
      expect(store.events().map((event) => event.seq)).toEqual([2]);
      late(page("child", 1)); await tick();
      expect(store.events().map((event) => event.seq)).toEqual([2]);
      dispose();
    });
  });

  it("starts the next child without the last one's failure", async () => {
    await createRoot(async (dispose) => {
      const [showing] = createSignal(true);
      const read = vi.fn().mockResolvedValueOnce(null).mockImplementationOnce(() => new Promise<SubagentInspection>(() => {}));
      const store = createSubagentActivity({ read, showing, later: () => () => {} });
      store.follow(target("one")); await tick();
      expect(store.error()).toContain("unavailable");
      store.follow(target("two")); await tick();
      expect(store.error()).toBeUndefined();
      expect(store.loaded()).toBe(false);
      dispose();
    });
  });

  it("drops the offline reason with the child", async () => {
    await createRoot(async (dispose) => {
      const [showing] = createSignal(true), [online] = createSignal(false);
      const store = createSubagentActivity({ read: vi.fn(), showing, online, later: () => () => {} });
      store.follow(target()); await tick();
      expect(store.error()).toContain("offline");
      store.follow(undefined); await tick();
      expect(store.error()).toBeUndefined();
      expect(store.loaded()).toBe(false);
      dispose();
    });
  });

  it("turns a read that never settles into an error, then reads again", async () => {
    await createRoot(async (dispose) => {
      const [showing] = createSignal(true), time = clock();
      let late!: (value: SubagentInspection) => void;
      const read = vi.fn().mockImplementationOnce(() => new Promise<SubagentInspection>((resolve) => { late = resolve; })).mockResolvedValue(page("child", 2));
      const store = createSubagentActivity({ read, showing, later: time.later });
      store.follow(target()); await tick();
      expect(store.loaded()).toBe(false);
      time.fire(SUBAGENT_READ_DEADLINE_MS); await tick();
      expect(store.loaded()).toBe(true);
      expect(store.error()).toContain("did not finish");
      // The abandoned read's answer changes nothing, late as it is.
      late(page("child", 1)); await tick();
      expect(store.events()).toEqual([]);
      expect(store.error()).toContain("did not finish");
      time.fire(SUBAGENT_POLL_MS); await tick();
      expect(read).toHaveBeenCalledTimes(2);
      expect(store.error()).toBeUndefined();
      expect(store.events().map((event) => event.seq)).toEqual([2]);
      dispose();
    });
  });

  it("gives the native read its whole bound before calling it stuck", async () => {
    // inspect_subagent_activity: 15 s to a peer plus the 2 s drain (details.rs, run.rs).
    expect(SUBAGENT_READ_DEADLINE_MS).toBeGreaterThan(17_000);
    await createRoot(async (dispose) => {
      const [showing] = createSignal(true), time = clock();
      const store = createSubagentActivity({ read: vi.fn().mockResolvedValue(page()), showing, later: time.later });
      store.follow(target()); await tick();
      // An answered read leaves only the next poll; its deadline cannot fire later.
      expect(time.timers.map((timer) => timer.ms)).toEqual([SUBAGENT_POLL_MS]);
      dispose();
    });
  });

  it("leaves no timer behind when the child closes with a read outstanding", async () => {
    await createRoot(async (dispose) => {
      const [showing] = createSignal(true), time = clock();
      const store = createSubagentActivity({ read: vi.fn(() => new Promise<SubagentInspection>(() => {})), showing, later: time.later });
      store.follow(target()); await tick();
      expect(time.timers.map((timer) => timer.ms)).toEqual([SUBAGENT_READ_DEADLINE_MS]);
      store.follow(undefined); await tick();
      expect(time.timers).toEqual([]);
      dispose();
    });
  });
});
