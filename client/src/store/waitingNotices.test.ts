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

  it("holds and batches the top-level fields hgs writes (task 258)", () => {
    const out = port();
    const notices = new WaitingNotices(out);
    // A card the server held while the owner was away is not announced.
    expect(notices.observe([{ ...item("a"), notify: false }], true)).toEqual([]);
    expect(notices.known()).toEqual([]);
    // Cards that come back together are one notice.
    const first = { ...item("b"), notify: true, batch: "b1" };
    const second = { ...item("c"), notify: true, batch: "b1" };
    expect(notices.observe([first, second], true).map((notice) => notice.token)).toEqual(["team-waiting:batch:b1"]);
    expect(out.posted[0].body).toBe("b\nc");
  });

  it("does not raise an item a restart already showed and saves every change (W2)", () => {
    const out = port();
    const saves: Record<string, string>[] = [];
    const notices = new WaitingNotices(out, [["a", "team-waiting:a"]], (posted) => saves.push(Object.fromEntries(posted)));
    // "a" was shown before the restart; only the fresh "b" is announced.
    expect(notices.observe([item("a"), item("b")], false).map((notice) => notice.token)).toEqual(["team-waiting:b"]);
    expect(saves).toEqual([{ a: "team-waiting:a", b: "team-waiting:b" }]);
    // A complete read that drops "a" rewrites the saved set without it.
    notices.observe([item("b")], true);
    expect(saves.at(-1)).toEqual({ b: "team-waiting:b" });
    notices.withdraw("b");
    expect(saves.at(-1)).toEqual({});
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

  it("announces a new proposal card once, and withdraws it when it is decided (task 217)", () => {
    const out = port();
    const notices = new WaitingNotices(out);
    const proposal: TeamWaitingItem = {
      id: "team-1/owner_word_asked/proposal-p1", kind: "owner_word_asked", team: "team-1", team_name: "Live",
      title: "The lead proposes: max_members 6 → 8", action: "decide_proposal", since: 1, proposal: "p1", detail: { proposal: "p1" },
    };
    // The desktop notification the owner sees when the lead's Activity draws a new card.
    expect(notices.observe([proposal], true)).toEqual([{
      token: "team-waiting:team-1/owner_word_asked/proposal-p1", title: "Waiting for you in Live", body: proposal.title,
    }]);
    expect(out.posted.map((posted) => posted.body)).toEqual([proposal.title]);
    // One notice per card, never again for it.
    expect(notices.observe([proposal], true)).toEqual([]);
    // Decided: the complete read no longer lists it, so its notice goes.
    notices.observe([], true);
    expect(out.withdrawn).toEqual(["team-waiting:team-1/owner_word_asked/proposal-p1"]);
  });
});
