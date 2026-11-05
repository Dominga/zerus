import { describe, expect, it } from "vitest";
import { createRoot } from "solid-js";
import { createTeams, sessionIs, type TeamsFollow } from "./teamsView";
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
    store.follow("", { list: true, waiting: false, team: null });
    store.follow("", { list: true, waiting: false, team: null });
    store.follow("", { list: true, waiting: false, team: "t1" });
    store.follow("", { list: false, waiting: false, team: null });
    expect(calls.map(([, follow]) => follow.team ?? (follow.list ? "list" : "none"))).toEqual(["list", "t1", "none"]);
    expect(store.followed().size).toBe(0);
    refuse = true;
    store.follow("studio", { list: true, waiting: false, team: null });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(store.followError("studio")).toContain("Too many computers");
    refuse = false;
    store.follow("studio", { list: true, waiting: false, team: null });
    expect(calls).toHaveLength(5);
  });
});
