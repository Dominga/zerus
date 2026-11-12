// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import type { AccountsView, TeamMemberView, TeamView } from "../../types";
import { SwapMemberDialog } from "./SwapMemberDialog";

vi.mock("../../bridge", () => ({ teamRoleModels: vi.fn(async () => []) }));
vi.mock("../../store/catalog", () => ({ catalog: { rows: {}, order: [] } }));
vi.mock("./ProposalCards", () => ({ windowPort: {} }));
vi.mock("../../store/accounts", () => {
  const member = (host: string, id: string, label: string) => ({
    key: `${host}:${id}`, host, machine: host || "local", id, label, provider: "codex", native: false,
    installed: true, is_default: false, signed_in: true, usage: { identity: {} },
  });
  const account = (host: string, id: string, label: string) => ({
    key: `${host}:${id}`, label, provider: "codex", installed: true, machines: [host], members: [member(host, id, label)],
    usage: { identity: {} }, permission: { source: "" }, identity_lines: [], identity_note: null,
  });
  return {
    accountsPage: () => ({ accounts: [account("", "local", "Local Work"), account("studio", "remote", "Remote Work")] }) as unknown as AccountsView,
    loadAccounts: vi.fn(),
  };
});

describe("Swap member account selection", () => {
  it("offers accounts from the dialog's computer", () => {
    const team = { id: "team", name: "Team", members: [] } as unknown as TeamView;
    const member = { id: "dev", role: "Developer", agent: "codex", model: "", effort: "", account: "" } as unknown as TeamMemberView;
    const host = document.createElement("div");
    document.body.append(host);
    const dispose = render(() => createComponent(SwapMemberDialog, { computer: "studio", team, member, close: () => {} }), host);
    const html = host.innerHTML;
    dispose();
    host.remove();
    expect(html).toContain("Remote Work");
    expect(html).not.toContain("Local Work");
  });
});
