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

describe("waiting notices", () => {
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
