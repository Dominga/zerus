// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import * as accounts from "../../store/accounts";
import { usageOf } from "../../store/testing";
import { emptyAccounts, type Account, type Profile, type Usage } from "../../types";
import { AccountsScreen } from "./Accounts";

vi.mock("../../store/latest", async (original) => ({
  ...await original<object>(), useNow: () => () => 1_793_000_000,
}));

let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); vi.restoreAllMocks(); });

const DSH_BALANCE = [
  { kind: "Total", balance: "6.20", currency: "USD" },
  { kind: "Topped up", balance: "6.20", currency: "USD" },
  { kind: "Granted", balance: "0.00", currency: "USD" },
];

function show(usage: Usage) {
  const member = {
    key: "local\u001fnative-dsh", host: "local", machine: "arch", id: "native-dsh", label: "DeepSeek API", provider: "dsh",
    native: true, installed: true, is_default: true, home: "/home/example/.dsh", auth_revision: "", online: true,
    state: "apikey", detail: { source: "API key configured on %1.", args: ["arch"] }, signed_in: false,
    permission: "bypass", permission_label: { source: "Bypass" }, permission_hint: { source: "" }, model_supported: false, model_text: { source: "" },
    usage, opencode: null,
  } as unknown as Profile;
  const account: Account = { key: "dsh-key", label: "DeepSeek API", provider: "dsh", installed: true, machines: ["arch"], members: [member],
    usage, permission: { source: "Bypass" }, identity_lines: [], identity_note: null };
  vi.spyOn(accounts, "accountsPage").mockReturnValue({ ...emptyAccounts(), snapshot: true, accounts: [account] });
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(AccountsScreen, {}), host);
  return host;
}

describe("DeepSeek API-key balance on Accounts", () => {
  it("shows the total balance in place of limit meters and every part in the usage panel", () => {
    const host = show(usageOf({ provider: "dsh", status: "configured", balances: DSH_BALANCE, balance_available: true,
      identity: { email: "", name: "", plan: "", organization: "", account_id: "api-key:abc", auth_method: "API key" } }));
    const row = host.querySelector(".acct-row");
    const summary = row?.querySelector(".acct-row-usage .um-summary");
    expect(summary?.querySelector("[aria-hidden='true']")?.textContent).toBe("$6.20");
    expect(summary?.getAttribute("title")).toBe("Balance: 6.20 USD\nTopped up: 6.20 USD\nGranted: 0.00 USD");
    expect(summary?.querySelector(".sr-only")?.textContent).toBe("Balance: 6.20 USD\nTopped up: 6.20 USD\nGranted: 0.00 USD");
    expect(row?.textContent).not.toContain("too low");
    const rows = [...host.querySelectorAll(".acct-usage .acct-balance")].map((r) => r.textContent);
    expect(rows).toEqual(["Total6.20 USD", "Topped up6.20 USD", "Granted0.00 USD"]);
    expect(host.querySelector(".acct-usage .acct-balance.total")?.textContent).toBe("Total6.20 USD");
    // With a balance, the panel no longer says the usage is elsewhere.
    expect(host.querySelector(".acct-usage")?.textContent).not.toContain("could not read");
    expect(host.querySelector(".acct-usage .set-status.k-warn")).toBeNull();
  });

  it("warns in the row and the panel when DeepSeek says the balance no longer covers API calls", () => {
    const host = show(usageOf({ provider: "dsh", status: "configured", balance_available: false,
      balances: [{ kind: "Total", balance: "0.00", currency: "USD" }, { kind: "Topped up", balance: "0.00", currency: "USD" }, { kind: "Granted", balance: "0.00", currency: "USD" }] }));
    expect(host.querySelector(".acct-row .acct-balance-low")?.textContent).toBe("Balance too low for API calls");
    expect(host.querySelector(".acct-usage .set-status.k-warn")?.textContent).toContain("DeepSeek reports the balance is too low for API calls.");
  });

  it("keeps the configured-key state without a balance and draws no empty meters", () => {
    const host = show(usageOf({ provider: "dsh", status: "configured" }));
    expect(host.querySelector(".acct-row .acct-row-usage")).toBeNull();
    expect(host.querySelector(".acct-usage")?.textContent).toContain("API key configured. Zerus could not read its DeepSeek balance; refresh to retry.");
  });

  it("shows account-wide spend and this computer's session usage in the usage panel", () => {
    const host = show(usageOf({ provider: "dsh", status: "configured", balances: DSH_BALANCE,
      spend: { since: 1_792_900_000, today: "2026-11-10", periods: [
        { period: "today", amounts: [{ currency: "USD", spent: "0.14", granted: "0.00" }], top_up: false, gap_from: null },
        { period: "month", amounts: [{ currency: "USD", spent: "1.20", granted: "0.00" }], top_up: false, gap_from: null }] },
      local_usage: { today: "2026-11-10", partial: true, reading: true, unreadable: 1, days: [
        { day: "2026-11-10", models: [{ model: "deepseek-flash", requests: 172, cache_hit: 16_549_760, cache_miss: 327_667, output: 172_520 }] }] } }));
    const spend = host.querySelector(".acct-usage .acct-spend");
    expect([...spend!.querySelectorAll(".acct-balance")].map((r) => r.textContent)).toEqual(["Spent today$0.14", "Spent this month$1.20"]);
    expect(spend!.textContent).toContain("Top-ups excluded.");
    const local = host.querySelector(".acct-usage .acct-local");
    expect(local!.querySelector("h4")?.textContent).toBe("This computer's DeepSeek sessions");
    expect(local!.textContent).toContain("Still reading session logs; totals may grow.");
    expect(local!.textContent).toContain("1 session log(s) could not be read and are not counted.");
    const rows = [...local!.querySelectorAll(".acct-usage-table tbody tr")].map((r) => [...r.children].map((c) => c.textContent));
    expect(rows.slice(0, 4)).toEqual([
      ["Today"], ["deepseek-flash", "172", "16.5M", "327.7K", "172.5K"],
      ["This month"], ["deepseek-flash", "172", "16.5M", "327.7K", "172.5K"],
    ]);
    expect(local!.querySelector(".acct-usage-table tbody td[title]")?.getAttribute("title")).toBe("172");
    expect(local!.querySelector("details summary")?.textContent).toBe("By day");
    // Neither section reads as a separator list.
    expect(host.querySelector(".acct-usage")?.textContent).not.toContain("\u00b7");
  });

  it("omits the spend and session sections for accounts that report neither", () => {
    const host = show(usageOf({ provider: "dsh", status: "configured", balances: DSH_BALANCE }));
    expect(host.querySelector(".acct-spend")).toBeNull();
    expect(host.querySelector(".acct-local")).toBeNull();
  });
});
