import { describe, expect, it } from "vitest";
import { resumeTeamChoice, resumeTeamToo, type PausedTeam, type ResumeTeamPort } from "./resumeTeam";
import { choiceButtons } from "./teamConfirm";
import { WaitingNotices, type WaitingNotice } from "./waitingNotices";
import { barActions, barChips, pausedText, type ActTeam } from "../ui/teams/actions";
import { decidedWaiting, resumesTeam } from "../ui/teams/decisions";
import type { TeamWaitingItem } from "../types";

// Task 112 (owner, 2026-11-06: "I paused it in the morning, unpaused it in
// the afternoon, but Zerus still thinks it is paused"): one meaning of Pause.

function port(team: PausedTeam | undefined, answer: string | undefined) {
  const asked: string[] = [];
  const resumed: string[] = [];
  const fake: ResumeTeamPort = {
    teamOf: () => team,
    async ask(choice) { asked.push(`${choice.name}:${choice.initial}`); return answer; },
    async resume(computer, found) { resumed.push(`${computer}/${found.id}`); },
  };
  return { fake, asked, resumed };
}

describe("resuming a paused team's session", () => {
  const paused = { id: "t1", name: "Adventure", status: "paused" };
  const row = { computer: "desktop", name: "kimi/SonicTerror/adventure-lead" };

  it("asks once, Resume team focused, and resumes the team on that answer", async () => {
    const both = port(paused, "team");
    expect(await resumeTeamToo(row, both.fake)).toBe(true);
    expect(both.asked).toEqual(["resumeTeamToo:team"]);
    expect(both.resumed).toEqual(["desktop/t1"]);
    const only = port(paused, "session");
    expect(await resumeTeamToo(row, only.fake)).toBe(true);
    expect(only.resumed).toEqual([]);
    const cancel = port(paused, undefined);
    expect(await resumeTeamToo(row, cancel.fake)).toBe(false);
    const running = port({ ...paused, status: "running" }, "team");
    expect(await resumeTeamToo(row, running.fake)).toBe(true);
    expect(running.asked).toEqual([]);
    expect(await resumeTeamToo(row, port(undefined, "team").fake)).toBe(true);
  });

  it("focuses Resume team in the dialog, Cancel last", () => {
    const buttons = choiceButtons(resumeTeamChoice(paused));
    expect(buttons.map((button) => `${button.id}:${button.initial}`)).toEqual(["team:true", "session:false", "cancel:false"]);
    // A choice without its own focus keeps Cancel focused.
    expect(choiceButtons({ name: "x", title: "x", text: "x", choices: [{ id: "a", text: "A" }] }).map((button) => button.initial)).toEqual([false, true]);
  });
});

describe("a paused team says so", () => {
  const team = (over: Partial<ActTeam>): ActTeam => ({
    id: "t1", name: "Adventure", status: "paused", plan_mode: "standard", revision: 1, pending_phase: null, waiting_for_lead: false,
    settings: {}, members: [], tasks: [], phases: [], ...over,
  } as unknown as ActTeam);

  it("on the team bar: since when, by whom, and Resume team", () => {
    const at = new Date(2026, 10, 6, 9, 12);
    const today = new Date(2026, 10, 6, 15, 0);
    expect(pausedText(team({ paused: { at: at.getTime() / 1000, by: "You" } }), today)).toBe("Team paused since 09:12 by You: members get no tasks");
    expect(pausedText(team({ paused: { at: at.getTime() / 1000, by: "You" } }), new Date(2026, 10, 7, 8, 0)))
      .toBe("Team paused since 2026-11-06 09:12 by You: members get no tasks");
    expect(pausedText(team({ paused: null }))).toBe("Team paused: members get no tasks");
    expect(pausedText(team({ status: "running" }))).toBe("");
    expect(barChips(team({ paused: { at: at.getTime() / 1000, by: "Lead" } }))[0].id).toBe("teamPaused");
    expect(barActions(team({}), "standard").find((action) => action.id === "resume")?.text).toBe("Resume team");
  });

  it("in Waiting for you: Resume team, never notified, gone once resumed", () => {
    const item = { id: "t1/team_paused/1", kind: "team_paused", team: "t1", team_name: "Adventure", title: "Team paused since 09:12 by You: members get no tasks",
      action: "resume_team", since: 1, proposal: null } as unknown as TeamWaitingItem;
    expect(resumesTeam(item)).toBe(true);
    const posted: WaitingNotice[] = [];
    new WaitingNotices({ post: (notice) => posted.push(notice), withdraw: () => {} }).observe([item], true);
    expect(posted).toEqual([]);
    expect(decidedWaiting([item], { team: "t1", action: { kind: "resume" } })).toEqual(["t1/team_paused/1"]);
    expect(decidedWaiting([item], { team: "t2", action: { kind: "resume" } })).toEqual([]);
  });
});
