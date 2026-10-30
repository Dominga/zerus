import { describe, expect, it, vi } from "vitest";
import { createRoot } from "solid-js";
import { createActivity, type FollowRequest } from "./activity";
import { buildTranscript } from "../ui/sessions/transcript";
import type { ActivityEvent, ActivityOut } from "../types";

const target = { id: "\nrun:a", computer: "", key: "run:a", name: "claude/sample-project/a" };
const ev = (seq: number, type = "PostToolUse", extra: Partial<ActivityEvent> = {}): ActivityEvent => ({ seq, type, source: "claude_hooks", tool: "Bash", detail: `step ${seq}`, at: 1_793_000_000 + seq, ...extra });
const out = (kind: string, payload: unknown, key = "run:a"): ActivityOut => ({ computer: "", key, kind, payload });

function setup() {
  const calls: FollowRequest[] = [];
  const unfollows: string[] = [];
  const port = { follow: vi.fn(async (r: FollowRequest) => { calls.push(r); }), unfollow: vi.fn(async (_c: string, k: string) => { unfollows.push(k); }) };
  const store = createActivity(port);
  return { store, calls, unfollows };
}

describe("activity store", () => {
  it("follows with the tail, merges events by seq and ignores other sessions", () => {
    createRoot((dispose) => {
      const { store, calls } = setup();
      store.follow(target);
      expect(calls).toEqual([{ computer: "", key: "run:a", name: target.name, run_id: "a", conversation_id: undefined, after: undefined, tail: 100 }]);
      store.applyActivity([out("events", { events: [ev(5), ev(7)], cursor: 7 })]);
      store.applyActivity([out("events", { events: [ev(7), ev(9)], cursor: 9 }), out("events", { events: [ev(1)], cursor: 1 }, "run:other")]);
      expect(store.events().map((e) => e.seq)).toEqual([5, 7, 9]);
      expect(store.loaded()).toBe(true);
      dispose();
    });
  });

  it("reads its attachments in the inspected conversation, else the row's", () => {
    createRoot((dispose) => {
      const { store } = setup();
      expect(store.attachmentScope()).toBeUndefined();
      store.follow({ ...target, conversationId: "conv-row" });
      expect(store.attachmentScope()).toEqual({ computer: "", name: target.name, conversation_id: "conv-row", archive_id: null });
      store.applyActivity([out("state", { value: { conversation_id: "conv-inspected" } })]);
      expect(store.attachmentScope()?.conversation_id).toBe("conv-inspected");
      dispose();
    });
  });

  it("reads older pages with `after` and doubles the reach after an empty read", () => {
    createRoot((dispose) => {
      const { store, calls } = setup();
      store.follow(target);
      store.applyActivity([out("events", { events: [ev(900), ev(950)], cursor: 950 })]);

      store.loadOlder();
      expect(calls.at(-1)?.after).toBe(700);
      expect(store.loadingOlder()).toBe(true);
      store.applyActivity([out("events", { events: [ev(750), ev(900), ev(950)], cursor: 950 })]);
      expect(store.loadingOlder()).toBe(false);
      expect(store.events()[0].seq).toBe(750);

      // Nothing older within 200 rows: the next read reaches 400 back.
      store.loadOlder();
      expect(calls.at(-1)?.after).toBe(550);
      store.applyActivity([out("events", { events: [ev(750)], cursor: 950 })]);
      store.loadOlder();
      expect(calls.at(-1)?.after).toBe(350);
      dispose();
    });
  });

  it("stops reading older pages once a read from the start brings nothing", () => {
    createRoot((dispose) => {
      const { store, calls } = setup();
      store.follow(target);
      store.applyActivity([out("events", { events: [ev(120)], cursor: 120 })]);
      store.loadOlder();
      expect(calls.at(-1)?.after).toBe(0);
      store.applyActivity([out("events", { events: [ev(120)], cursor: 120 })]);
      expect(store.olderDone()).toBe(true);
      const before = calls.length;
      store.loadOlder();
      expect(calls.length).toBe(before);
      dispose();
    });
  });

  it("replaces the window, keeps the newest state and marks the end", () => {
    createRoot((dispose) => {
      const { store } = setup();
      store.follow(target);
      store.applyActivity([out("events", { events: [ev(1), ev(2)], cursor: 2 })]);
      store.applyActivity([out("replace", { events: [ev(10)], cursor: 10 }), out("state", { value: { phase: "tool" }, rev: 3 }), out("state", { value: { phase: "idle" }, rev: 2 })]);
      expect(store.events().map((e) => e.seq)).toEqual([10]);
      expect(store.summary()).toEqual({ phase: "tool" });
      store.applyActivity([out("ended", null)]);
      expect(store.ended()).toBe(true);
      dispose();
    });
  });

  it("unfollows the previous session when the selection changes", () => {
    createRoot((dispose) => {
      const { store, unfollows } = setup();
      store.follow(target);
      store.applyActivity([out("events", { events: [ev(1)], cursor: 1 })]);
      store.follow({ ...target, id: "\nrun:b", key: "run:b" });
      expect(unfollows).toEqual(["run:a"]);
      expect(store.events()).toEqual([]);
      dispose();
    });
  });
});

