import { describe, expect, it } from "vitest";
import { createRoot } from "solid-js";
import { createSavedDrafts, draftOwnerRow, draftPresentation, draftSubagent, listStep, modifiedText, PAGE, REASONS, replaceText, TEXTS, type RestoreTarget, type SavedDraftsPort } from "./savedDrafts";
import type { DraftSource, SavedDraftDetail, SavedDraftItem } from "../types";
import type { Session } from "../ui/sessions/types";

const item = (key: string, source: DraftSource = "client", extra: Partial<SavedDraftItem> = {}): SavedDraftItem => ({
  key, kind: "message", computer: "", session: key.split("\n")[1] ?? "", title: key.replaceAll("\n", " / "), caption: key, machine: "this computer",
  meta: "", detail: "first line", modified: 1_793_000_000, attachments: 0, source, archived: false, ...extra,
});
const detail = (text: string, extra: Partial<SavedDraftDetail> = {}): SavedDraftDetail => ({ text, attachments: 0, detail: "0 attachments", restorable: true, ...extra });
const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

/** Answers the test gives later, in the order the calls came. */
function later<T>() {
  const waiting: { args: unknown[]; resolve: (v: T) => void; reject: (e: unknown) => void }[] = [];
  return {
    call: (...args: unknown[]) => new Promise<T>((resolve, reject) => { waiting.push({ args, resolve, reject }); }),
    answer: (v: T) => waiting.shift()!.resolve(v),
    fail: (e: unknown) => waiting.shift()!.reject(e),
    args: () => waiting.map((w) => w.args),
    count: () => waiting.length,
  };
}

function fakes() {
  const calls: string[] = [];
  const pages = { client: later<{ items: SavedDraftItem[]; next: string | null }>(), qt: later<{ items: SavedDraftItem[]; next: string | null }>() };
  const conflicts = later<{ items: SavedDraftItem[]; next: number | null }>();
  const details = later<SavedDraftDetail>();
  const recovers = later<void>();
  const restores = later<void>();
  const port: SavedDraftsPort = {
    page: (source, limit, before) => { calls.push(`page ${source} ${limit} ${before}`); return pages[source].call(); },
    conflicts: (limit, after) => { calls.push(`conflicts ${limit} ${after}`); return conflicts.call(); },
    detail: (source, key) => { calls.push(`detail ${source} ${key}`); return details.call(); },
    recover: (key) => { calls.push(`recover ${key}`); return recovers.call(); },
    restore: (handle, source, key) => { calls.push(`restore ${handle} ${source} ${key}`); return restores.call(); },
  };
  return { calls, pages, conflicts, details, recovers, restores, port };
}

const target = (extra: Partial<RestoreTarget> = {}): RestoreTarget => ({ handle: 4, key: "\nclaude/p/open", hasDraft: false, idle: true, ...extra });
const ownerRow = (extra: Partial<Session>) => ({ name: "claude/p/a", computer: "", rawMachine: "studio", state: "running", catalogKey: "run:1", ...extra }) as Session;

