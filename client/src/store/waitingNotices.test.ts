import { describe, expect, it } from "vitest";
import type { TeamWaitingItem } from "../types";
import { WaitingNotices, type WaitingNotice, type WaitingNotificationPort } from "./waitingNotices";

function item(id: string, title = id): TeamWaitingItem {
  return { id, kind: "report", team: "team-1", team_name: "Orbit", title, action: "decide_proposal", since: 1, proposal: null } as TeamWaitingItem;
}

function port(): WaitingNotificationPort & { posted: WaitingNotice[]; withdrawn: string[] } {
  const posted: WaitingNotice[] = [];
  const withdrawn: string[] = [];
  return {
    posted,
    withdrawn,
    post(notice) { posted.push(notice); },
    withdraw(token) { withdrawn.push(token); },
  };
}

function card(id: string, detail: Record<string, unknown>): TeamWaitingItem {
  return { ...item(id, `card ${id}`), kind: "owner_word_asked", detail: { id, ...detail } } as TeamWaitingItem;
}

describe("waiting notices", () => {
  it("holds the owner's cards while away and posts them as one batch when back (task 159)", () => {
    const out = port();
    const notices = new WaitingNotices(out);
    expect(notices.observe([card("p1", { notify: false }), card("p2", { notify: false })], true)).toEqual([]);
    expect(out.posted).toHaveLength(0);
    const back = notices.observe([card("p1", { notify: true, batch: "cards-1" }), card("p2", { notify: true, batch: "cards-1" }), item("r")], true);
    expect(back.map((notice) => notice.token)).toEqual(["team-waiting:batch:cards-1", "team-waiting:r"]);
    expect(out.posted[0].body).toBe("card p1\ncard p2");
    // The batch's notice stays while one card still waits, and goes with the last.
    notices.observe([card("p2", { notify: true, batch: "cards-1" }), item("r")], true);
    expect(out.withdrawn).toEqual([]);
    notices.withdraw("p2");
    expect(out.withdrawn).toEqual(["team-waiting:batch:cards-1"]);
    // A lone card of its own batch is an ordinary notice.
    const single = notices.observe([card("p3", { notify: true, batch: "cards-2" })], false);
    expect(single.map((notice) => notice.token)).toEqual(["team-waiting:p3"]);
  });

  it("posts once per new item and keeps the page order", () => {
    const out = port();
    const notices = new WaitingNotices(out);
    const fresh = notices.observe([item("a"), item("b")], false);
    expect(fresh.map((notice) => notice.token)).toEqual(["team-waiting:a", "team-waiting:b"]);
    expect(out.posted.map((notice) => notice.body)).toEqual(["a", "b"]);
    expect(notices.observe([item("b")], false)).toEqual([]);
    expect(out.posted).toHaveLength(2);
  });

  it("withdraws a notice when its item is answered", () => {
    const out = port();
    const notices = new WaitingNotices(out);
    notices.observe([item("a")], false);
    notices.withdraw("a");
    expect(out.withdrawn).toEqual(["team-waiting:a"]);
    expect(notices.known()).toEqual([]);
  });

  it("withdraws everything gone from a complete read", () => {
    const out = port();
    const notices = new WaitingNotices(out);
    notices.observe([item("a"), item("b")], false);
    notices.observe([item("b")], true);
    expect(out.withdrawn).toEqual(["team-waiting:a"]);
    notices.observe([], true);
    expect(out.withdrawn).toEqual(["team-waiting:a", "team-waiting:b"]);
  });
});
