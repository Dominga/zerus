import { describe, expect, it } from "vitest";
import { createRoot } from "solid-js";
import { NOTHING, createTeams, mergeFollows, sessionIs, type TeamsFollow } from "./teamsView";
import type { TeamList, TeamView } from "../types";

// Task 9: the Teams views as the batches bring them, the team a session
// belongs to, and what is followed (once per change).

const list = (status = "running"): TeamList => ({
  truncated: false,
  teams: [{
    id: "t1", name: "Checkout", status, project: "", folder: "/work/p", plan_mode: "standard", lead: "lead", tasks: 4, accepted: 1,
    submitted: 0, in_review: 0, in_test: 0, attention: 2, open_proposals: 0, waiting_for_lead: false, pending_phase: null, updated: 1, revision: 3,
    members: [
      { id: "lead", role: "Lead", agent: "claude", state: "active", lead: true, session: { host: "", name: "claude/p/lead", run_id: "r", conversation_id: "c" } },
      { id: "dev", role: "Backend", agent: "codex", state: "active", lead: false, session: { host: "studio", name: "codex/p/dev", run_id: "r", conversation_id: "c" } },
    ],
  }],
});

describe("Teams views", () => {
  it("keep the latest view per key; a failure keeps what was shown, an ended view is gone", () => {
    createRoot((dispose) => {
      const store = createTeams({ follow: async () => {} });
      store.apply([{ computer: "", key: "list", kind: "list", payload: list() }, { computer: "", key: "team:t1", kind: "team", payload: { id: "t1" } }]);
      expect(store.list("")?.teams[0].id).toBe("t1");
      expect(store.team("", "t1")?.id).toBe("t1");
      expect(store.team("studio", "t1")).toBeUndefined();
      store.apply([{ computer: "", key: "team:t1", kind: "failed", payload: { code: "timeout", message: "hgs did not answer" } }]);
      expect(store.team("", "t1")?.id).toBe("t1");
      expect(store.failure("", "team:t1")).toBe("hgs did not answer");
      store.apply([{ computer: "", key: "team:t1", kind: "team", payload: { id: "t1", name: "Renamed" } as TeamView }]);
      expect([store.team("", "t1")?.name, store.failure("", "team:t1")]).toEqual(["Renamed", undefined]);
      store.apply([{ computer: "", key: "team:t1", kind: "ended", payload: null }]);
      expect(store.team("", "t1")).toBeUndefined();
      dispose();
    });
  });

  it("find a session's team by its member sessions, here and on the computer they name", () => {
    createRoot((dispose) => {
      const store = createTeams({ follow: async () => {} });
      store.apply([{ computer: "", key: "list", kind: "list", payload: list() }]);
      expect(store.teamOf({ computer: "", name: "claude/p/lead" })?.id).toBe("t1");
      expect(sessionIs({ host: "studio", name: "codex/p/dev", run_id: "", conversation_id: "" }, "", { computer: "studio", name: "codex/p/dev" })).toBe(true);
      expect(sessionIs({ host: "", name: "claude/p/lead", run_id: "", conversation_id: "" }, "", { computer: "studio", name: "claude/p/lead" })).toBe(false);
      expect(store.teamOf({ computer: "", name: "claude/p/other" })).toBeUndefined();
      store.apply([{ computer: "", key: "list", kind: "list", payload: list("archived") }]);
      expect(store.teamOf({ computer: "", name: "claude/p/lead" })).toBeUndefined();
      dispose();
    });
  });

  it("follow a computer once per change, stop it with nothing, and try a refused follow again", async () => {
    const calls: [string, TeamsFollow][] = [];
    let refuse = false;
    const store = createTeams({ follow: async (computer, follow) => { calls.push([computer, follow]); if (refuse) throw new Error("Too many computers"); } });
    store.follow("", { list: true, waiting: false, team: null, catalog: false });
    store.follow("", { list: true, waiting: false, team: null, catalog: false });
    store.follow("", { list: true, waiting: false, team: "t1", catalog: false });
    store.follow("", { list: false, waiting: false, team: null, catalog: false });
    expect(calls.map(([, follow]) => follow.team ?? (follow.list ? "list" : "none"))).toEqual(["list", "t1", "none"]);
    expect(store.followed().size).toBe(0);
    refuse = true;
    store.follow("studio", { list: true, waiting: false, team: null, catalog: false });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(store.followError("studio")).toContain("Too many computers");
    refuse = false;
    store.follow("studio", { list: true, waiting: false, team: null, catalog: false });
    expect(calls).toHaveLength(5);
  });

  it("the Team tab and the catalog page follow one computer together, as one follow (task 47)", () => {
    // The core keeps one follow per computer: a second call replaces the
    // first. The Team tab (the selected session's list and team), Waiting for
    // you, a team opened by New team and the catalog page merge into one.
    const tab = { computer: "", list: true, team: "t1" };
    const catalog = { computer: "", catalog: true };
    expect(mergeFollows([tab, catalog]).get("")).toEqual({ list: true, waiting: false, team: "t1", catalog: true });
    expect(mergeFollows([catalog, tab]).get("")).toEqual({ list: true, waiting: false, team: "t1", catalog: true });
    // Waiting for you joins them; another computer keeps its own follow.
    const merged = mergeFollows([tab, catalog, { computer: "", waiting: true }, { computer: "studio", list: true }]);
    expect(merged.get("")).toEqual({ list: true, waiting: true, team: "t1", catalog: true });
    expect(merged.get("studio")).toEqual({ ...NOTHING, list: true });
    // A team opened explicitly (New team) is the one team read; a need without one keeps it.
    expect(mergeFollows([tab, { computer: "", team: "draft-2" }, catalog]).get("")?.team).toBe("draft-2");
    // Through the store: the catalog page opens while the Team tab shows, and
    // closes again; each change is one call carrying everything still needed.
    const calls: TeamsFollow[] = [];
    const store = createTeams({ follow: async (_, follow) => { calls.push(follow); } });
    const apply = (needs: Parameters<typeof mergeFollows>[0]) => store.follow("", mergeFollows(needs).get("") ?? NOTHING);
    apply([tab]);
    apply([tab, catalog]);
    apply([tab]);
    apply([]);
    expect(calls).toEqual([
      { list: true, waiting: false, team: "t1", catalog: false },
      { list: true, waiting: false, team: "t1", catalog: true },
      { list: true, waiting: false, team: "t1", catalog: false },
      NOTHING,
    ]);
    expect(store.followed().size).toBe(0);
  });
});
