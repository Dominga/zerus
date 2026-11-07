// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createComponent, Show, type Accessor } from "solid-js";
import { render } from "solid-js/web";
const native = vi.hoisted(() => ({ clear: vi.fn(), offer: vi.fn(), cancel: vi.fn(), confirm: vi.fn(), sync: vi.fn(), dismiss: vi.fn(), remove: vi.fn(), inspect: vi.fn(), restore: vi.fn(), manual: vi.fn(), manualCancel: vi.fn() }));
vi.mock("../../bridge", () => ({ inTauri: () => false, terminationCleanupKeep: native.clear, terminationCleanupDismiss: native.dismiss, terminationCleanupRemove: native.remove, terminationCleanupInspect: native.inspect, terminationCleanupManualOffer: native.manual, terminationCleanupManualCancel: native.manualCancel,
  terminationProgramForceOffer: native.offer, terminationProgramForceCancel: native.cancel, terminationProgramForceConfirm: native.confirm }));
const catalogState = vi.hoisted(() => ({ rows: {} as Record<string, { key: string; computer: string; name: string }> }));
const navigation = vi.hoisted(() => ({ select: vi.fn(), tab: vi.fn(), page: vi.fn() }));
vi.mock("../../store/catalog", () => ({ catalog: catalogState }));
vi.mock("../../store/ui", () => ({ setPage: navigation.page, setSelectedKey: navigation.select, setDetailTab: navigation.tab }));
vi.mock("../../store/cleanupNotices", () => ({ anyCleanupRestore: () => false, cleanupRestoring: () => false, restoreCleanupRow: native.restore, syncCleanupNotices: native.sync }));
vi.mock("../../store/actions", () => ({ notify: vi.fn() }));
vi.mock("../../i18n", () => ({ tr: (s: string, ...args: unknown[]) => s.replace(/%(\d+)/g, (_, n) => String(args[Number(n) - 1])), trNoop: (s: string) => s }));
vi.mock("../lib/icons", () => ({ Icon: () => null }));
vi.mock("../components/MachineLabel", () => ({ MachineLabel: () => null }));
import WorktreeCleanup from "./WorktreeCleanup";
import { CleanupChip, CleanupReviewButton } from "./CleanupChip";
import { applyCleanup, cleanup, cleanupDialog, closeCleanup, emptyCleanup, openCleanup, type CleanupRow, groupCleanupRows } from "../../store/terminationCleanup";
const row = (over: Partial<CleanupRow> = {}): CleanupRow => ({ id: "original", label: "worktree-fixture", path: "/srv/example/work", machine: "this computer", computer: "", branch: "kept-branch", size: "Not measured", status: "kept", reason: "Some programs are still running.", details: "", can_check: false, can_force: true, ready: false, ...over });
const flush = async () => { for (let i = 0; i < 12; i++) await Promise.resolve(); };
let revision = 0, dispose: (() => void) | undefined;
beforeEach(() => {
  vi.clearAllMocks(); catalogState.rows = {}; closeCleanup(); applyCleanup({ ...emptyCleanup(), revision: ++revision });
  native.inspect.mockResolvedValue({ path: "unrelated fixture response" });
  native.dismiss.mockImplementation(async () => ({ ...emptyCleanup(), revision: ++revision }));
  native.clear.mockImplementation(async () => ({ ...emptyCleanup(), revision: ++revision }));
  native.offer.mockImplementation(async () => ({ confirmation: "native-handle", remaining_millis: 60_000, revision: cleanup().revision + 1, programs: [{ pid: 42, label: "original-survivor" }] }));
  native.cancel.mockResolvedValue({ ...emptyCleanup(), revision: ++revision });
  vi.spyOn(HTMLDialogElement.prototype, "showModal").mockImplementation(function (this: HTMLDialogElement) { this.open = true; });
  vi.spyOn(HTMLDialogElement.prototype, "close").mockImplementation(function (this: HTMLDialogElement) { this.open = false; });
});
afterEach(() => { dispose?.(); dispose = undefined; closeCleanup(); document.body.replaceChildren(); vi.restoreAllMocks(); });
function mount() {
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => [createComponent(CleanupReviewButton, {}), createComponent(CleanupChip, {}), Show({ get when() { return cleanupDialog(); }, children: (_current: Accessor<NonNullable<ReturnType<typeof cleanupDialog>>>) => createComponent(WorktreeCleanup, {}) })], host);
  return host;
}
const button = (host: HTMLElement, text: string) => [...host.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === text)!;
const review = (host: HTMLElement) => host.querySelector<HTMLButtonElement>("button.cleanup-review")!;

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

