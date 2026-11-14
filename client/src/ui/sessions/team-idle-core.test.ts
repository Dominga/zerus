// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vitest";
import core from "./fixtures/team-session-presentation.json";
import teams from "./fixtures/agent-team-views.json";
import type { TeamList } from "../../store/teams";
import type { ViewRow } from "../../types";
import { toSession } from "../../store/catalog";
import { agentTeamIndex, agentTeamSession } from "./agent-teams";
vi.hoisted(() => vi.stubGlobal("location", new URL("http://localhost/")));
const machine = "00000000-0000-4000-8000-0000000000aa";
const row = (kind: keyof typeof core) => toSession("", "run:startup", core[kind] as unknown as ViewRow);
const member = () => agentTeamIndex(true, (teams as unknown as { list: TeamList }).list.teams, undefined, [row("unknown")], () => machine).get(row("unknown").key)!;

describe("real core startup presentation", () => {
  it("names a live unknown-activity member with the real -1 job-count sentinel", () => {
    const s = row("unknown");
    expect(s.status).toBe("ready");
    expect(s.caption).toBe("Status unknown");
    expect(s.processState).toBe("running");
    expect(s.backgroundJobs).toBe(-1);
    expect(s.conversationId).toBeUndefined();
    expect(agentTeamSession(s, member(), true).caption).toBe("Waiting for a task");
  });
  it("names a ready member too", () => {
    const s = row("ready");
    expect(s.caption).toBe("Ready");
    expect(s.backgroundJobs).toBe(0);
    expect(agentTeamSession(s, member(), true).caption).toBe("Waiting for a task");
  });
  it("keeps a positive count's normal core caption", () => {
    const s = row("background");
    expect(s.backgroundJobs).toBe(1);
    expect(agentTeamSession(s, member(), true)).toBe(s);
  });
  it("leaves ordinary and offline sessions unchanged", () => {
    const s = row("unknown");
    expect(agentTeamSession(s, undefined, true)).toBe(s);
    expect(agentTeamSession(s, member(), false)).toBe(s);
  });
});
