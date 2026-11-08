import type { AccountChoice } from "../../store/newTeamSources";
import type { Session } from "../sessions/types";
import type { ComputerId, TeamMemberView, TeamSessionView, TeamView, TeamAction, TeamCall, TeamOutcome } from "../../types";
import type { DecisionPort } from "./decisions";
import { act } from "./decisions";
import { KNOWN_MODELS, effortOptions, modelOptions, type ModelChoice } from "./catalogControls";

// Task #218: the Swap member dialog's choices, as Qt's swapDialog after the
// fix: the agent's accounts, the account's model catalog (the agent's known
// models until it is read or when it has none), the model's efforts, and the
// other live sessions to adopt. A typed model stays offered. Pure, so the
// dialog and the tests share one rule.

/** The agents a member may run as (Qt's agentNames). */
export const SWAP_AGENTS = ["claude", "codex", "kimi", "dsh", "opencode"] as const;

/** One member's chosen swap: a new session with these fields, or an existing one. */
export interface SwapChoice {
  agent: string;
  model: string;
  effort: string;
  account: string;
  /** An existing session to adopt; none: a new session. */
  session?: TeamSessionView;
}

export interface SwapWhere {
  computer: ComputerId;
  team: string;
  teamName: string;
}

export interface SwapOptions {
  models: { id: string; label: string }[];
  efforts: string[];
  accounts: AccountChoice[];
}

/** The agent's accounts (the default is always offered) and the catalog's model choices. */
export function swapOptions(
  agent: string,
  model: string,
  account: string,
  catalog: readonly ModelChoice[] | undefined,
  accounts: readonly AccountChoice[],
  effort = "",
): SwapOptions {
  const choices = accounts.filter((found) => found.provider === agent);
  const current = account && !choices.some((found) => found.id === account);
  return {
    models: modelOptions({ agent, model, account }, catalog),
    efforts: effortOptions(model, catalog, effort),
    accounts: current
      ? [...choices, { id: account, label: account, provider: agent, signedIn: true }]
      : choices,
  };
}

/** The first model an agent is known by; "" for an agent without a known list. */
export const firstModel = (agent: string): string => KNOWN_MODELS[agent]?.[0] ?? "";

/** The member's own values, as the dialog opens: Qt preselects them. */
export function swapChoice(member: TeamMemberView): SwapChoice {
  return { agent: member.agent || "claude", model: member.model, effort: member.effort, account: member.account };
}

/** The other live local sessions a member may adopt; this team's own are not offered. */
export function swapSessions(team: TeamView, rows: Record<string, Session>, order: readonly string[]): TeamSessionView[] {
  const taken = new Set(team.members.map((member) => member.session?.name).filter((name): name is string => !!name));
  const out: TeamSessionView[] = [];
  const seen = new Set<string>();
  for (const id of order) {
    const row = rows[id];
    if (!row || row.computer !== "" || row.state === "archived" || !row.name || taken.has(row.name) || seen.has(row.name)) continue;
    seen.add(row.name);
    out.push({ host: "", name: row.name, run_id: row.runId ?? "", conversation_id: row.conversationId ?? "" });
  }
  return out;
}

/** The core's `swap-member` call: a new session carries its fields; an existing one names the session. */
export function swapAction(where: SwapWhere, member: TeamMemberView, choice: SwapChoice): Omit<TeamCall, "confirmed"> {
  const action: TeamAction = choice.session
    ? { kind: "swap-member", member: member.id, session: choice.session }
    : {
        kind: "swap-member",
        member: member.id,
        agent: choice.agent,
        ...(choice.model.trim() ? { model: choice.model.trim() } : {}),
        ...(choice.effort ? { effort: choice.effort } : {}),
        ...(choice.account ? { account: choice.account } : {}),
      };
  return { computer: where.computer, team: where.team, team_name: where.teamName, action };
}

/** The call with the core's confirmation in the app's dialog: Cancel sends nothing, a yes sends it confirmed. */
export function swapMemberCall(
  port: Pick<DecisionPort, "send" | "confirm">,
  where: SwapWhere,
  member: TeamMemberView,
  choice: SwapChoice,
): Promise<TeamOutcome | "cancelled"> {
  return act(port, swapAction(where, member, choice));
}
