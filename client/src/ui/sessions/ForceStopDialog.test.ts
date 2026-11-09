// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createComponent, createSignal, Show, type Accessor } from "solid-js";
import { render } from "solid-js/web";
import type { CleanupView } from "../../store/terminationCleanup";

const native = vi.hoisted(() => ({ offer: vi.fn(), confirm: vi.fn(), cancel: vi.fn(), apply: vi.fn(), notify: vi.fn() }));
vi.mock("../../bridge", () => ({ terminationProgramForceOffer: native.offer, terminationProgramForceConfirm: native.confirm, terminationProgramForceCancel: native.cancel }));
const state = vi.hoisted(() => ({
  read: null as unknown as () => Pick<CleanupView, "revision" | "rows">,
  write: null as unknown as (value: Pick<CleanupView, "revision" | "rows">) => void,
}));
vi.mock("../../store/terminationCleanup", () => ({ applyCleanup: native.apply, cleanup: () => state.read() }));
vi.mock("../../store/actions", () => ({ notify: native.notify }));
vi.mock("../../i18n", () => ({ tr: (s: string, ...args: unknown[]) => s.replace(/%(\d+)/g, (_, n) => String(args[Number(n) - 1])) }));
vi.mock("../lib/icons", () => ({ Icon: () => null }));
import { ForceStopDialog } from "./ForceStopDialog";

const id = "original-cleanup-row", confirmation = "fresh-native-confirmation";
const offered = () => ({ confirmation, remaining_millis: 60_000, revision: 1, programs: [{ pid: 42, label: "original-survivor" }] });
const flush = async () => { for (let i = 0; i < 12; i++) await Promise.resolve(); };
let dispose: (() => void) | undefined;
beforeEach(() => {
  vi.clearAllMocks();
  const [view, setView] = createSignal<Pick<CleanupView, "revision" | "rows">>({ revision: 0, rows: [] });
  state.read = view; state.write = setView;
  native.offer.mockResolvedValue(offered());
  native.confirm.mockResolvedValue({ revision: 2 });
  native.cancel.mockResolvedValue({ revision: 3 });
  // Only the platform modal API is stubbed; Solid renders/unmounts the real
  // component, children, reactive props and event handlers in the DOM.
  vi.spyOn(HTMLDialogElement.prototype, "showModal").mockImplementation(function (this: HTMLDialogElement) { this.open = true; });
  vi.spyOn(HTMLDialogElement.prototype, "close").mockImplementation(function (this: HTMLDialogElement) { this.open = false; });
});
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); vi.restoreAllMocks(); vi.useRealTimers(); });

function mount() {
  const host = document.createElement("div"); document.body.append(host);
  const [row, setRow] = createSignal<{ id: string; label: string } | undefined>({ id, label: "worktree-fixture" });
  const close = vi.fn(() => setRow(undefined));
  dispose = render(() => Show({
    get when() { return row(); },
    keyed: false,
    children: (current: Accessor<{ id: string; label: string }>) => createComponent(ForceStopDialog, {
      // This is the production WorktreeCleanup non-keyed Show boundary.
      get id() { return current().id; }, get label() { return current().label; }, close,
    }),
  }), host);
  return { host, close, visible: () => !!row() };
}
const force = (host: HTMLElement) => host.querySelector<HTMLButtonElement>("button.solid-danger")!;

it("submits the original handle once across the real parent Show unmount", async () => {
  const { host, close, visible } = mount(); await flush();
  expect(host.textContent).toContain("original-survivor (PID 42)");
  const button = force(host); expect(button.disabled).toBe(false);
  button.click(); button.click(); await flush();
  expect(native.confirm).toHaveBeenCalledExactlyOnceWith(id, confirmation);
  expect(native.apply).toHaveBeenCalledExactlyOnceWith({ revision: 2 });
  expect(close).toHaveBeenCalledTimes(1); expect(visible()).toBe(false);
  expect(host.querySelector("dialog")).toBeNull();
  expect(native.cancel).not.toHaveBeenCalled(); expect(native.notify).not.toHaveBeenCalled();
});

it("cancels the original unconsumed offer even after its Show accessor is invalidated", async () => {
  const { host, close } = mount(); await flush();
  host.querySelector<HTMLButtonElement>("button.primary")!.click(); await flush();
  expect(close).toHaveBeenCalledTimes(1);
  expect(native.cancel).toHaveBeenCalledExactlyOnceWith(id, confirmation);
  expect(native.confirm).not.toHaveBeenCalled();
});

it("cancels a late native list once with its original row after unmount", async () => {
  let finish!: (value: ReturnType<typeof offered>) => void;
  native.offer.mockReturnValueOnce(new Promise((resolve) => { finish = resolve; }));
  const { host } = mount();
  expect(force(host).disabled).toBe(true);
  host.querySelector<HTMLButtonElement>("button.primary")!.click();
  finish(offered()); await flush();
  expect(native.cancel).toHaveBeenCalledExactlyOnceWith(id, confirmation);
  expect(native.confirm).not.toHaveBeenCalled();
});

it.each(["The session state changed.", "The original program stop is unconfirmed."])("keeps %s visible on the consumed confirmation without another input", async (reason) => {
  native.confirm.mockRejectedValueOnce(reason);
  const { host, visible } = mount(); await flush(); force(host).click(); await flush();
  expect(visible()).toBe(true);
  expect(host.querySelector('[role="alert"]')?.textContent).toBe(reason);
  expect(force(host).disabled).toBe(true); force(host).click();
  expect(native.confirm).toHaveBeenCalledExactlyOnceWith(id, confirmation);
  const close = host.querySelector<HTMLButtonElement>("button.primary")!;
  expect(close.textContent).toBe("Close"); close.click(); await flush();
  expect(visible()).toBe(false); expect(native.cancel).not.toHaveBeenCalled();
});

