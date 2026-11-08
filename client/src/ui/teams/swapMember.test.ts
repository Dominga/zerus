import { describe, expect, it } from "vitest";
import type { TeamCall, TeamConfirmation, TeamMemberView, TeamOutcome, TeamSessionView, TeamView } from "../../types";
import type { DecisionPort } from "./decisions";
import { firstModel, swapAction, swapMemberCall, swapOptions, swapSessions } from "./swapMember";

const member = (over: Partial<TeamMemberView> = {}): TeamMemberView => ({
  id: "backend", role: "Backend", agent: "codex", model: "gpt-5.5", effort: "high", account: "work", computer: "", folder: "worktree",
  state: "active", activity: "idle", phase: "idle", error: "", lead: false, session: { host: "", name: "codex/p/backend", run_id: "r1", conversation_id: "c1" },
  live: true, stuck: false, question: false, queue: [], minutes_without_progress: 0, host_shared: false, ...over,
});

const team = (over: Partial<TeamView> = {}): TeamView => ({
  id: "t", name: "Orbit", status: "running", project: "", folder: "", goal: "", plan_mode: "", lead: "lead", revision: 1, created: 1,
  updated: 1, waiting_for_lead: false, pending_phase: null, settings: {}, members: [member()], tasks: [], phases: [], proposals: [],
  attention: [], validation: { errors: [], warnings: [] }, health: { critical_path: 0, parallelism: 0, warnings: [] }, ready: [], cycle: [],
  draft: null, truncated: false, ...over,
});

const where = { computer: "", team: "t", teamName: "Orbit" };

describe("the Swap member dialog's choices", () => {
  it("offers the agent's accounts, the account's models and the model's efforts", () => {
    const catalog = [{ id: "gpt-5.5", label: "", effort_options: ["low", "high"] }];
    const accounts = [
      { id: "work", label: "Work", provider: "codex", signedIn: true },
      { id: "home", label: "Home", provider: "claude", signedIn: true },
    ];
    const options = swapOptions("codex", "gpt-5.5", "work", catalog, accounts, "high");
    expect(options.models).toEqual([{ id: "gpt-5.5", label: "gpt-5.5" }]);
    expect(options.efforts).toEqual(["", "low", "high"]);
    expect(options.accounts.map((found) => found.id)).toEqual(["work"]);
    // A typed custom model stays offered; without a catalog the known models are.
    expect(swapOptions("codex", "team-custom", "", undefined, []).models.map((found) => found.id)).toEqual(["gpt-5.5-codex", "gpt-5.5", "team-custom"]);
    expect(firstModel("codex")).toBe("gpt-5.5-codex");
  });

  it("names the chosen fields for a new session and the picked session for an existing one", () => {
    expect(swapAction(where, member(), { agent: "codex", model: "gpt-5.5", effort: "high", account: "work" })).toMatchObject({
      computer: "", team: "t", team_name: "Orbit",
      action: { kind: "swap-member", member: "backend", agent: "codex", model: "gpt-5.5", effort: "high", account: "work" },
    });
    const session: TeamSessionView = { host: "", name: "claude/p/spare", run_id: "r9", conversation_id: "c9" };
    expect(swapAction(where, member(), { agent: "", model: "", effort: "", account: "", session })).toMatchObject({
      action: { kind: "swap-member", member: "backend", session },
    });
  });

  it("asks in the app dialog: Cancel sends nothing and a yes sends it confirmed", async () => {
    const confirmation: TeamConfirmation = { name: "teamSwapConfirm", title: "Swap member?", text: "Swap Backend to codex gpt-5.5?", action: "Swap" };
    function fake(yes: boolean) {
      const sent: TeamCall[] = [], asked: TeamConfirmation[] = [];
      const port: Pick<DecisionPort, "send" | "confirm"> = {
        async send(call: TeamCall): Promise<TeamOutcome> {
          sent.push(call);
          if (!call.confirmed) return { outcome: "confirm", confirmation };
          return { outcome: "done", receipt: { status: "swapped" }, request_id: "r1" };
        },
        async confirm(askedFor: TeamConfirmation) { asked.push(askedFor); return yes; },
      };
      return { sent, asked, port };
    }
    const choice = { agent: "codex", model: "gpt-5.5", effort: "high", account: "work" };
    const no = fake(false);
    expect(await swapMemberCall(no.port, where, member(), choice)).toBe("cancelled");
    expect(no.asked).toEqual([confirmation]);
    expect(no.sent).toHaveLength(1);
    expect(no.sent[0]).toMatchObject({ confirmed: false, action: { kind: "swap-member", member: "backend" } });
    const yes = fake(true);
    expect(await swapMemberCall(yes.port, where, member(), choice)).toMatchObject({ outcome: "done" });
    expect(yes.sent).toHaveLength(2);
    expect(yes.sent[1]).toMatchObject({ confirmed: true, action: { kind: "swap-member", member: "backend", model: "gpt-5.5" } });
  });

  it("offers other live local sessions and never this team's own", () => {
    const rows = {
      "run:self": { key: "run:self", catalogKey: "run:self", computer: "", name: "codex/p/backend", state: "ready" },
      "run:spare": { key: "run:spare", catalogKey: "run:spare", computer: "", name: "claude/p/spare", state: "ready", runId: "r9", conversationId: "c9" },
      "run:old": { key: "run:old", catalogKey: "run:old", computer: "", name: "claude/p/old", state: "archived" },
      "run:peer": { key: "run:peer", catalogKey: "run:peer", computer: "mac", name: "claude/p/peer", state: "ready" },
    } as unknown as Parameters<typeof swapSessions>[1];
    expect(swapSessions(team(), rows, Object.keys(rows))).toEqual([{ host: "", name: "claude/p/spare", run_id: "r9", conversation_id: "c9" }]);
  });
});
