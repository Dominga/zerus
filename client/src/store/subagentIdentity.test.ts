import { describe, expect, it, vi } from "vitest";
import { createSubagentIdentity, type ChildTarget } from "./subagentIdentity";
import { sessionIdentity } from "../ui/sessions/actions-ui";
import { toSession } from "./catalog";

const parent = toSession("mac", "run:r1", { name: "codex/sample-project/review", run_id: "r1", conversation_id: "parent" });
const opened = (agentId = "child", extra: Partial<ChildTarget> = {}): ChildTarget => ({
  target: { computer: "mac", name: parent.name, run_id: "r1", conversation_id: "parent" }, archiveId: null, agentId, ...extra,
});
const tick = async () => { for (let i = 0; i < 6; i++) await Promise.resolve(); };

describe("open subagent identity", () => {
  it("fills the header from the child's actual inspection, without borrowing the parent's ID", async () => {
    const read = vi.fn().mockResolvedValue({ agent_id: "child", conversation_id: "child-conversation" });
    const store = createSubagentIdentity(read);
    store.follow(opened());
    expect(sessionIdentity(parent, undefined, store.child())).toEqual({ value: "child", title: "Agent ID", caption: "Copy agent ID" });
    await tick();
    expect(read).toHaveBeenCalledExactlyOnceWith(opened().target, null, "child");
    expect(store.child()).toEqual({ id: "child", conversationId: "child-conversation" });
    expect(sessionIdentity(parent, undefined, store.child())).toEqual({ value: "child-conversation", title: "Session ID", caption: "Copy session ID" });
    store.follow(opened()); await tick(); expect(read).toHaveBeenCalledTimes(1);
  });
  it("reads at most one child at a time and discards a late answer after switching", async () => {
    let resolve!: (value: unknown) => void;
    const read = vi.fn().mockImplementationOnce(() => new Promise((r) => { resolve = r; }))
      .mockResolvedValue({ agent_id: "newest", conversation_id: "newest-conversation" });
    const store = createSubagentIdentity(read);
    store.follow(opened()); store.follow(opened("skipped")); store.follow(opened("newest"));
    expect(read).toHaveBeenCalledTimes(1);
    resolve({ agent_id: "child", conversation_id: "late-conversation" }); await tick();
    expect(read).toHaveBeenCalledTimes(2);
    expect(read).toHaveBeenLastCalledWith(opened().target, null, "newest");
    expect(store.child()).toEqual({ id: "newest", conversationId: "newest-conversation" });
  });
  it("does not retry failed reads on details updates, and never accepts another child", async () => {
    const read = vi.fn().mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValue({ agent_id: "another", conversation_id: "wrong-conversation" });
    const store = createSubagentIdentity(read);
    store.follow(opened()); await tick(); store.follow(opened()); await tick();
    expect(read).toHaveBeenCalledTimes(1); expect(store.child()).toEqual({ id: "child" });
    store.follow(undefined); store.follow(opened()); await tick();
    expect(store.child()).toEqual({ id: "child" });
  });
  it("pins parent, archive and child selection and ignores reads after closing", async () => {
    let resolve!: (value: unknown) => void;
    const read = vi.fn().mockImplementation(() => new Promise((r) => { resolve = r; }));
    const store = createSubagentIdentity(read);
    store.follow(opened("child", { archiveId: "archive-1" }));
    expect(read).toHaveBeenCalledWith(opened().target, "archive-1", "child");
    store.follow(undefined); resolve({ agent_id: "child", conversation_id: "old-conversation" }); await tick();
    expect(store.child()).toBeUndefined(); expect(read).toHaveBeenCalledTimes(1);
    const next = opened("child", { target: { ...opened().target, run_id: "r2", conversation_id: "next-parent" } });
    store.follow(next); expect(read).toHaveBeenLastCalledWith(next.target, null, "child");
  });
});