it("also displays a synchronous bridge refusal, never an uncaught event error", async () => {
  native.confirm.mockImplementationOnce(() => { throw Error("native transport unavailable"); });
  const { host } = mount(); await flush(); force(host).click(); await flush();
  expect(host.querySelector('[role="alert"]')?.textContent).toContain("native transport unavailable");
  expect(force(host).disabled).toBe(true);
  expect(native.confirm).toHaveBeenCalledExactlyOnceWith(id, confirmation);
});

it("does not dismiss or resubmit while the one native admission is pending", async () => {
  let finish!: (value: unknown) => void;
  native.confirm.mockReturnValueOnce(new Promise((resolve) => { finish = resolve; }));
  const { host, close } = mount(); await flush(); force(host).click();
  expect(force(host).disabled).toBe(true);
  expect(host.querySelector<HTMLButtonElement>("button.primary")!.disabled).toBe(true);
  const cancel = new Event("cancel", { cancelable: true }); host.querySelector("dialog")!.dispatchEvent(cancel);
  expect(cancel.defaultPrevented).toBe(true); expect(close).not.toHaveBeenCalled();
  finish({ revision: 2 }); await flush();
  expect(native.confirm).toHaveBeenCalledExactlyOnceWith(id, confirmation);
  expect(close).toHaveBeenCalledTimes(1);
});

it("expires at the original remaining budget with no Force input", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "performance"] });
  native.offer.mockResolvedValueOnce({ ...offered(), remaining_millis: 100 });
  const { host } = mount(); await flush(); expect(force(host).disabled).toBe(false);
  vi.advanceTimersByTime(100); await flush();
  expect(force(host).disabled).toBe(true); expect(host.textContent).toContain("Force stop offer expired");
  force(host).click(); expect(native.confirm).not.toHaveBeenCalled();
  host.querySelector<HTMLButtonElement>("button.primary")!.click(); await flush();
  expect(native.cancel).toHaveBeenCalledExactlyOnceWith(id, confirmation);
});

it("subtracts the whole offer round trip instead of starting another budget", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "performance"] });
  let finish!: (value: ReturnType<typeof offered>) => void;
  native.offer.mockReturnValueOnce(new Promise((resolve) => { finish = resolve; }));
  const { host } = mount(); vi.advanceTimersByTime(150);
  finish({ ...offered(), remaining_millis: 100 }); await flush();
  expect(force(host).disabled).toBe(true); expect(host.textContent).toContain("Force stop offer expired");
  force(host).click(); expect(native.confirm).not.toHaveBeenCalled();
});

it("checks the deadline at the click even if the browser timer has not run", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "performance"] });
  native.offer.mockResolvedValueOnce({ ...offered(), remaining_millis: 100 });
  const { host } = mount(); await flush();
  vi.spyOn(performance, "now").mockReturnValue(101);
  expect(force(host).disabled).toBe(false); force(host).click();
  expect(force(host).disabled).toBe(true); expect(native.confirm).not.toHaveBeenCalled();
});

it("retires a mounted offer when the original native generation disappears", async () => {
  const { host } = mount(); await flush(); expect(force(host).disabled).toBe(false);
  state.write({ revision: 1, rows: [] }); await flush();
  expect(force(host).disabled).toBe(true); expect(host.textContent).toContain("no longer current");
  force(host).click(); expect(native.confirm).not.toHaveBeenCalled();
  // A later view is DATA, never a new offer or consent. It cannot re-enable.
  state.write({ revision: 0, rows: [] }); await flush();
  expect(force(host).disabled).toBe(true); expect(native.offer).toHaveBeenCalledTimes(1);
});

it("updates a throttled expiry when the GUI regains focus without any input", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "performance"] });
  native.offer.mockResolvedValueOnce({ ...offered(), remaining_millis: 100 });
  const { host } = mount(); await flush();
  vi.spyOn(performance, "now").mockReturnValue(101);
  window.dispatchEvent(new Event("focus"));
  expect(force(host).disabled).toBe(true); expect(host.textContent).toContain("Force stop offer expired");
  expect(native.confirm).not.toHaveBeenCalled();
});

it("never replaces a native unconfirmed effect with a later timeout", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "performance"] });
  native.offer.mockResolvedValueOnce({ ...offered(), remaining_millis: 100 });
  const { host } = mount(); await flush();
  state.write({ revision: 1, rows: [{ id, label: "work", path: "/srv/example/work", machine: "", computer: "", branch: "work", size: "", status: "kept", reason: "", details: "", can_check: false, ready: false, force_revision: null, program_retirement: "stop_unconfirmed" }] });
  vi.advanceTimersByTime(100); await flush();
  expect(force(host).disabled).toBe(true); expect(host.textContent).toContain("program stop is unconfirmed");
  expect(host.textContent).not.toContain("Force stop offer expired");
  force(host).click(); expect(native.confirm).not.toHaveBeenCalled();
});

it("explains a settled native generation rather than leaving a disabled button unexplained", async () => {
  const { host } = mount(); await flush();
  state.write({ revision: 1, rows: [{ id, label: "work", path: "/srv/example/work", machine: "", computer: "", branch: "work", size: "", status: "kept", reason: "", details: "", can_check: false, ready: false, force_revision: null, program_retirement: "cleanup_settled" }] });
  expect(force(host).disabled).toBe(true); expect(host.textContent).toContain("no longer current");
  force(host).click(); expect(native.confirm).not.toHaveBeenCalled();
});
