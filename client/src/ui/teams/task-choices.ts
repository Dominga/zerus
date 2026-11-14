import type { TeamSummary } from "../../store/teams";

export interface TaskTeamChoice extends TeamSummary { label: string }

/** The queue writer accepts Running and Paused, not every listed team. */
export function taskTeamChoices(available: boolean | undefined, teams: readonly TeamSummary[], project: string | undefined): TaskTeamChoice[] {
  if (available !== true || !project) return [];
  const candidates = teams.filter((team) => team.project === project && ["running", "paused"].includes(team.status));
  const names = new Map<string, string[]>();
  for (const team of candidates) names.set(team.name, [...(names.get(team.name) ?? []), team.id]);
  return candidates.slice(0, 8).map((team) => {
    const peers = names.get(team.name)!;
    let length = 8;
    while (length < team.id.length && peers.some((id) => id !== team.id && id.slice(-length) === team.id.slice(-length))) length += 4;
    return { ...team, label: peers.length > 1 ? `${team.name} (${team.id.slice(-length)})` : team.name };
  });
}
