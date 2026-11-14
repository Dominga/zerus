import type { TeamSummary, TeamView } from "../../store/teams";
import type { Session } from "./types";

export interface AgentTeamMember {
  team: string;
  name: string;
  project: string;
  member: string;
  role: string;
  lead: boolean;
}

/** Only the followed roster's durable bindings establish membership. */
export function agentTeamIndex(available: boolean | undefined, summaries: readonly TeamSummary[], view: TeamView | undefined,
                              sessions: readonly Session[], machineOf: (s: Session) => string | undefined): Map<string, AgentTeamMember> {
  const result = new Map<string, AgentTeamMember>();
  if (available !== true) return result;
  const bindings = new Map<string, AgentTeamMember | null>();
  for (const team of summaries) {
    if (!["planning", "running", "paused"].includes(team.status)) continue;
    const members = [...(team.member_sessions ?? [])];
    if (team.lead_session && !members.some((m) => m.member === team.lead)) {
      members.push({ member: team.lead, role: "lead", ...team.lead_session });
    }
    for (const m of members) {
      if (!m.machine || !m.session_id) continue;
      const detail = view?.bar.id === team.id ? view.members.find((v) => v.id === m.member
        && v.state !== "removed" && v.session?.machine === m.machine && v.session.session_id === m.session_id) : undefined;
      const key = `${m.machine}\n${m.session_id}`;
      const member = { team: team.id, name: team.name, project: team.project, member: m.member,
        role: detail?.role ?? m.role ?? "", lead: m.member === team.lead };
      // A conflicting binding proves no membership, even across scanned pages.
      bindings.set(key, bindings.has(key) ? null : member);
    }
  }
  for (const s of sessions) {
    const machine = machineOf(s);
    if (!machine) continue;
    const launch = `${machine}\nlaunch_id:${s.launchId}`;
    const member = s.launchId && bindings.has(launch) ? bindings.get(launch)
      : s.runId ? bindings.get(`${machine}\nrun_id:${s.runId}`) : undefined;
    if (member) result.set(s.key, member);
  }
  return result;
}

/** Team sessions use the lead's project position; the catalog assignments stay intact. */
export function agentTeamProjects(groups: Map<string, Session[]>, index: ReadonlyMap<string, AgentTeamMember>,
                                  visible: (s: Session) => boolean = () => true, projects: readonly string[] = []): Map<string, Session[]> {
  if (!index.size) return groups;
  const projectOf = new Map<string, string>();
  for (const [project, sessions] of groups) for (const s of sessions) {
    const member = index.get(s.key);
    if (member && visible(s) && (!projects.length || projects.includes(project)) && (member.lead || !projectOf.has(member.team))) projectOf.set(member.team, project);
  }
  const result = new Map<string, Session[]>();
  for (const [project, sessions] of groups) for (const s of sessions) {
    const member = index.get(s.key);
    const destination = member ? projectOf.get(member.team) ?? project : project;
    const list = result.get(destination) ?? [];
    list.push(s);
    result.set(destination, list);
  }
  return result;
}

/** A live, quiet member without its first conversation is waiting for work. */
export function agentTeamSession(s: Session, member: AgentTeamMember | undefined, online: boolean): Session {
  if (!member || member.lead || !online || s.state !== "running" || s.processState !== "running" || s.conversationId
    || s.status !== "ready" || s.attention || s.backgroundJobs || s.draft) return s;
  return { ...s, caption: "Waiting for a task" };
}
