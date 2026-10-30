import { afterEach, describe, expect, it, vi } from "vitest";
import { createContentSearch, identityMatches, metadataMatches, mergeSessionMatch, searchCount, SEARCH_HITS, SEARCH_HOSTS, type SearchHit, type SearchReply } from "./content-search-model";
import type { Session } from "../ui/sessions/types";

const host = (n: number, online = true) => ({ computer: `host-${n}`, machine: `machine-${n}`, online });
const hit = (id: string): SearchHit => ({ name: "claude/demo/sample", run_id: "run-one", conversation_id: "conversation-one", archive_id: "", message_id: id, at: 10, role: "assistant", snippet: "needle", event: { type: "AgentMessage", detail: "needle" }, content_truncated: false, session: null, group: "" });
function fixture() {
  vi.useFakeTimers();
  const calls: { computer: string; query: string; generation: number; resolve(reply: SearchReply): void; reject(error: string): void }[] = [];
  const cancelled: number[] = [];
  const search = createContentSearch({ firstGeneration: 1, cancel: (token) => cancelled.push(token),
    read: (computer, query, generation) => new Promise((resolve, reject) => calls.push({ computer, query, generation, resolve, reject })) });
  const answer = (index: number, results: SearchHit[] = [hit(String(index))], extra = {}) => calls[index].resolve({ query: calls[index].query, results, truncated: false, unavailable_sessions: 0, ...extra });
  return { search, calls, cancelled, answer };
}
const tick = () => vi.advanceTimersByTimeAsync(0);
afterEach(() => vi.useRealTimers());
describe("bounded public-content search", () => {
  it("matches public metadata and exact historical identities without rebinding a same-name run", () => {
    const row = { name: "claude/demo/sample", title: "Demo", machine: "workstation", model: "sample-model", cwdPath: "/example/project", runId: "run-one", conversationId: "conversation-one", subagents: [{ id: "child-one", name: "child", state: "ready", summary: "" }] } as unknown as Session;
    expect(metadataMatches(row, "CONVERSATION-ONE")).toBe(true);
    expect(metadataMatches(row, "CHILD-ONE")).toBe(true);
    expect(metadataMatches(row, "missing")).toBe(false);
    expect(identityMatches(row, hit("one"))).toBe(true);
    expect(identityMatches({ ...row, runId: "replacement" }, hit("one"))).toBe(false);
    expect(identityMatches({ ...row, conversationId: "replacement" }, hit("one"))).toBe(false);
    expect(identityMatches({ ...row, archiveId: "other-archive" }, hit("one"))).toBe(false);
  });
  it("waits 350 ms and two characters, then runs four computers at most", async () => {
    const f = fixture(); const hosts = Array.from({ length: 8 }, (_, n) => host(n));
    f.search.configure("a", hosts, true); await vi.advanceTimersByTimeAsync(1000); expect(f.calls).toHaveLength(0);
    f.search.configure("needle", hosts, true); await vi.advanceTimersByTimeAsync(349); expect(f.calls).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(1); expect(f.calls).toHaveLength(4);
    f.answer(0); await tick(); expect(f.calls).toHaveLength(5);
    f.search.dispose();
  });
  it("keeps cancelled jobs in their lanes and never lets a stale query replace the current one", async () => {
    const f = fixture(), hosts = Array.from({ length: 5 }, (_, n) => host(n));
    f.search.configure("older", hosts, true); await vi.advanceTimersByTimeAsync(350);
    f.search.configure("newer", hosts, true); await vi.advanceTimersByTimeAsync(350); expect(f.calls).toHaveLength(4);
    expect(f.cancelled).toContain(f.calls[0].generation);
    f.answer(0); await tick(); expect(f.calls).toHaveLength(5); expect(f.calls[4].query).toBe("newer"); expect(f.search.hits()).toHaveLength(0);
    f.answer(4, [hit("current")]); await tick(); expect(f.search.hits().map((found) => found.hit.message_id)).toEqual(["current"]);
    for (let n = 1; n < 4; n++) f.answer(n); await tick(); expect(f.search.hits()).toHaveLength(1);
    f.search.dispose();
  });
  it("caps retained excerpts and computers and explicitly reports partial or unavailable history", async () => {
    const f = fixture(); const hosts = Array.from({ length: SEARCH_HOSTS + 2 }, (_, n) => host(n, n !== 0));
    f.search.configure("needle", hosts, true); await vi.advanceTimersByTimeAsync(350);
    expect(f.search.errors()[0]).toContain("machine offline; contents unavailable"); expect(f.search.limited()).toBe(true);
    for (let n = 0; n < 4; n++) f.answer(n, Array.from({ length: 100 }, (_, i) => hit(`${n}-${i}`)), { truncated: true, unavailable_sessions: 2 });
    await tick(); expect(f.search.hits()).toHaveLength(SEARCH_HITS); expect(f.search.unavailable()).toBe(8);
    f.search.dispose();
  });
  it("cancels on hide and accepts a reconnect as a fresh generation", async () => {
    const f = fixture(); f.search.configure("needle", [host(0, false)], true); await vi.advanceTimersByTimeAsync(350); expect(f.calls).toHaveLength(0);
    f.search.configure("needle", [host(0)], true); await vi.advanceTimersByTimeAsync(350); expect(f.calls).toHaveLength(1);
    f.search.configure("needle", [host(0)], false); f.answer(0); await tick(); expect(f.search.hits()).toHaveLength(0); expect(f.search.busy()).toBe(false);
    f.search.configure("needle", [host(0)], true); await vi.advanceTimersByTimeAsync(350); expect(f.calls).toHaveLength(2);
    f.search.dispose();
  });
});

it("shows mixed cold coverage inline and keeps fresh results available", async () => {
  const f = fixture(); f.search.configure("needle", [host(0)], true); await vi.advanceTimersByTimeAsync(350);
  f.answer(0, [hit("fresh")], { contents_unavailable: true }); await tick();
  expect(f.search.hits()).toHaveLength(1);
  expect(f.search.errors()).toEqual(["machine-0: contents unavailable"]);
  f.search.dispose();
});
it("keeps one session row and its position while metadata and matching messages arrive", () => {
  const session = { key: "one", name: "example" } as Session;
  const items: { id: string; session: Session; found?: { computer: string; machine: string; hit: SearchHit } }[] = [];
  const positions = new Map<string, number>();
  const found = (id: string, at: number) => ({ computer: "", machine: "example", hit: { ...hit(id), at } });
  mergeSessionMatch(items, positions, { id: "stable", session }, 2);
  mergeSessionMatch(items, positions, { id: "other", session: { ...session, key: "two" } }, 2);
  mergeSessionMatch(items, positions, { id: "message", session, found: found("new", 20) }, 2);
  mergeSessionMatch(items, positions, { id: "late-page", session, found: found("old", 10) }, 2);
  expect(items.map((row) => row.id)).toEqual(["stable", "other"]);
  expect(items[0].found?.hit.message_id).toBe("new");
  expect(mergeSessionMatch(items, positions, { id: "overflow", session: { ...session, key: "three" } }, 2)).toBe(false);
  expect(searchCount(1, 1)).toBe("1 message, 1 session"); expect(searchCount(2, 2)).toBe("2 messages, 2 sessions");
});