it("renders one folder with every source and names the session and program blockers", () => {
  const reason = "Cannot remove this folder while another session is working. " + "Full readable reason. ".repeat(50);
  const sources = ["claude/example/first", "claude/example/second", "claude/example/third"];
  catalogState.rows.live = { key: "run:live", computer: "", name: "claude/example/fourth" };
  const rows = sources.map((name, i) => row({ id: `origin-${i}`, can_force: false, can_dismiss: true, folder_key: "same-native-folder", source: { computer: "", name, run_id: `run-${i}`, conversation_id: `conv-${i}` }, sessions: ["claude/example/fourth"], processes: [{ pid: 42, label: "report-server" }], reason }));
  applyCleanup({ ...emptyCleanup(), revision: ++revision, rows, kept: 1, can_clear: true });
  const host = mount(); review(host).click();
  expect(host.querySelectorAll("tbody tr")).toHaveLength(1);
  for (const name of sources) expect(host.textContent).toContain(name);
  expect(host.textContent).toContain("claude/example/fourth");
  expect(host.textContent).toContain("report-server (PID 42)");
  expect(host.querySelector(".wtc-reason")!.textContent).toBe(reason);
  expect(host.querySelector(".wtc-reason")!.classList.contains("ell")).toBe(false);
  button(host, "Open session").click();
  expect(navigation.select).toHaveBeenCalledExactlyOnceWith("run:live");
  expect(navigation.page).toHaveBeenCalledWith("sessions");
  expect(native.clear).not.toHaveBeenCalled(); expect(native.confirm).not.toHaveBeenCalled();
});

it("dismisses one unknown result explicitly and never bulk clears or removes it", async () => {
  const unknown = row({ id: "unknown-original", status: "unknown", can_force: false, can_dismiss: true });
  applyCleanup({ ...emptyCleanup(), revision: ++revision, rows: [unknown], unknown: 1, can_clear: false });
  const expectedRevision = revision;
  const host = mount(); review(host).click();
  expect(button(host, "Clear settled results")).toBeUndefined();
  button(host, "Dismiss").click(); await flush();
  expect(native.dismiss).toHaveBeenCalledExactlyOnceWith("unknown-original", expectedRevision);
  expect(native.clear).not.toHaveBeenCalled(); expect(native.remove).not.toHaveBeenCalled();
  expect(native.confirm).not.toHaveBeenCalled();
  expect(host.textContent).toContain("No working folders need review.");
});

it("keeps the pending original Force identity in a grouped folder and offers no Dismiss", async () => {
  const settled = row({ id: "settled", can_force: false, can_dismiss: false, folder_key: "shared" });
  const pending = row({ id: "original-live", can_force: true, can_dismiss: false, folder_key: "shared" });
  applyCleanup({ ...emptyCleanup(), revision: ++revision, rows: [settled, pending], kept: 1, can_clear: false });
  const host = mount(); review(host).click();
  expect(host.querySelectorAll("tbody tr")).toHaveLength(1);
  expect(button(host, "Dismiss")).toBeUndefined();
  button(host, "Force stop…").click(); await flush();
  expect(native.offer).toHaveBeenCalledExactlyOnceWith("original-live");
  expect(native.dismiss).not.toHaveBeenCalled();
  button(host, "Cancel").click(); await flush();
  expect(native.cancel).toHaveBeenCalledWith("original-live", "native-handle");
});

it("reads current blockers without changing the recorded removal decision", async () => {
  const original = row({ can_force: false, can_dismiss: true, sessions: [], processes: [], reason: "The folder is still in use." });
  native.inspect.mockResolvedValue({ path: original.path, common_dir: "/srv/example/.git", review: {sessions: ["claude/example/current"], processes: [{pid: 84, command: "current-report-server"}]} });
  applyCleanup({ ...emptyCleanup(), revision: ++revision, rows: [original], kept: 1 });
  const host = mount(); review(host).click(); await flush();
  expect(host.textContent).toContain("claude/example/current");
  expect(host.textContent).toContain("current-report-server (PID 84)");
  expect(cleanup().rows[0]).toBe(original);
  expect(native.remove).not.toHaveBeenCalled(); expect(native.confirm).not.toHaveBeenCalled();
});

