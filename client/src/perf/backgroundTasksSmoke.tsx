// Actual Solid/WebKit Background tasks item, synthetic native rows only. Serve Vite;
// scripts/menu-smoke.py --title "Zerus background tasks regression (synthetic)" \
//   --url http://127.0.0.1:<port>/scripts/background-tasks-smoke.html
// runs in a private Xvfb. No CLI, agent, native termination or removal is invoked.
// ?hold=<ms> keeps the final open list on screen for a capture before reporting.
import { render } from "solid-js/web";
import { BackgroundTasksChip } from "../ui/screens/BackgroundTasks";
import { CleanupChip, CleanupReviewButton } from "../ui/screens/CleanupChip";
import { MachinesStatus } from "../ui/components/MachinesStatus";
import { applyBatch } from "../store/catalog";
import { applyCleanup, cleanupDialog, closeCleanup, emptyCleanup, type CleanupRow, type CleanupView } from "../store/terminationCleanup";
import { applySkin, skins } from "../skin/runtime";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/detail.css";
import "../ui/styles/screens.css";
import "../ui/styles/sessions.css";
import "../ui/styles/motion.css";

const failures: string[] = [];
const check = (condition: unknown, message: string) => { if (!condition) failures.push(message); };
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
const settle = async () => { await frame(); await frame(); };
let revision = 0;
const row = (id: string, over: Partial<CleanupRow> = {}): CleanupRow => ({ id, label: id, path: `/srv/example/${id}`, machine: "this computer", computer: "", branch: id,
  size: "1 KiB", status: "checking", reason: "", details: "", can_check: false, ready: false, busy: true, activity: "checking", ...over });
const show = async (rows: CleanupRow[], over: Partial<CleanupView> = {}) => {
  const running = rows.some((r) => r.busy);
  applyCleanup({ ...emptyCleanup(), revision: ++revision, rows, busy: running, cleaning: running, ...over });
  await settle();
};
const bar = () => document.querySelector<HTMLElement>(".statusbar")!;
const chip = () => bar().querySelector<HTMLButtonElement>(".bg-tasks-chip");
const result = () => bar().querySelector<HTMLButtonElement>(".cleanup-result");
const review = () => document.querySelector<HTMLButtonElement>(".sp-foot .cleanup-review")!;
const workButtons = () => bar().querySelectorAll(".bg-tasks-chip, .cleanup-result");
const list = () => document.querySelector<HTMLElement>(".menu.bg-tasks");
const open = async () => { chip()!.click(); await settle(); };
const press = async (key: string) => { (document.activeElement ?? document.body).dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true })); await settle(); };
const pointer = async (target: Element) => { target.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true })); await settle(); };

