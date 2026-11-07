import { describe, expect, it } from "vitest";
import { cardKey, cardOpen, compactCount, contextUsage, homeShort, sessionAccount, sessionGoal, sideTeam } from "./inspector-data";
import { usageOf } from "../../store/testing";
import { emptyAccounts, type Account, type Profile } from "../../types";

const member = (extra: Partial<Profile> = {}): Profile => ({
  key: "\u001fcodex-f970ef234e", host: "", machine: "workstation", id: "codex-f970ef234e", label: "codex-f970ef234e", provider: "codex", native: false,
  installed: true, is_default: false, home: "/home/user/.config/hgs/accounts/codex-f970ef234e", auth_revision: "", online: true, state: "ready", detail: { source: "" },
  signed_in: true, permission: "auto", permission_label: { source: "Auto" }, permission_hint: { source: "" }, model_supported: true, model_text: { source: "" },
  usage: usageOf(), opencode: null, ...extra,
});
const account = (members: Profile[], key: string, label: string, plan = ""): Account => ({
  key, label, provider: members[0].provider, installed: true, machines: members.map((m) => m.machine), members,
  usage: usageOf({ identity: { email: label, name: "", plan, organization: "", account_id: "", auth_method: "" } }), permission: { source: "Auto" }, identity_lines: [], identity_note: null,
});
const row = (extra: { computer?: string; cmd?: string; accountId?: string; accountHome?: string } = {}) =>
  ({ computer: "", cmd: "codex", accountId: "codex-f970ef234e", accountHome: "", ...extra });

describe("Inspector account", () => {
  const profile = member(), native = member({ key: "\u001fnative-codex", id: "native-codex", label: "Codex", native: true, home: "/home/user/.codex" });
  const peer = member({ key: "studio\u001fcodex-f970ef234e", host: "studio", machine: "studio" });
  const work = account([profile, native, peer], "codex\u001fwork@example.com", "work@example.com", "Pro");
  const view = { ...emptyAccounts(), accounts: [work, account([member({ key: "\u001fclaude-a1", id: "claude-a1", provider: "claude", home: "/home/user/.claude-a1" })], "claude\u001fa1", "Personal")] };

  it("names the account, not the profile id, with the profile and its duplicate tag", () => {
    const found = sessionAccount(row(), view);
    expect(found.account?.label).toBe("work@example.com");
    expect(found.profile).toBe(profile);
    expect(found.tag).toBe("f970ef");
    expect(sessionAccount(row({ accountId: "native-codex" }), view)).toMatchObject({ profile: native, tag: "native" });
  });

  it("matches a peer's profile on that computer only", () => {
    expect(sessionAccount(row({ computer: "studio" }), view).profile).toBe(peer);
    expect(sessionAccount(row({ computer: "elsewhere" }), view)).toEqual({});
  });

  it("finds no account for an unknown profile, another provider or an unloaded view", () => {
    expect(sessionAccount(row({ accountId: "codex-gone" }), view)).toEqual({});
    expect(sessionAccount(row({ cmd: "claude" }), view)).toEqual({});
    expect(sessionAccount(row(), emptyAccounts())).toEqual({});
  });

  it("uses an older session's home only when the binding evidence names one account", () => {
    const older = row({ accountId: "", accountHome: profile.home });
    expect(sessionAccount(older, view)).toEqual({});
    const evidence = [{ host: "", provider: "codex", id: profile.id, home: profile.home, group: work.key }];
    expect(sessionAccount(older, { ...view, binding_evidence: evidence, binding_evidence_complete: true }).profile).toBe(profile);
    const shared = [...evidence, { host: "", provider: "codex", id: "codex-old", home: profile.home, group: "codex\u001fother" }];
    expect(sessionAccount(older, { ...view, binding_evidence: shared, binding_evidence_complete: true })).toEqual({});
  });

  it("shortens homes under the usual user directories", () => {
    expect(homeShort("/home/user/.config/hgs/accounts/claude-a1")).toBe("~/.config/hgs/accounts/claude-a1");
    expect(homeShort("/Users/user/.codex")).toBe("~/.codex");
    expect(homeShort("/root/.kimi-code")).toBe("~/.kimi-code");
    expect(homeShort("/home/user")).toBe("~");
    expect(homeShort("C:\\Users\\user\\.codex")).toBe("~\\.codex");
    expect(homeShort("/srv/agents/codex")).toBe("/srv/agents/codex");
    expect(homeShort("/homestead/user/.codex")).toBe("/homestead/user/.codex");
    expect(homeShort("")).toBe("");
  });
});

describe("Activity goal caption", () => {
  it("pursues only an active goal and labels native terminal statuses", () => {
    for (const [status, caption] of Object.entries({ active: "Pursuing goal", complete: "Goal complete", paused: "Goal paused", blocked: "Goal blocked", usage_limited: "Usage limit reached", budget_limited: "Goal budget limit reached", unknown: "Goal" })) {
      expect(sessionGoal({ goal: { objective: "Fixture objective", status } })).toMatchObject({ objective: "Fixture objective", status, caption });
    }
    expect(sessionGoal({ goal: null }).objective).toBe("");
  });
});

// Task 206: the Compact skins' side panel of sections.
describe("side panel sections", () => {
  it("remember a fold against each section's default", () => {
    const none = new Set<string>();
    expect([cardOpen("sec-team", true, none), cardOpen("sec-rules", false, none)]).toEqual([true, false]);
    expect(cardKey("sec-team", true)).toBe("sec-team");
    expect(cardKey("sec-rules", false)).toBe("+sec-rules");
    const listed = new Set(["sec-team", "+sec-rules"]);
    expect([cardOpen("sec-team", true, listed), cardOpen("sec-rules", false, listed)]).toEqual([false, true]);
    // The tabs' cards keep their old entries: a listed id is a folded card.
    expect(cardOpen("details-id", true, new Set(["details-id"]))).toBe(false);
  });
  it("read the context window as Qt's Context card does", () => {
    expect(contextUsage({})).toBeUndefined();
    expect(contextUsage({ session_usage: { context: { used: 45_000, limit: 200_000 } } })).toEqual({ used: 45_000, limit: 200_000, percent: 23 });
    expect(contextUsage({ session_usage: { context: { used: 900 } } })).toEqual({ used: 900, limit: 0, percent: -1 });
    expect([compactCount(900), compactCount(45_000)]).toEqual(["900", "45k"]);
  });
  it("sum the team card: the current phase, the finished phases and the accepted tasks", () => {
    const phase = (n: number, done: number, total: number) => ({ n, title: "", state: "", status: "", done, total, barrier: false, auto: false, acceptance: "", paused: false, health: "" });
    const task = (status: string) => ({ status }) as never;
    const card = sideTeam({ phases: [phase(1, 3, 3), phase(2, 1, 4), phase(3, 0, 2), phase(4, 0, 0)], tasks: [task("accepted"), task("accepted"), task("cancelled"), task("assigned")], created: 1000 }, 4600);
    expect(card).toEqual({ phase: { n: 2, done: 1, total: 4 }, phases: { done: 1, total: 3 }, accepted: 2, total: 3, age: 3600 });
    expect(sideTeam({ phases: [phase(1, 2, 2)], tasks: [], created: 0 }, 10).phase).toEqual({ n: 1, done: 2, total: 2 });
  });
});
