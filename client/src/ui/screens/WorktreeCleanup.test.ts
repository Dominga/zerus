// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createComponent, Show, type Accessor } from "solid-js";
import { render } from "solid-js/web";
const native = vi.hoisted(() => ({ clear: vi.fn(), offer: vi.fn(), cancel: vi.fn(), confirm: vi.fn(), sync: vi.fn() }));
vi.mock("../../bridge", () => ({ inTauri: () => false, terminationCleanupKeep: native.clear,
  terminationProgramForceOffer: native.offer, terminationProgramForceCancel: native.cancel, terminationProgramForceConfirm: native.confirm }));
vi.mock("../../store/catalog", () => ({ catalog: { rows: {} } }));
vi.mock("../../store/ui", () => ({ setPage: vi.fn(), setSelectedKey: vi.fn(), setDetailTab: vi.fn() }));
vi.mock("../../store/cleanupNotices", () => ({ anyCleanupRestore: () => false, cleanupRestoring: () => false, restoreCleanupRow: vi.fn(), syncCleanupNotices: native.sync }));
vi.mock("../../store/actions", () => ({ notify: vi.fn() }));
vi.mock("../../i18n", () => ({ tr: (s: string, ...args: unknown[]) => s.replace(/%(\d+)/g, (_, n) => String(args[Number(n) - 1])), trNoop: (s: string) => s }));
vi.mock("../lib/icons", () => ({ Icon: () => null }));
vi.mock("../components/MachineLabel", () => ({ MachineLabel: () => null }));
import WorktreeCleanup from "./WorktreeCleanup";
import { CleanupChip } from "./CleanupChip";
import { applyCleanup, cleanup, cleanupDialog, closeCleanup, emptyCleanup, openCleanup, type CleanupRow } from "../../store/terminationCleanup";
const row = (over: Partial<CleanupRow> = {}): CleanupRow => ({ id: "original", label: "worktree-fixture", path: "/srv/example/work", machine: "this computer", computer: "", branch: "kept-branch", size: "Not measured", status: "kept", reason: "Some programs are still running.", details: "", can_check: false, can_force: true, ready: false, ...over });
const flush = async () => { for (let i = 0; i < 12; i++) await Promise.resolve(); };
let revision = 0, dispose: (() => void) | undefined;
beforeEach(() => {
  vi.clearAllMocks(); closeCleanup(); applyCleanup({ ...emptyCleanup(), revision: ++revision });
  native.clear.mockImplementation(async () => ({ ...emptyCleanup(), revision: ++revision }));
  native.offer.mockResolvedValue({ confirmation: "native-handle", programs: [{ pid: 42, label: "original-survivor" }] });
  native.cancel.mockResolvedValue({ ...emptyCleanup(), revision: ++revision });
  vi.spyOn(HTMLDialogElement.prototype, "showModal").mockImplementation(function (this: HTMLDialogElement) { this.open = true; });
  vi.spyOn(HTMLDialogElement.prototype, "close").mockImplementation(function (this: HTMLDialogElement) { this.open = false; });
});
afterEach(() => { dispose?.(); dispose = undefined; closeCleanup(); document.body.replaceChildren(); vi.restoreAllMocks(); });
function mount() {
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => [createComponent(CleanupChip, {}), Show({ get when() { return cleanupDialog(); }, children: (_current: Accessor<NonNullable<ReturnType<typeof cleanupDialog>>>) => createComponent(WorktreeCleanup, {}) })], host);
  return host;
}
const button = (host: HTMLElement, text: string) => [...host.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === text)!;
const review = (host: HTMLElement) => host.querySelector<HTMLButtonElement>('button[aria-label="Review working folders"]')!;

it("keeps the permanent review button and opens a truthful empty state", () => {
  const host = mount(); expect(review(host)).toBeTruthy(); review(host).click();
  expect(host.textContent).toContain("No working folders need review.");
  expect(button(host, "Force stop…")).toBeUndefined();
  button(host, "Close").click(); expect(cleanupDialog()).toBeUndefined();
  review(host).click(); expect(host.querySelector("dialog[open]")).not.toBeNull();
  expect(native.clear).not.toHaveBeenCalled(); expect(native.offer).not.toHaveBeenCalled();
});

it("footer Close repeatedly unmounts and reopens the SAME kept row and Force offer", async () => {
  const original = row(); applyCleanup({ ...emptyCleanup(), revision: ++revision, rows: [original], kept: 1 });
  const host = mount();
  for (let i = 0; i < 2; i++) {
    review(host).click(); expect(host.textContent).toContain("worktree-fixture"); expect(button(host, "Force stop…")).toBeTruthy();
    button(host, "Close").click(); expect(native.clear).not.toHaveBeenCalled(); await flush();
    expect(cleanupDialog()).toBeUndefined(); expect(cleanup().rows).toEqual([original]);
    expect(native.clear).not.toHaveBeenCalled();
  }
  review(host).click(); button(host, "Force stop…").click(); await flush();
  expect(native.offer).toHaveBeenCalledExactlyOnceWith("original");
  expect(host.textContent).toContain("original-survivor (PID 42)");
  button(host, "Cancel").click(); await flush(); expect(native.cancel).toHaveBeenCalledExactlyOnceWith("original", "native-handle");
  expect(native.confirm).not.toHaveBeenCalled();
});

it("X and Escape dismiss without clearing, including during background work", () => {
  applyCleanup({ ...emptyCleanup(), revision: ++revision, rows: [row({ busy: true, status: "stopping" })], busy: true });
  const host = mount(); review(host).click();
  host.querySelector<HTMLButtonElement>('button[aria-label="Close"]')!.click();
  expect(cleanupDialog()).toBeUndefined(); review(host).click();
  host.querySelector("dialog")!.dispatchEvent(new Event("cancel", { cancelable: true }));
  expect(cleanupDialog()).toBeUndefined(); expect(cleanup().rows).toHaveLength(1); expect(native.clear).not.toHaveBeenCalled();
});

it("clears only on the explicit action and keeps retained rows and their review open", async () => {
  const retained = row(); applyCleanup({ ...emptyCleanup(), revision: ++revision, rows: [retained, row({ id: "settled", can_force: false })], kept: 2, can_clear: true });
  native.clear.mockImplementation(async () => ({ ...emptyCleanup(), revision: ++revision, rows: [retained], kept: 1, can_clear: false }));
  const host = mount(); review(host).click(); button(host, "Clear settled results").click(); await flush();
  expect(native.clear).toHaveBeenCalledTimes(1); expect(cleanup().rows).toEqual([retained]);
  expect(cleanupDialog()).toBeTruthy(); expect(button(host, "Force stop…")).toBeTruthy(); await vi.waitFor(() => expect(native.sync).toHaveBeenCalled());
  expect(button(host, "Clear settled results")).toBeUndefined();
});
