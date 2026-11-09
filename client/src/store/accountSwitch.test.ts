import { describe, expect, it } from "vitest";
import type { AccountsView, Profile } from "../types";
import { accountGroups, accountOf, accountOptions, accountSwitchRequested, consumeAccountSwitch, movable, moveOutcome, requestAccountSwitch, switchOutcome, switchable } from "./accountSwitch";

const profile = (id: string, provider: string, host: string, signed = true, state = "ready"): Profile => ({
  key: `${host}/${provider}/${id}`, host, machine: host, id, label: id.toUpperCase(), provider,
  native: false, installed: true, is_default: false, home: "", auth_revision: "", online: true,
  state, detail: "", signed_in: signed, permission: "", permission_label: "", permission_hint: "",
  model_supported: true, model_text: "", usage: { status: "ok", provider, label: id, identity: {}, windows: [], balances: [] },
  opencode: null,
} as unknown as Profile);

const view = (members: Profile[]): AccountsView => ({
  hosts: [], accounts: [{ key: "a", label: "A", provider: "claude", installed: true, machines: [""], members, usage: {}, permission: "", identity_lines: [], identity_note: null }],
  overview: [], snapshot: false, loading: false, pending: 0, loaded_machines: 1, busy: false, notices: [], no_hosts: null, truncated: 0,
} as unknown as AccountsView);

describe("session account switch", () => {
  it("offers the same agent's signed-in accounts, current first, and only claude/codex", () => {
    expect(switchable("claude")).toBe(true);
    expect(switchable("kimi")).toBe(false);
    const rows = accountOptions(view([
      profile("work", "claude", ""), profile("old", "claude", "", false), profile("codex", "codex", ""),
    ]), "", "claude", "work");
    expect(rows.map((row) => row.id)).toEqual(["work"]);
    expect(accountOf("claude", "")).toBe("native-claude");
  });

  it("words each receipt and a refusal", () => {
    expect(switchOutcome({ status: "switched", to: { account_id: "work", label: "Work" } }, "", "work")).toContain("Moved to Work");
    expect(switchOutcome({ status: "rolled_back", from: { label: "Old" }, reason: "bad" }, "", "Work")).toContain("Stayed on Old");
    expect(switchOutcome({}, "hgs: no such account", "work")).toBe("Not moved: no such account");
  });
});

describe("task 352: any account of any agent", () => {
  const groups = (members: Profile[], provider: string, current: string) =>
    accountGroups(view(members), "", provider, current).map((group) => [group.provider, group.rows.map((row) => row.id)]);

  it("groups every usable account by agent, the session's own first", () => {
    const members = [
      profile("work", "claude", ""), profile("second", "claude", ""), profile("old", "claude", "", false, "signed_out"),
      profile("codex", "codex", ""), profile("native-dsh", "dsh", "", false, "signed_out"), profile("native-opencode", "opencode", "", false, "unknown"),
      profile("kimi", "kimi", "", false), profile("far", "codex", "peer"),
    ];
    expect(groups(members, "claude", "work")).toEqual([
      ["claude", ["work", "second"]], ["codex", ["codex"]], ["dsh", ["native-dsh"]], ["opencode", ["native-opencode"]],
    ]);
    // From DeepSeek back to Claude: DeepSeek's own row is the current one.
    expect(groups(members, "dsh", "native-dsh")).toEqual([
      ["dsh", ["native-dsh"]], ["claude", ["work", "second"]], ["codex", ["codex"]], ["opencode", ["native-opencode"]],
    ]);
    expect(movable("dsh")).toBe(true);
    expect(movable("sh")).toBe(false);
  });

  it("leaves out an agent that is not installed here and a computer that is offline", () => {
    const missing = { ...profile("native-dsh", "dsh", ""), installed: false, state: "not-installed" } as Profile;
    const offline = { ...profile("codex", "codex", ""), online: false } as Profile;
    expect(groups([profile("work", "claude", ""), missing, offline], "claude", "work")).toEqual([["claude", ["work"]]]);
  });

  it("words a move's receipt", () => {
    expect(moveOutcome({ status: "moved", new_session: "dsh/p/work" }, "", "DeepSeek Native")).toContain("dsh/p/work");
    expect(moveOutcome({ status: "failed", reason: "no start", relaunched: true }, "", "DeepSeek")).toContain("the old session resumed");
    expect(moveOutcome({ status: "failed", reason: "no start", relaunched: false }, "", "DeepSeek")).toContain("Resume the old session");
    expect(moveOutcome({ status: "uncertain", new_session: "dsh/p/work" }, "", "DeepSeek")).toContain("did not confirm");
    expect(moveOutcome({}, "hgs: DeepSeek is not installed", "DeepSeek")).toBe("Not moved: DeepSeek is not installed");
  });
});

describe("account switch entry points", () => {
  it("asks the composer chip for one session and is consumed once", () => {
    requestAccountSwitch("pc\nrun:one");
    expect(accountSwitchRequested()).toBe("pc\nrun:one");
    expect(consumeAccountSwitch("pc\nrun:two")).toBe(false);
    expect(consumeAccountSwitch("pc\nrun:one")).toBe(true);
    expect(accountSwitchRequested()).toBe("");
  });
});