async function walk() {
  const requestedSkin = new URLSearchParams(location.search).get("skin");
  applySkin(skins.find((s) => s.appearance === requestedSkin) ?? skins[0]);
  applyBatch({ upserts: [], removes: [], replaces: [], activity: [], computers: [{ computer: "", status: "live", attempt: 0 }] });
  render(() => <><footer class="sp-foot" style={{ position: "fixed", inset: "auto auto 28px 72px", width: "320px" }}><span class="grow" /><CleanupReviewButton /></footer>
    <footer class="statusbar" style={{ position: "fixed", inset: "auto 0 0 72px" }}><MachinesStatus mode={() => "replay"} /><BackgroundTasksChip /><CleanupChip /><span class="grow" /></footer></>, document.getElementById("root")!);
  await settle();
  check(workButtons().length === 0, "an empty store showed a work-result chip");
  check(review() && !review().classList.contains("attention") && !review().querySelector(".worktree-review-count"), "idle footer review is not calm and available");
  const idleColor = getComputedStyle(review()).color;

  // Only running work: one item, no cleanup chip repeating it.
  await show([row("first"), row("second", { status: "removing", activity: "removing" })], { text: "Cleaning working folders…" });
  check(chip()?.textContent === "Background tasks 2", `count: ${chip()?.textContent}`);
  check(workButtons().length === 1, "the cleanup chip repeated running work");
  check(chip()!.querySelector(".su-spin"), "the running item has no spinner");
  check(chip()!.getBoundingClientRect().height === 20, "the chip left the status bar's chip height");

  // Its list: above the chip, inside the window, on the first row's only action.
  await open();
  const opened = list()!;
  check(opened, "the list did not open");
  check(chip()!.getAttribute("aria-expanded") === "true", "aria-expanded did not follow the list");
  const box = opened.getBoundingClientRect(), anchor = chip()!.getBoundingClientRect();
  check(box.bottom <= anchor.top, `the list covers the chip: ${box.bottom} > ${anchor.top}`);
  check(box.left >= 8 && box.right <= innerWidth - 8, `the list left the window: ${box.left}..${box.right}`);
  const texts = [...opened.querySelectorAll(".bg-task")].map((task) => [...task.children].map((part) => part.textContent));
  check(JSON.stringify(texts) === JSON.stringify([["first", "Checking…", "Review…"], ["second", "Checking again and removing…", "Review…"]]), `rows: ${JSON.stringify(texts)}`);
  check([...opened.querySelectorAll("button")].every((button) => button.textContent === "Review…"), "the list added an operation");
  const first = opened.querySelector("button")!;
  check(document.activeElement === first, "focus did not move into the list");

  // A store update keeps the list, the focused button and the placement.
  await show([row("first", { status: "removing", activity: "removing" }), row("second")]);
  check(list() === opened && document.activeElement === first, "an update rebuilt the list");
  check(opened.querySelector(".bg-task .muted")?.textContent === "Checking again and removing…", "the open list did not follow the state");

  // The ways out.
  await press("Escape");
  check(!list() && document.activeElement === chip(), "Escape did not close the list and return focus");
  check(chip()!.getAttribute("aria-expanded") === "false", "aria-expanded stayed open");
  await open();
  await pointer(document.body);
  check(!list(), "an outside press did not close the list");
  await open();
  await pointer(chip()!); check(list(), "pressing the chip closed the list before its click");
  chip()!.click(); await settle(); check(!list(), "the chip did not toggle the list closed");

  // Review opens the existing dialog on that row, with focus returning to the chip.
  await open();
  list()!.querySelectorAll("button")[1].click(); await settle();
  check(!list(), "Review left the list open");
  check(cleanupDialog()?.selected === "second", `Review selected ${cleanupDialog()?.selected}`);
  check(cleanupDialog()?.origin === chip(), "the dialog will not return focus to the chip");
  closeCleanup();

  // Work that ends under the open list takes it away.
  await open();
  await show([]);
  check(!list() && !chip(), "the item outlived the work");
  await press("Escape"); await pointer(document.body);

  // Running work beside held results: both stay.
  const kept = row("kept", { status: "kept", busy: false, activity: null });
  await show([row("running"), kept], { kept: 1, text: "Folder kept: kept.", title: "Folder kept: kept." });
  check(chip()?.textContent === "Background tasks 1", `mixed count: ${chip()?.textContent}`);
  check(result()?.textContent === "Folder kept: kept.", `mixed result: ${result()?.textContent}`);
  check(review().classList.contains("warn") && review().textContent === "1", "the footer did not count the held folder");
  check(getComputedStyle(review()).color !== idleColor, "the held-folder count did not change the footer icon colour");
  check(review().getBoundingClientRect().width >= 26 && review().scrollWidth <= review().clientWidth, "the footer count clipped or overlapped its icon");
  check(result()!.getBoundingClientRect().right <= bar().getBoundingClientRect().right, "the status bar clipped the held result");
  await show([row("running")], { unknown: 1, text: "Working folders: 1 unknown" });
  check(result()?.textContent === "Working folders: 1 unknown", "running work hid a result past the row cap");

  // The running item goes; the result and its review stay.
  await show([kept], { kept: 1, text: "Folder kept: kept.", title: "Folder kept: kept." });
  check(!chip(), "the item stayed beside finished results");
  result()!.click(); await settle();
  check(cleanupDialog(), "the held result lost its review"); closeCleanup();

  // The largest list stays bounded, and a long name doesn't widen it past its limit.
  const name = "a-very-long-generated-working-folder-name-that-keeps-going-".repeat(4);
  await show(Array.from({ length: 64 }, (_, i) => row(`${name}${i}`)));
  await open();
  const full = list()!, fullBox = full.getBoundingClientRect();
  check(fullBox.height <= 320 && full.scrollHeight > full.clientHeight, `64 rows are not bounded: ${fullBox.height}`);
  check(fullBox.right <= innerWidth - 8 && fullBox.width <= 560, `a long name widened the list: ${fullBox.width}`);
  check(full.scrollWidth <= full.clientWidth, "a long name made the list scroll sideways");
  await press("Escape");

  // Left on screen for a capture when asked.
  const hold = Number(new URLSearchParams(location.search).get("hold") ?? 0);
  if (hold) {
    const skin = new URLSearchParams(location.search).get("skin");
    if (skin) applySkin(skins.find((s) => s.appearance === skin) ?? skins[0]);
    await show([row("feature-login-rework", { status: "removing", activity: "removing" }), row("fix-activity-lines"), row("restored-session", { status: "kept", activity: "restoring" }), kept], { kept: 2, text: "Working folders: 2 kept", title: "Working folders: 2 kept" });
    await open();
    await new Promise((resolve) => setTimeout(resolve, hold));
  }
}
const report = () => (window as unknown as { webkit: { messageHandlers: { result: { postMessage(text: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify({ failures }));
void walk().catch((error) => { failures.push(String(error)); }).finally(report);
