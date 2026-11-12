// @vitest-environment happy-dom
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
import type { AccountsView, Profile } from "../../types";
import type { Session } from "./types";
import { ProviderSelectionButton } from "./ProviderSelectionButton";

const port = vi.hoisted(() => ({ opened: (): boolean => false, open: () => {}, close: () => {}, apply: vi.fn() }));
vi.mock("../../store/sessionModels", () => ({ sessionModels: {
  view: () => ({ accessible: "Model and effort", tooltip: "Private fixture", shownModel: "First", suffix: "  low", enabled: true, terminal: true, openCode: false, hint: "Private fixture", changeNotice: "", applyLabel: "Apply" }),
  opened: () => port.opened(), open: () => port.open(), close: () => port.close(), model: () => "first", chooseModel: () => {},
  effort: () => "low", chooseEffort: () => {}, options: () => [{ id: "first", label: "First" }],
  efforts: () => [{ value: "low", label: "Low" }, { value: "high", label: "High" }], catalogError: () => "", canApply: () => true, apply: () => port.apply(),
} }));
vi.mock("../../store/terminal", () => ({ terminal: { opened: vi.fn() } }));
vi.mock("../../store/ui", () => ({ setDetailTab: vi.fn() }));
vi.mock("./AccountSwitchChip", () => ({ AccountSwitchChip: () => null }));
vi.mock("../../store/accounts", () => {
  const profile = { key: "work", host: "", id: "work", label: "Default account", provider: "codex", usage: { identity: {} } } as unknown as Profile;
  const account = { key: "work", label: "Work", provider: "codex", members: [profile], usage: { identity: {} } };
  return { accountsPage: () => ({ accounts: [account] }) as unknown as AccountsView };
});

beforeAll(async () => { await import("./ProviderSelectionDialog"); }, 60_000);
let host: HTMLDivElement, dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); document.body.replaceChildren(); vi.clearAllMocks(); });
const flush = async () => { await vi.dynamicImportSettled(); for (let i = 0; i < 5; i++) await Promise.resolve(); };
function setup() {
  const [opened, setOpened] = createSignal(false);
  port.opened = opened;
  port.open = () => setOpened(true);
  port.close = () => setOpened(false);
  port.apply = vi.fn(() => setOpened(false));
  const session = { accountProvider: "codex", accountId: "work", computer: "", agent: "codex" } as Session;
  host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(ProviderSelectionButton, { session }), host);
  return host.querySelector<HTMLButtonElement>(".provider-selection-button")!;
}

describe("Provider selection in the composer", () => {
  it("opens account, model and effort together and cancels without applying", async () => {
    const button = setup();
    expect(button.textContent).toContain("Work");
    expect(button.textContent).toContain("First");
    button.click(); await flush();
    const dialog = host.querySelector<HTMLDialogElement>("dialog.provider-selection-dialog")!;
    expect(dialog.open).toBe(true);
    expect(dialog.textContent).toContain("Account");
    expect(dialog.querySelector(".provider-selection-account-row")).not.toBeNull();
    expect(dialog.querySelector("#session-model-choice")).not.toBeNull();
    [...dialog.querySelectorAll<HTMLButtonElement>("button")].find((item) => item.textContent === "Cancel")!.click(); await flush();
    expect(host.querySelector("dialog")).toBeNull();
    expect(document.activeElement).toBe(button);
    expect(port.apply).not.toHaveBeenCalled();
  });

  it("applies the existing session model operation and closes the combined dialog", async () => {
    const button = setup(); button.click(); await flush();
    const dialog = host.querySelector<HTMLDialogElement>("dialog.provider-selection-dialog")!;
    [...dialog.querySelectorAll<HTMLButtonElement>("button")].find((item) => item.textContent === "Apply")!.click(); await flush();
    expect(port.apply).toHaveBeenCalledTimes(1);
    expect(host.querySelector("dialog")).toBeNull();
  });
});
