// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
import type { CreationCard } from "../../types";
const state = vi.hoisted(() => ({ read: null as unknown as () => CreationCard, write: null as unknown as (card: CreationCard) => void }));
const calls = vi.hoisted(() => ({ edit: vi.fn(), action: vi.fn(), hide: vi.fn(), back: vi.fn() }));
vi.mock("../../store/creations", () => ({ selectedCard: () => state.read(), cardVersion: () => state.read().state,
  creationDetailsText: async () => state.read().error, editChoices: calls.edit, runCreationAction: calls.action,
  hideCreation: calls.hide, returnToFilters: calls.back }));
vi.mock("../../store/ui", () => ({ now: () => 1000 }));
vi.mock("../../i18n", () => ({ tr: (text: string) => text, trNoop: (text: string) => text }));
import { CreationPanel } from "./CreationPanel";
let dispose: (() => void) | undefined;
const failed = (): CreationCard => ({ id: "original", launch_id: "original-launch", tag: "example", cmd: "codex", account: "", group: "",
  host: "", machine: "fixture", target: "/srv/repo", state: "failed", caption: "Creation failed", created: 1000,
  error: "This session already has an account. Choose a new session name.", show_pending: true,
  launch_dispatched: true, stop_requested: false, worktree_receipts: false, can_resume: false, can_edit: true, bound_session: "" });
beforeEach(() => { vi.clearAllMocks(); const [read, write] = createSignal(failed()); state.read = read; state.write = write; });
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); });
const mount = () => { const host = document.createElement("div"); document.body.append(host); dispose = render(() => createComponent(CreationPanel, {}), host); return host; };
const flush = async () => { for (let i = 0; i < 5; i++) await Promise.resolve(); };

it("shows a confirmed failure reason and Edit choices without offering an original replay", async () => {
  const host = mount(); await flush();
  expect(host.textContent).toContain("Creation failed");
  expect(host.querySelector("textarea")?.value).toContain("already has an account");
  const buttons = [...host.querySelectorAll<HTMLButtonElement>("button")];
  const edit = buttons.find((b) => b.textContent === "Edit choices…")!;
  expect(edit).toBeDefined(); expect(buttons.some((b) => b.textContent === "Continue setup")).toBe(false);
  expect(buttons.some((b) => b.textContent === "Check result")).toBe(false);
  edit.click(); expect(calls.edit).toHaveBeenCalledExactlyOnceWith("original");
  buttons.find((b) => b.textContent === "Hide status")!.click();
  expect(calls.hide).toHaveBeenCalledExactlyOnceWith("original"); expect(calls.action).not.toHaveBeenCalled();
});

it("keeps an uncertain original check-only, without edit, Continue or automatic delivery", async () => {
  state.write({ ...failed(), state: "unknown", caption: "Creation result unknown", can_edit: false });
  const host = mount(); await flush();
  const buttons = [...host.querySelectorAll<HTMLButtonElement>("button")];
  expect(buttons.some((b) => b.textContent === "Edit choices…")).toBe(false);
  expect(buttons.some((b) => b.textContent === "Continue setup")).toBe(false);
  expect(calls.action).not.toHaveBeenCalled();
  buttons.find((b) => b.textContent === "Check result")!.click();
  expect(calls.action).toHaveBeenCalledExactlyOnceWith("original", "check");
  expect(calls.edit).not.toHaveBeenCalled();
});


it.each(["unknown", "recover"])("lets an explicit Stop retry its metadata fence from %s", async (phase) => {
  state.write({ ...failed(), state: phase, launch_dispatched: false, stop_requested: true, worktree_pending: true, can_edit: false });
  const host = mount(); await flush();
  const stop = [...host.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Stop creating")!;
  expect(stop.disabled).toBe(false); stop.click();
  expect(calls.action).toHaveBeenCalledExactlyOnceWith("original", "stop");
});

it("shows stopped but historically unknown setup with separate Recreate and dismissal", async () => {
  state.write({ ...failed(), state: "stopped", caption: "Creation stopped", launch_dispatched: false,
    worktree_pending: true, worktree_fenced: true, stop_requested: true,
    error: "Further starts are fenced. The earlier attempt's outcome is unknown. Review the destination.", can_edit: true });
  const host = mount(); await flush();
  expect(host.querySelector("textarea")?.value).toContain("outcome is unknown");
  const buttons = [...host.querySelectorAll<HTMLButtonElement>("button")];
  expect(buttons.some((b) => b.textContent === "Continue setup" || b.textContent === "Stop creating")).toBe(false);
  buttons.find((b) => b.textContent === "Recreate…")!.click();
  expect(calls.edit).toHaveBeenCalledExactlyOnceWith("original");
  buttons.find((b) => b.textContent === "Hide status")!.click();
  expect(calls.hide).toHaveBeenCalledExactlyOnceWith("original");
  expect(calls.action).not.toHaveBeenCalled();
});