describe("Saved drafts (MessageComposer.cpp:1230-1387)", () => {
  it("opens with one bounded page per section and selects the first draft, its preview loading first", async () => {
    const f = fakes();
    await createRoot(async () => {
      const s = createSavedDrafts(f.port);
      s.show(4);
      expect(f.calls).toEqual([`page client ${PAGE} null`, `page qt ${PAGE} null`, `conflicts ${PAGE} 0`]);
      f.pages.qt.answer({ items: [item("\nclaude/p/qt", "qt")], next: null });
      f.pages.client.answer({ items: [item("\nclaude/p/a"), item("\nclaude/p/b")], next: "1:ab" });
      f.conflicts.answer({ items: [], next: null });
      await tick();
      // The Qt page answered first: its draft is selected, as the first listed then.
      expect(s.selected()?.key).toBe("\nclaude/p/qt");
      expect(s.detail()).toBeUndefined();
      f.details.answer(detail("from Qt"));
      await tick();
      expect(s.detail()?.text).toBe("from Qt");
      expect(s.empty()).toBe(false);
    });
  });

  it("Show more reads the next page on request only, and closing drops every row and any late answer", async () => {
    const f = fakes();
    await createRoot(async () => {
      const s = createSavedDrafts(f.port);
      s.show(4);
      f.pages.client.answer({ items: [item("\nclaude/p/a")], next: "7:aa" });
      f.pages.qt.answer({ items: [], next: null });
      f.conflicts.answer({ items: [item("\nclaude/p/c", "qt")], next: 1 });
      await tick();
      expect(f.calls.filter((c) => c.startsWith("page") || c.startsWith("conflicts"))).toHaveLength(3);
      void s.more("client");
      void s.more("client");
      expect(f.calls.filter((c) => c.startsWith("page client"))).toEqual([`page client ${PAGE} null`, `page client ${PAGE} 7:aa`]);
      // A page never repeats a draft the section shows.
      f.pages.client.answer({ items: [item("\nclaude/p/a"), item("\nclaude/p/b")], next: null });
      await tick();
      expect(s.sections.client.items.map((i) => i.key)).toEqual(["\nclaude/p/a", "\nclaude/p/b"]);
      void s.more("client");
      expect(f.calls.filter((c) => c.startsWith("page client"))).toHaveLength(2);
      void s.more("conflicts");
      expect(f.calls.at(-1)).toBe(`conflicts ${PAGE} 1`);
      s.close();
      f.conflicts.answer({ items: [item("\nclaude/p/late", "dev")], next: null });
      f.details.answer(detail("late"));
      await tick();
      expect(s.sections.conflicts.items).toEqual([]);
      expect(s.sections.client.items).toEqual([]);
      expect(s.selected()).toBeUndefined();
      expect(s.detail()).toBeUndefined();
    });
  });

  it("a preview of an earlier selection never replaces the newer one", async () => {
    const f = fakes();
    await createRoot(async () => {
      const s = createSavedDrafts(f.port);
      s.show(4);
      s.select(item("\nclaude/p/a"));
      s.select(item("\nclaude/p/b"));
      f.details.answer(detail("for a"));
      await tick();
      expect(s.detail()).toBeUndefined();
      f.details.answer(detail("for b"));
      await tick();
      expect(s.detail()?.text).toBe("for b");
    });
  });

  it("says when there are no saved drafts at all", async () => {
    const f = fakes();
    await createRoot(async () => {
      const s = createSavedDrafts(f.port);
      s.show(undefined);
      expect(s.empty()).toBe(false);
      f.pages.client.answer({ items: [], next: null });
      f.pages.qt.answer({ items: [], next: null });
      await tick();
      expect(s.empty()).toBe(false);
      f.conflicts.answer({ items: [], next: null });
      await tick();
      expect(s.empty()).toBe(true);
      expect(TEXTS.empty).toBe("No saved drafts on this desktop.");
    });
  });

  it("Restore needs the composer it opened for, idle, and a draft it may restore", async () => {
    const f = fakes();
    await createRoot(async () => {
      const s = createSavedDrafts(f.port);
      s.show(4);
      s.select(item("\nclaude/p/a"));
      expect(s.canRestore(target())).toBe(false);
      f.details.answer(detail("saved"));
      await tick();
      expect(s.canRestore(target())).toBe(true);
      expect(s.canRestore(target({ handle: 5 })), "another composer opened meanwhile").toBe(false);
      expect(s.canRestore(target({ idle: false }))).toBe(false);
      expect(s.canRestore(undefined)).toBe(false);
      s.select(item("voice-message-draft\n\nclaude/p/a"));
      f.details.answer(detail("words", { restorable: false }));
      await tick();
      expect(s.canRestore(target())).toBe(false);
      s.show(undefined);
      s.select(item("\nclaude/p/a"));
      f.details.answer(detail("saved"));
      await tick();
      expect(s.canRestore(target()), "opened without a composer").toBe(false);
    });
  });

  it("asks before it replaces a draft that is not empty, unless it is that very draft", async () => {
    const f = fakes();
    await createRoot(async () => {
      const s = createSavedDrafts(f.port);
      s.show(4);
      s.select(item("\nclaude/p/a"));
      expect(s.asks(target())).toBe(false);
      expect(s.asks(target({ hasDraft: true }))).toBe(true);
      s.select(item("\nclaude/p/open"));
      expect(s.asks(target({ hasDraft: true }))).toBe(false);
      // Qt's copy of the same key is another draft.
      s.select(item("\nclaude/p/open", "qt"));
      expect(s.asks(target({ hasDraft: true }))).toBe(true);
      expect(TEXTS.replaceTitle).toBe("Replace current draft?");
      // This computer's empty machine leaves no leading " / ".
      expect(replaceText("\nclaude/p/open")).toBe("Replace the unsent draft in claude/p/open with the selected saved draft?");
      expect(replaceText("studio\nclaude/p/open\narchive-1")).toBe("Replace the unsent draft in studio / claude/p/open / archive-1 with the selected saved draft?");
      expect(TEXTS.replaceConfirm).toBe("Replace draft");
    });
  });

  it("Restore sends the selection to the composer and shows why not when Rust refuses", async () => {
    const f = fakes();
    await createRoot(async () => {
      const s = createSavedDrafts(f.port);
      s.show(4);
      s.select(item("\nclaude/p/a", "qt"));
      f.details.answer(detail("saved"));
      await tick();
      const refused = s.restore(target());
      expect(s.busy()).toBe(true);
      f.restores.fail("The current draft cannot be replaced while it is loading, sending or waiting to send.");
      expect(await refused).toBe(false);
      expect(s.message()).toEqual({ text: "The current draft cannot be replaced while it is loading, sending or waiting to send.", error: true });
      const done = s.restore(target());
      f.restores.answer();
      expect(await done).toBe(true);
      expect(f.calls.filter((c) => c.startsWith("restore"))).toEqual(["restore 4 qt \nclaude/p/a", "restore 4 qt \nclaude/p/a"]);
      expect(await s.restore(target({ handle: 9 }))).toBe(false);
      expect(f.calls.filter((c) => c.startsWith("restore"))).toHaveLength(2);
    });
  });

  it("Recover copies only a Qt draft, then lists this computer's drafts again", async () => {
    const f = fakes();
    await createRoot(async () => {
      const s = createSavedDrafts(f.port);
      s.show(4);
      f.pages.client.answer({ items: [], next: null });
      f.pages.qt.answer({ items: [item("\nclaude/p/q", "qt")], next: null });
      f.conflicts.answer({ items: [], next: null });
      await tick();
      s.select(item("\nclaude/p/a"));
      await s.recover();
      expect(f.calls.some((c) => c.startsWith("recover"))).toBe(false);
      s.select(item("\nclaude/p/q", "qt"));
      const first = s.recover();
      f.recovers.fail("Another draft already exists here. Recover the previous one from Saved drafts.");
      await first;
      expect(s.message()?.error).toBe(true);
      const second = s.recover();
      f.recovers.answer();
      await second;
      expect(s.message()).toEqual({ text: TEXTS.recovered, error: false });
      expect(f.calls.filter((c) => c.startsWith("page client"))).toEqual([`page client ${PAGE} null`, `page client ${PAGE} null`]);
    });
  });

  it("Go to session finds the draft's row as Qt's draftOwner does", () => {
    const row = (extra: Partial<Session>) => ({ name: "claude/p/a", computer: "", rawMachine: "studio", state: "running", catalogKey: "run:1", ...extra }) as Session;
    const message = item("\nclaude/p/a");
    expect(draftOwnerRow(message, [row({})])?.catalogKey).toBe("run:1");
    expect(draftOwnerRow(message, [row({ state: "archived" })])).toBeUndefined();
    expect(draftOwnerRow(message, [row({ computer: "studio" })]), "another computer's row").toBeUndefined();
    expect(draftOwnerRow({ ...message, archived: true }, [row({})])).toBeUndefined();
    expect(draftOwnerRow({ ...message, kind: "unknown" }, [row({})])).toBeUndefined();
    // A subagent key names the machine; the alias's row wins over a machine-name match.
    const subagent = item("studio\nclaude/p/a\nconv\nagent", "client", { kind: "subagent", computer: "studio", session: "claude/p/a" });
    expect(draftOwnerRow(subagent, [row({ computer: "", catalogKey: "run:machine" })])?.catalogKey).toBe("run:machine");
    expect(draftOwnerRow(subagent, [row({ computer: "", catalogKey: "run:machine" }), row({ computer: "studio", catalogKey: "run:alias" })])?.catalogKey).toBe("run:alias");
    expect(TEXTS.unreachable).toBe("This session is not in the session list now.");
  });

  it("opens a subagent only while its conversation and child id match", () => {
    const owner = ownerRow({ conversationId: "conv", subagents: [{ id: "agent", name: "Agent", state: "working", summary: "" }] });
    const draft = item("studio\nclaude/p/a\nconv\nagent", "client", { kind: "subagent", computer: "studio", session: "claude/p/a" });
    expect(draftSubagent(draft, owner)).toBe("agent");
    expect(draftSubagent(draft, ownerRow({ conversationId: "other", subagents: [{ id: "agent", name: "Agent", state: "working", summary: "" }] }))).toBeUndefined();
    expect(draftSubagent(draft, ownerRow({ conversationId: "conv", subagents: [] }))).toBeUndefined();
    expect(draftSubagent(item("\nclaude/p/a"), owner)).toBeUndefined();
  });

  it("takes Saved drafts roles from the catalog without adding the subagent's model", () => {
    const owner = ownerRow({ title: "Custom title", machine: "studio", agent: "claude", computer: "", cwd: "/work/zerus", branch: "main", model: "model-1", effort: "high" });
    const presentation = draftPresentation(item("\nclaude/p/a"), owner);
    expect(presentation).toMatchObject({ title: "Custom title", machine: "studio", agent: "claude", computer: "", checkout: "zerus / main", model: "model-1 high" });
    const subagent = draftPresentation(item("studio\nclaude/p/a\nconv\nagent", "client", { kind: "subagent", computer: "studio", session: "claude/p/a" }), owner);
    expect(subagent.model).toBe("");
    const fallback = draftPresentation(item("\nclaude/p/a"), undefined);
    expect(fallback).toMatchObject({ title: "\nclaude/p/a", machine: "this computer", checkout: "", model: "" });
  });

  it("shows the modified time as d MMM HH:mm, and nothing when unknown", () => {
    const at = new Date(2026, 9, 7, 9, 5).getTime() / 1000;
    expect(modifiedText(at)).toBe("7 Oct 09:05");
    expect(modifiedText(0)).toBe("");
  });

  it("names why Restore and Copy text are off", async () => {
    const f = fakes();
    await createRoot(async () => {
      const s = createSavedDrafts(f.port);
      s.show(4);
      expect([s.restoreReason(target()), s.copyReason()]).toEqual([REASONS.select, REASONS.select]);
      s.select(item("\nclaude/p/a"));
      expect([s.restoreReason(target()), s.copyReason()]).toEqual([REASONS.loading, REASONS.loading]);
      f.details.answer(detail(""));
      await tick();
      expect(s.copyReason()).toBe(REASONS.noText);
      expect(s.restoreReason(undefined)).toBe(REASONS.noSession);
      expect(s.restoreReason(target({ handle: 5 }))).toBe(REASONS.otherSession);
      expect(s.restoreReason(target({ idle: false }))).toBe(REASONS.composerBusy);
      expect(s.restoreReason(target())).toBe("");
      s.select(item("voice-message-draft\n\nclaude/p/a"));
      const voice = "Open the original conversation (claude/p/a) to review or check this voice message.";
      f.details.answer(detail("words", { restorable: false, detail: voice }));
      await tick();
      expect(s.restoreReason(target())).toBe(voice);
      expect(s.copyReason()).toBe("");
      s.show(undefined);
      s.select(item("\nclaude/p/a"));
      f.details.answer(detail("saved"));
      await tick();
      expect(s.restoreReason(target())).toBe(REASONS.noSession);
    });
  });

  it("Recover is busy until its answer, and Restore waits for it", async () => {
    const f = fakes();
    await createRoot(async () => {
      const s = createSavedDrafts(f.port);
      s.show(4);
      s.select(item("\nclaude/p/q", "qt"));
      f.details.answer(detail("saved"));
      await tick();
      const run = s.recover();
      expect([s.recovering(), s.recoverReason(), s.restoreReason(target())]).toEqual([true, REASONS.working, REASONS.working]);
      expect(await s.restore(target())).toBe(false);
      f.recovers.answer();
      await run;
      expect([s.recovering(), s.recoverReason()]).toEqual([false, ""]);
    });
  });

  it("the list's keys move the selection by one, by a page and to the ends", () => {
    expect([listStep(5, 1, "ArrowDown", 3), listStep(5, 4, "ArrowDown", 3), listStep(5, -1, "ArrowDown", 3)]).toEqual([2, 4, 0]);
    expect([listStep(5, 1, "ArrowUp", 3), listStep(5, 0, "ArrowUp", 3), listStep(5, -1, "ArrowUp", 3)]).toEqual([0, 0, 0]);
    expect([listStep(5, 2, "Home", 3), listStep(5, 2, "End", 3)]).toEqual([0, 4]);
    expect([listStep(9, 1, "PageDown", 3), listStep(9, 7, "PageDown", 3), listStep(9, -1, "PageDown", 3)]).toEqual([4, 8, 3]);
    expect([listStep(9, 5, "PageUp", 3), listStep(9, 1, "PageUp", 3), listStep(9, 4, "PageUp", 0)]).toEqual([2, 0, 3]);
    expect([listStep(0, -1, "ArrowDown", 3), listStep(5, 1, "Enter", 3), listStep(5, 1, "a", 3)]).toEqual([undefined, undefined, undefined]);
  });
});
