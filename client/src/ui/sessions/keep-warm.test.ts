import { describe, expect, it } from "vitest";
import { DEFAULT_HOURS, KEEPALIVE_ENVELOPE, hoursOf, isKeepalive, parseHours, sliderHours, toggledHours, warmable } from "./keep-warm";
import { buildTranscript, timeText } from "./transcript";
import type { ActivityEvent } from "../../types";
import type { ActivityItem } from "./types";

const A = "a".repeat(64), B = "b".repeat(64);

describe("Keep cache warm's saved hours (CacheKeepalive::hours)", () => {
  it("reads Qt's JSON object as Qt reads it, clamped to 0..24", () => {
    const saved = parseHours(JSON.stringify({ [A]: 3, [B]: 30, c: 2.5, d: "4", e: -1, f: 0 }));
    expect([...saved]).toEqual([[A, 3], [B, 24]]);
    for (const broken of [undefined, "", "  ", "[]", "[1]", "null", "not json", "5", 3]) expect(parseHours(broken).size).toBe(0);
  });

  it("finds a session by its exact identity only", () => {
    const saved = parseHours(JSON.stringify({ [A]: 3, [B]: 5 }));
    expect(hoursOf(saved, [A, B])).toBe(3);
    expect(hoursOf(saved, [B, A])).toBe(5);
    expect(hoursOf(saved, ["c".repeat(64), A])).toBe(0);
    expect(hoursOf(saved, undefined)).toBe(0);
    expect(hoursOf(parseHours(JSON.stringify({ short: 2 })), ["short", A])).toBe(0);
  });

  it("is offered for a listed conversation, never an archived one", () => {
    expect(warmable({ conversationId: "c", state: "running", compactKeys: [A, B] })).toBe(true);
    expect(warmable({ conversationId: "c", state: "paused", compactKeys: [A, B] })).toBe(true);
    expect(warmable({ conversationId: "c", state: "archived", compactKeys: [A, B] })).toBe(false);
    expect(warmable({ conversationId: "", state: "running", compactKeys: [A, B] })).toBe(false);
    expect(warmable({ conversationId: "c", state: "running" })).toBe(false);
  });

  it("toggles off or one hour, and the slider shows 1 to 8", () => {
    expect(toggledHours(0)).toBe(DEFAULT_HOURS);
    expect(toggledHours(5)).toBe(0);
    expect([0, 1, 5, 8, 24].map(sliderHours)).toEqual([1, 1, 5, 8, 8]);
  });

  it("recognizes hgs's notice and the message in a native history", () => {
    expect(isKeepalive({ origin: "cache_keepalive", detail: "Cache warmed" }, "notice")).toBe(true);
    expect(isKeepalive({ origin: "peer_message", detail: "hi" }, "notice")).toBe(false);
    expect(isKeepalive({ detail: `${KEEPALIVE_ENVELOPE}Reply with exactly: ok</zerus-cache-keepalive>` }, "user")).toBe(true);
    expect(isKeepalive({ detail: `quoted ${KEEPALIVE_ENVELOPE}` }, "user")).toBe(false);
    expect(isKeepalive({ detail: KEEPALIVE_ENVELOPE }, "assistant")).toBe(false);
  });
});

describe("kept-warm caches in Activity (ActivityView.cpp:1239-1251, 1367-1370)", () => {
  const notice = (seq: number, at: number): ActivityEvent => ({ seq, at, type: "UserPromptSubmit", origin: "cache_keepalive", detail: "Cache warmed" });
  const reply = (seq: number, at: number, detail = "ok"): ActivityEvent => ({ seq, at, type: "Stop", detail });

  it("keepalives in a row are one quiet line without their replies, at the last one's time", () => {
    const events: ActivityEvent[] = [
      { seq: 1, at: 10, type: "UserPromptSubmit", detail: "build it" }, reply(2, 20, "done"),
      notice(3, 300), reply(4, 301), notice(5, 600), reply(6, 601),
      { seq: 7, at: 900, type: "UserPromptSubmit", detail: "next" },
    ];
    const items = buildTranscript(events) as ActivityItem[];
    expect(items.map((i) => [i.kind, i.body])).toEqual([
      ["user", "build it"], ["agent", "done"], ["context", "Cache warmed 2 times while idle"], ["user", "next"],
    ]);
    expect(items[2].at).toBe(timeText(600));
  });

  it("one keepalive reads Cache warmed while idle, also from its message text", () => {
    const native: ActivityEvent[] = [
      { seq: 1, at: 50, type: "UserPromptSubmit", detail: `${KEEPALIVE_ENVELOPE}Zerus keeps this conversation's prompt cache warm. Reply with exactly: ok</zerus-cache-keepalive>` },
      { seq: 2, at: 51, type: "AgentMessage", detail: "ok" }, reply(3, 52),
    ];
    const items = buildTranscript(native) as ActivityItem[];
    expect(items.map((i) => [i.kind, i.body, i.at])).toEqual([["context", "Cache warmed while idle", timeText(50)]]);
  });

  it("an event between two keepalives keeps them apart", () => {
    const events: ActivityEvent[] = [notice(1, 300), reply(2, 301), { seq: 3, at: 400, type: "PreToolUse", tool: "Bash", detail: "ls" }, notice(4, 600)];
    const items = buildTranscript(events) as ActivityItem[];
    expect(items.map((i) => [i.kind, i.body])).toEqual([
      ["context", "Cache warmed while idle"], ["tool", "ls"], ["context", "Cache warmed while idle"],
    ]);
  });
});