describe("transcript", () => {
  it("builds messages, receipts and tool groups as the Qt Activity does", () => {
    const items = buildTranscript([
      ev(1, "UserPromptSubmit", { detail: "Fix the cursor" }),
      ev(2, "PreToolUse", { tool: "Read" }),
      ev(3, "PostToolUse", { tool: "Read" }),
      ev(4, "PreToolUse", { tool: "Bash" }),
      ev(5, "PostToolUseFailure", { tool: "Bash", detail: "exit status 1" }),
      ev(6, "AgentThinking", { detail: "The empty page needs a branch." }),
      ev(7, "Stop", { detail: "Fixed." }),
      ev(8, "UserPromptQueued", { detail: "Also run the docs check" }),
      ev(9, "UserPromptSubmit", { origin: "peer_message", sender: "codex/p/review", detail: "Rebase first" }),
    ]);
    expect(items.map((i) => i.kind)).toEqual(["user", "tools", "tool", "thinking", "agent", "user", "system"]);
    const [user, tools, failure] = items as { title?: string; body?: string; receipt?: string; warn?: boolean }[];
    expect(user.receipt).toBe("Sent");
    expect(tools.title).toBe("2 tool calls, 3 events");
    expect(tools.body).toBe("Read, Bash");
    expect(failure.title).toBe("Tool failed / Bash");
    expect(failure.warn).toBe(true);
    expect((items[5] as { receipt?: string }).receipt).toBe("Queued at this time");
    expect((items[6] as { sender?: string }).sender).toBe("Message from codex/p/review");
  });
});

describe("question replies in the transcript", () => {
  it("answers are labelled and keep their questions", () => {
    const items = buildTranscript([
      { seq: 1, type: "QuestionAnswered", at: 10, detail: "Which checks? → Unit" },
      { seq: 2, type: "UserPromptSubmit", at: 11, detail: "<send_user_message_question_reply>…", question_replies: [{ question: "Which checks?", answer: "Unit" }] },
    ] as ActivityEvent[]);
    expect(items.map((i) => (i as { label?: string }).label)).toEqual(["You (answer)", "You (answer)"]);
    expect((items[1] as { replies?: unknown }).replies).toEqual([{ question: "Which checks?", answer: "Unit" }]);
  });

  it("a recorded prompt with answers reads as a recorded answer", () => {
    const items = buildTranscript([], { prompt: "<send_user_message_question_reply>…", prompt_replies: [{ question: "Q", answer: "A" }] });
    const prompt = items.find((i) => i.id === "prompt-snapshot") as { label?: string; replies?: unknown };
    expect(prompt.label).toBe("You (recorded answer)");
    expect(prompt.replies).toEqual([{ question: "Q", answer: "A" }]);
  });
});
