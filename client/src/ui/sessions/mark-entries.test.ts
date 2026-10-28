import { beforeEach, describe, expect, it, vi } from "vitest";

const m = vi.hoisted(() => {
  // store/ui.ts reads the page address and storage when it loads.
  const store = new Map<string, string>();
  vi.stubGlobal("location", { search: "", hash: "", href: "http://localhost/" });
  vi.stubGlobal("localStorage", { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => { store.set(k, v); }, removeItem: (k: string) => { store.delete(k); } });
  return {
  markRead: vi.fn(() => true), markNeedsAttention: vi.fn(() => true), markAllRead: vi.fn(() => 0), unreadReplies: vi.fn(() => new Map<string, string>()),
  notify: vi.fn(), stillListed: vi.fn((row: unknown) => row),
  };
});
vi.mock("../../store/marks", async (orig) => ({ ...(await orig<object>()), markRead: m.markRead, markNeedsAttention: m.markNeedsAttention, markAllRead: m.markAllRead, unreadReplies: m.unreadReplies }));
vi.mock("../../store/actions", async (orig) => ({ ...(await orig<object>()), notify: m.notify, stillListed: m.stillListed }));

import { markAllEntry, markAllLabel, markAllTip, markEntry, markReadChip } from "./mark-entries";
import type { Session } from "./types";

const session = (extra: Record<string, unknown> = {}, reviewLater = false) => ({
  key: "run:a", computer: "", name: "claude/proj/a", state: "running", status: "ready", unread: false, reviewLater, ...extra,
}) as unknown as Session;
const unread = { unread: true };

beforeEach(() => { vi.clearAllMocks(); m.markRead.mockReturnValue(true); m.markNeedsAttention.mockReturnValue(true); m.stillListed.mockImplementation((row: unknown) => row); });

describe("markReadChip", () => {
  it("is active for a live row that needs attention", () => {
    expect(markReadChip(session(unread)).active).toBe(true);
    expect(markReadChip(session({}, true)).active).toBe(true);
    expect(markReadChip(session()).active).toBe(false);
    expect(markReadChip(session({ ...unread, state: "archived" })).active).toBe(false);
  });
  it("describes a review mark and a notification", () => {
    expect(markReadChip(session({}, true)).detail).toBe("Marked for later. Mark as read to clear this reminder.");
    expect(markReadChip(session(unread)).detail).toBe("Mark this session's current notification as read. This does not answer a pending question.");
  });
});

describe("markEntry", () => {
  it("offers Mark as read for a row that needs attention, always enabled", () => {
    const e = markEntry(session({}, true))!;
    expect(e).toMatchObject({ label: "Mark as read" });
    expect(e.disabled).toBeUndefined();
    e.action!();
    expect(m.markRead).toHaveBeenCalledTimes(1);
    expect(m.notify).not.toHaveBeenCalled();
  });
  it("offers Mark needs attention otherwise, with Qt's hint", () => {
    const e = markEntry(session())!;
    expect(e).toMatchObject({ label: "Mark needs attention", hint: "Keep a reminder to review this session later" });
    expect(e.disabled).toBeUndefined();
    e.action!();
    expect(m.markNeedsAttention).toHaveBeenCalledTimes(1);
    expect(m.notify).not.toHaveBeenCalled();
  });
  it("shows neither for an archived row", () => expect(markEntry(session({ state: "archived" }))).toBeUndefined());
  it("says why a refused mark did not run", () => {
    m.markRead.mockReturnValue(false);
    markEntry(session({}, true))!.action!();
    expect(m.notify).toHaveBeenLastCalledWith("error", "A new reply or question arrived. Open the menu again to mark it as read.", true);
    m.markNeedsAttention.mockReturnValue(false);
    markEntry(session())!.action!();
    expect(m.notify).toHaveBeenLastCalledWith("error", "This session is no longer available. Open its actions again.", true);
  });
});

describe("the click-time recheck", () => {
  it("runs first: a vanished or changed row calls no mark and the notice comes from the recheck", () => {
    m.stillListed.mockReturnValue(undefined);
    markEntry(session({}, true))!.action!();
    markEntry(session())!.action!();
    expect(m.stillListed).toHaveBeenCalledTimes(2);
    expect(m.markRead).not.toHaveBeenCalled();
    expect(m.markNeedsAttention).not.toHaveBeenCalled();
    expect(m.notify).not.toHaveBeenCalled();
  });
});

describe("markAllEntry", () => {
  it("counts the unread replies and is off at zero", () => {
    expect(markAllLabel(0)).toBe("Mark all as read");
    expect(markAllEntry()).toMatchObject({ label: "Mark all as read", disabled: true });
    m.unreadReplies.mockReturnValue(new Map([["a", "1"], ["b", "2"]]));
    const e = markAllEntry();
    expect(e).toMatchObject({ label: "Mark all as read (2)", disabled: false });
    expect(markAllTip(e)).toBe(`Mark all as read (2)\n${e.hint}`);
    expect(e.hint).toMatch(/^Mark current replies across all machines as read, including hidden sessions\./);
  });
  it("marks them and counts them in a notice", () => {
    m.unreadReplies.mockReturnValue(new Map([["a", "1"]]));
    m.markAllRead.mockReturnValue(1);
    markAllEntry().action!();
    expect(m.notify).toHaveBeenLastCalledWith("info", "1 reply marked as read.");
    m.markAllRead.mockReturnValue(3);
    markAllEntry().action!();
    expect(m.notify).toHaveBeenLastCalledWith("info", "3 replies marked as read.");
    m.notify.mockClear();
    m.markAllRead.mockReturnValue(0);
    markAllEntry().action!();
    expect(m.notify).not.toHaveBeenCalled();
  });
});
