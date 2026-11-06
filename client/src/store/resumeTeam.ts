import { tr } from "../i18n";
import type { TeamChoice } from "../types";

// Task 112 (owner, 2026-11-06: "I paused it in the morning, unpaused it in
// the afternoon, but Zerus still thinks it is paused"): a team's pause is the
// team's, not its sessions'. Resuming one of a paused team's sessions asks
// once whether to resume the team too (Resume team focused); "Only this
// session" leaves the team paused, Cancel resumes nothing.

export interface ResumeTeamRow { computer: string; name: string; state?: string }
export interface PausedTeam { id: string; name: string; status: string }

export interface ResumeTeamPort {
  teamOf(row: ResumeTeamRow): PausedTeam | undefined;
  ask(choice: TeamChoice): Promise<string | undefined>;
  resume(computer: string, team: PausedTeam): Promise<unknown>;
}

/** The question, as the dialog shows it. */
export function resumeTeamChoice(team: Pick<PausedTeam, "name">): TeamChoice {
  return {
    name: "resumeTeamToo",
    title: tr("Resume the team too?"),
    text: tr("%1 is paused: its members get no tasks while it stays paused. Resume the team too?", team.name),
    choices: [{ id: "team", text: tr("Resume team") }, { id: "session", text: tr("Only this session") }],
    initial: "team",
  };
}

/** Asks before a paused team's session resumes; false when the owner cancelled. */
export async function resumeTeamToo(row: ResumeTeamRow, port?: ResumeTeamPort): Promise<boolean> {
  let use: ResumeTeamPort;
  try {
    use = port ?? (await defaultPort());
  } catch {
    return true;   // no team store here (a test's bridge): nothing to ask
  }
  const team = use.teamOf(row);
  if (!team || team.status !== "paused") return true;
  const answer = await use.ask(resumeTeamChoice(team));
  if (!answer) return false;
  if (answer === "team") await use.resume(row.computer, team).catch(() => undefined);
  return true;
}

async function defaultPort(): Promise<ResumeTeamPort> {
  const [{ teams }, { teamConfirm }, bridge] = await Promise.all([import("./teams"), import("./teamConfirm"), import("../bridge")]);
  return {
    teamOf: (row) => teams.teamOf(row),
    ask: (choice) => teamConfirm.askChoice(choice),
    resume: (computer, team) => bridge.teamAction({ computer, team: team.id, team_name: team.name, action: { kind: "resume" } }),
  };
}