it("preserves raw pending metadata and keeps computers and repositories separate", () => {
  const pending = { ...row({folder_key: "machine-repository-a", can_force: true}), force_revision: 7 };
  const groups = groupCleanupRows([row({id:"settled", folder_key:"machine-repository-a", can_force:false}), pending, row({id:"other-repo", folder_key:"machine-repository-b"}), row({id:"other-computer", computer:"other", folder_key:"other-machine"})]);
  expect(groups).toHaveLength(3);
  expect(groups[0].primary).toBe(pending);
  expect(groups[0].rows[1]).toBe(pending);
  expect((groups[0].primary as typeof pending).force_revision).toBe(7);
});

 it("preserves each archived source restore action in a grouped folder", () => {
  const rows = ["first", "second"].map((name) => {
    const target = {computer: "", name, run_id: name + "-run", conversation_id: name + "-conversation"};
    return row({id: name, can_force: false, folder_key: "shared-folder", source: target, restore: {target, machine: "machine", archive_id: name + "-archive", status: "kept", overflow: false}});
  });
  applyCleanup({...emptyCleanup(), revision: ++revision, rows, kept: 1});
  const host = mount(); review(host).click();
  button(host, "Restore session first").click(); button(host, "Restore session second").click();
  expect(native.restore.mock.calls.map(([row]) => row.id)).toEqual(["first", "second"]);
});

it("never displays a previous computer's blocker during the next same-path inspection", async () => {
  const alpha = row({id:"alpha", computer:"alpha", can_force:false, folder_key:"alpha-folder", sessions:[]});
  const beta = row({id:"beta", computer:"beta", can_force:false, folder_key:"beta-folder", sessions:[]});
  native.inspect.mockImplementation((id:string)=>id==="alpha" ? Promise.resolve({path:alpha.path,review:{sessions:["alpha-only-blocker"]}}) : new Promise(()=>{}));
  applyCleanup({...emptyCleanup(),revision:++revision,rows:[alpha,beta],kept:2});
  const host=mount();review(host).click();await flush();expect(host.textContent).toContain("alpha-only-blocker");
  host.querySelectorAll<HTMLTableRowElement>("tbody tr")[1].click();await flush();
  expect(host.textContent).not.toContain("alpha-only-blocker");
});

it("keeps the original unknown outcome visible beside a later lease retirement", () => {
  applyCleanup({ ...emptyCleanup(), revision: ++revision, unknown: 1, rows: [row({ status: "unknown", can_force: false,
    reason: "Removal could not be confirmed. Check the original result before continuing.", program_retirement: "expired" })] });
  const host = mount(); review(host).click();
  expect(host.textContent).toContain("Removal could not be confirmed");
  expect(host.textContent).toContain("Force stop offer expired");
  expect(cleanup().rows[0].status).toBe("unknown"); expect(button(host, "Force stop…")).toBeUndefined();
  expect(native.confirm).not.toHaveBeenCalled(); expect(native.clear).not.toHaveBeenCalled();
});

it("keeps the pending raw Force ID beside an expired unknown member of the same folder", async () => {
  const expired = row({ id: "old-unknown", folder_key: "shared-folder", status: "unknown", can_force: false,
    reason: "Original removal remains unconfirmed.", program_retirement: "expired" });
  const pending = row({ id: "current-original", folder_key: "shared-folder", can_force: true, can_dismiss: false });
  applyCleanup({ ...emptyCleanup(), revision: ++revision, unknown: 1, rows: [expired, pending] });
  const host = mount(); review(host).click();
  expect(host.querySelectorAll("tbody tr")).toHaveLength(1);
  expect(host.querySelector('[data-outcome="unknown"]')?.textContent).toContain("Original removal remains unconfirmed.");
  expect(host.textContent).toContain("Force stop offer expired");
  button(host, "Force stop…").click(); await flush();
  expect(native.offer).toHaveBeenCalledExactlyOnceWith("current-original");
  expect(cleanup().rows[0]).toBe(expired); expect(cleanup().rows[1]).toBe(pending);
  expect(native.confirm).not.toHaveBeenCalled(); expect(native.dismiss).not.toHaveBeenCalled();
  button(host, "Cancel").click(); await flush();
  expect(native.cancel).toHaveBeenCalledExactlyOnceWith("current-original", "native-handle");
});

it("opens manual removal only through the original native row handle",async()=>{
 native.manual.mockResolvedValue({confirmation:"native-manual",path:"/srv/example/hand-made",computer:"fixture",branch:"kept",size:"0 B",changes:0,untracked:0,ignored:[],required:{local_files:false,discard_changes:false,unchecked_processes:false,merged_branches:false},ready:false,reason:"Current session still uses it",sessions:[],processes:[]});
 applyCleanup({...emptyCleanup(),revision:++revision,rows:[row({id:"unowned-original",can_force:false,can_remove_anyway:true,status:"review"})],kept:1});
 const host=mount();review(host).click();button(host,"Remove anyway…").click();await flush();
 expect(native.manual).toHaveBeenCalledExactlyOnceWith("unowned-original");expect(host.textContent).toContain("/srv/example/hand-made");
 expect(button(host,"Remove working folder").disabled).toBe(true);button(host,"Cancel").click();await flush();expect(native.manualCancel).toHaveBeenCalledExactlyOnceWith("native-manual");
});
