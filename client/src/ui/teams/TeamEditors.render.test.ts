// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
import { TeamWorkspace } from "./TeamTab";
import { teamView } from "./fixtures";
import type { TeamOutcome, TeamView } from "../../store/teams";
import { setAccountsPage } from "../../store/accounts";
import { emptyAccounts } from "../../types";
vi.mock("../../store/accounts", async () => {
  const { createSignal } = await import("solid-js");
  const { emptyAccounts } = await import("../../types");
  const [accountsPage, setAccountsPage] = createSignal(emptyAccounts());
  return { accountsPage, setAccountsPage, loadAccounts: vi.fn(async () => undefined) };
});
let dispose: (() => void) | undefined;
beforeEach(() => {
  HTMLDialogElement.prototype.showModal = function () { this.open = true; };
  HTMLDialogElement.prototype.close = function () { this.open = false; };
  setAccountsPage(emptyAccounts());
});
afterEach(() => { dispose?.(); document.body.replaceChildren(); });
const click = (host: ParentNode, text: string) => {
  const button = [...host.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent?.trim() === text);
  expect(button, text).toBeTruthy(); button!.click();
};
const flush = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };
function setup(result: TeamOutcome) {
  const [view, setView] = createSignal<TeamView>({ ...teamView, revision: 20, member_roles: [{ id: "developer", name: "Developer", agent: "claude", model: "role-model", effort: "high", account: { mode: "freest" } }] });
  const action = vi.fn(async () => result);
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(TeamWorkspace, { get view() { return view(); }, busy: false, failure: null, onAction: action, onPage: () => {}, onAddTask: () => {} }), host);
  return { host, action, setView, view };
}
it("opens without a write and sends rename at the revision shown when opened", async () => {
  const { host, action, setView, view } = setup({ outcome: "done", request_id: "example", receipt: {} });
  click(host, "Rename team"); expect(action).not.toHaveBeenCalled();
  const input = host.querySelector<HTMLInputElement>('input[aria-label="Team name"]')!;
  input.value = "Readable team"; input.dispatchEvent(new Event("input", { bubbles: true }));
  setView({ ...view(), revision: 21 });
  click(host, "Save"); await flush();
  expect(action).toHaveBeenCalledExactlyOnceWith("rename_team", { name: "Readable team" }, 20, teamView.bar.id);
});
it("adds a pinned role with explicit seat choices; changing agent clears incompatible choices", async () => {
  const { host, action } = setup({ outcome: "done", request_id: "example", receipt: {} });
  click(host, "Add member"); expect(action).not.toHaveBeenCalled();
  const agent = host.querySelector<HTMLSelectElement>('select[aria-label="Agent"]')!;
  agent.value = "codex"; agent.dispatchEvent(new Event("change", { bubbles: true }));
  const model = host.querySelector<HTMLInputElement>('input[aria-label="Model"]')!;
  model.value = "example-model"; model.dispatchEvent(new Event("input", { bubbles: true }));
  click(host.querySelector("dialog")!, "Add member"); await flush();
  expect(action).toHaveBeenCalledExactlyOnceWith("add_member", { role: "developer", agent: "codex", model: "example-model", effort: "", account: { mode: "freest" } }, 20, teamView.bar.id);
});
it("an unknown result is never submitted again and the form remains closeable", async () => {
  const { host, action } = setup({ outcome: "uncertain", request_id: "example", detail: "Lost answer" });
  click(host, "Edit goal"); click(host, "Save"); await flush();
  click(host, "Save"); await flush(); expect(action).toHaveBeenCalledTimes(1);
  expect(host.querySelector('[role="alert"]')?.textContent).toContain("use Check");
  click(host, "Cancel"); expect(host.querySelector("dialog")).toBeNull();
});

it("changing the selected team closes the form without writing to either team", async () => {
  const { host, action, setView, view } = setup({ outcome: "done", request_id: "example", receipt: {} });
  click(host, "Rename team");
  setView({ ...view(), bar: { ...view().bar, id: "another-team" } });
  await flush();
  expect(host.querySelector("dialog")).toBeNull();
  expect(action).not.toHaveBeenCalled();
});
