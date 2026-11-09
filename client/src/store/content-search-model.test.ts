import { afterEach, describe, expect, it, vi } from "vitest";
import { coverageNotes, createContentSearch, identityMatches, matchedIdentity, metadataMatches, searchCount, searchRows, searchTooltip, SESSION_MESSAGES, SEARCH_HITS, SEARCH_HOSTS, type SearchHit, type SearchReply } from "./content-search-model";
import type { Session } from "../ui/sessions/types";
import { translateWithCatalog } from "../i18n";
import ru from "../i18n.ru.json";

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
  it("counts matching creations after messages and sessions, as Qt does", () => {
    expect(searchCount(3, 2)).toBe("3 messages, 2 sessions");
    expect(searchCount(1, 1, 1)).toBe("1 message, 1 session, 1 creation");
    expect(searchCount(0, 0, 2)).toBe("0 messages, 0 sessions, 2 creations");
  });
  it("counts in Russian with its plural forms, creations included (zerus-5wmh.13.17.3.1)", () => {
    const russian = (source: string, ...args: (string | number)[]) => translateWithCatalog(ru, "ru", source, ...args);
    expect(searchCount(1, 1, 1, russian)).toBe("1 сообщение, 1 сессия, 1 создание");
    expect(searchCount(3, 2, 4, russian)).toBe("3 сообщения, 2 сессии, 4 создания");
    expect(searchCount(5, 11, 21, russian)).toBe("5 сообщений, 11 сессий, 21 создание");
    expect(searchCount(0, 0, 0, russian)).toBe("0 сообщений, 0 сессий");
  });
  it("matches a child's conversation ID and names the ID that matched, as Qt does (SessionSearch.cpp:239-268)", () => {
    const row = { name: "claude/demo/sample", title: "Demo", machine: "workstation", runId: "run-one", conversationId: "conversation-one", archiveId: "archive-one",
      subagents: [{ id: "agent-one", conversationId: "child-conversation-one", name: "child", state: "ready", summary: "" }, { id: "agent-two", name: "other", state: "ready", summary: "" }] } as unknown as Session;
    expect(metadataMatches(row, "CHILD-CONVERSATION")).toBe(true);
    expect(matchedIdentity(row, "child-conversation")).toBe("Child session ID: child-conversation-one");
    expect(matchedIdentity(row, "AGENT-TWO")).toBe("Agent ID: agent-two");
    // The first in Qt's order: session, run, archive, then each child's agent and conversation IDs.
    expect(matchedIdentity(row, "-one")).toBe("Session ID: conversation-one");
    expect(matchedIdentity(row, "run-")).toBe("Run ID: run-one");
    expect(matchedIdentity(row, "archive-")).toBe("Archive ID: archive-one");
    expect(matchedIdentity(row, "agent-one")).toBe("Agent ID: agent-one");
    // Names, titles and models are not identities.
    expect(matchedIdentity(row, "demo")).toBeUndefined();
    expect(matchedIdentity(row, "workstation")).toBeUndefined();
  });
  it("gives a session's message rows its matched ID for the tooltip, and lists the IDs there", () => {
    const row = { key: "k", name: "claude/demo/sample", title: "Demo", machine: "workstation", runId: "run-one", conversationId: "conversation-one" } as unknown as Session;
    const { rows } = searchRows([
      { id: "metadata:k", session: row, matched: "Run ID: run-one" },
      { id: "k\nm1", session: row, found: { computer: "", machine: "workstation", hit: hit("m1") } },
    ]);
    expect(rows.map((r) => [r.id, r.matched])).toEqual([["metadata:k", "Run ID: run-one"], ["k\nm1", "Run ID: run-one"]]);
    expect(searchTooltip(rows[1])).toBe("claude/demo/sample\nDemo\nworkstation\nSession ID: conversation-one\nRun ID: run-one\nneedle");
    expect(searchTooltip({ session: row, matched: "Child session ID: child-one" })).toBe("claude/demo/sample\nDemo\nworkstation\nSession ID: conversation-one\nRun ID: run-one\nChild session ID: child-one");
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

it("names why a host's contents are partial (task 77)", async () => {
  const f = fixture(); f.search.configure("needle", [host(0)], true); await vi.advanceTimersByTimeAsync(350);
  // This computer's index refused its folder: hgs answered from its recent-history scan.
  f.answer(0, [hit("fresh")], { truncated: true, unavailable_sessions: 5, contents_unavailable: true, index_state: "unavailable" }); await tick();
  expect(f.search.hits()).toHaveLength(1); expect(f.search.limited()).toBe(true);
  expect(f.search.errors()).toEqual([
    "machine-0: content index unavailable, recent history searched instead",
    "machine-0: history unavailable in 5 of the searched sessions",
  ]);
  f.search.dispose();
  expect(coverageNotes("studio", { unavailable_sessions: 0, contents_unavailable: true, index_state: "indexing" }))
    .toEqual(["studio: content index still building, recent history searched too"]);
  expect(coverageNotes("studio", { unavailable_sessions: 0, contents_unavailable: false, index_state: "ready" })).toEqual([]);
  expect(coverageNotes("studio", { unavailable_sessions: 0, contents_unavailable: false })).toEqual([]);
});
it("shows mixed cold coverage inline and keeps fresh results available", async () => {
  const f = fixture(); f.search.configure("needle", [host(0)], true); await vi.advanceTimersByTimeAsync(350);
  f.answer(0, [hit("fresh")], { contents_unavailable: true }); await tick();
  expect(f.search.hits()).toHaveLength(1);
  expect(f.search.errors()).toEqual(["machine-0: contents unavailable"]);
  f.search.dispose();
});
describe("one row per matching message (SessionSearch.cpp:276-282, zerus-5wmh.13.17.1)", () => {
  const session = (key: string) => ({ key, name: key } as Session);
  const found = (id: string, at: number) => ({ computer: "", machine: "example", hit: { ...hit(id), at } });
  const message = (key: string, id: string, at: number) => ({ id: `${key}\n${id}`, session: session(key), found: found(id, at) });

  it("keeps each session's rows together: its metadata row, then its messages newest first, sessions in first-seen order", () => {
    const { rows, limited } = searchRows([
      { id: "metadata:one", session: session("one") },
      message("two", "a", 5),
      message("one", "old", 10),
      message("one", "new", 30),
      { id: "metadata:two", session: session("two") },
      message("two", "b", 50),
    ]);
    expect(rows.map((r) => r.id)).toEqual(["metadata:one", "one\nnew", "one\nold", "metadata:two", "two\nb", "two\na"]);
    expect(limited).toBe(false);
  });

  it("keeps rows and their order stable as later replies add messages to a session", () => {
    const first = [{ id: "metadata:one", session: session("one") }, message("two", "a", 5)];
    const later = [...first, message("one", "x", 7), message("three", "c", 1)];
    expect(searchRows(first).rows.map((r) => r.id)).toEqual(["metadata:one", "two\na"]);
    expect(searchRows(later).rows.map((r) => r.id)).toEqual(["metadata:one", "one\nx", "two\na", "three\nc"]);
  });

  it("shows one row per message, at most 20 messages per session, and at most the row cap in all", () => {
    const many = Array.from({ length: SESSION_MESSAGES + 5 }, (_, n) => message("one", `m${n}`, n));
    const { rows, limited } = searchRows([...many, message("one", "m3", 3)]);
    expect(rows).toHaveLength(SESSION_MESSAGES);
    expect(new Set(rows.map((r) => r.id)).size).toBe(rows.length);
    expect(rows[0].found?.hit.message_id).toBe(`m${SESSION_MESSAGES + 4}`);
    expect(limited).toBe(true);
    const capped = searchRows([message("a", "1", 1), message("b", "2", 2), message("c", "3", 3)], { rows: 2, perSession: SESSION_MESSAGES });
    expect(capped.rows.map((r) => r.id)).toEqual(["a\n1", "b\n2"]);
    expect(capped.limited).toBe(true);
    expect(searchCount(1, 1)).toBe("1 message, 1 session"); expect(searchCount(2, 2)).toBe("2 messages, 2 sessions");
  });
});
