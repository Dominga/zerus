// Real component in a private browser fixture; callbacks never touch files.
import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import { LeftoverPanel } from "../ui/screens/WorktreeLeftover";
import type { OverviewRow } from "../ui/screens/worktree-overview-state";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/screens.css";

const failures: string[] = [];
const check = (value: unknown, message: string) => { if (!value) failures.push(message); };
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
const report = (value: unknown) => (window as unknown as { webkit: { messageHandlers: { result: { postMessage(text: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify(value));
const row: OverviewRow = {
  id: "opaque-leftover", path: "/srv/example/leftover", branch: "feature/kept", kind: "leftover", machine: "fixture-mac", computer: "", category: 5,
  state: "Partially removed", remains: "", size: "4 KiB", fresh: true, sessions: [],
  leftover: { branch: "feature/kept", branch_retained: true, bytes: 4096, complete: true, stopped: "", interrupted: false, reasons: ["Deleting leftover files is disabled on macOS. Open the folder in Finder."], sessions: [], processes: 0, fingerprint: "ab".repeat(32), consent: false, deletable: false, macos_deletion_blocked: true, repository: true, reserved_at: null, outcome: null },
};
async function run() {
  const [current, setCurrent] = createSignal(row);
  let opened = 0, kept = 0, deleted = 0;
  const root = document.getElementById("root")!;
  const dispose = render(() => <LeftoverPanel row={current()} busy={false} open={() => opened++} keep={() => kept++} remove={() => deleted++} review={() => {}} />, root);
  const button = (name: string) => [...root.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === name);
  await frame();
  const status = root.querySelector<HTMLElement>('[role="status"]');
  check(status && status.offsetHeight > 0 && status.textContent?.includes("AppleDouble") && status.textContent.includes("Finder on fixture-mac"), "the visible status lost its specific reason/manual next step");
  check(!button("Delete remaining files…"), "a blocked Mac offered deletion");
  check(button("Keep folder…") && !button("Keep folder…")!.disabled, "Keep folder became unavailable");
  check(button("Open in Finder") && !button("Open in Finder")!.disabled, "the local Finder action is unavailable");
  button("Open in Finder")?.click(); button("Keep folder…")?.click();
  check(opened === 1 && kept === 1 && deleted === 0, "the manual actions invoked deletion or lost their callbacks");
  setCurrent({ ...row, computer: "remote-mac" }); await frame();
  check(!button("Open in Finder") && root.textContent?.includes("Finder on fixture-mac"), "a remote folder was offered to the local opener");
  setCurrent({ ...row, leftover: { ...row.leftover!, reasons: [], deletable: true, macos_deletion_blocked: false } }); await frame();
  check(!root.querySelector('[role="status"]') && !button("Open in Finder"), "the Mac-only restriction leaked into Linux");
  check(button("Delete remaining files…") && !button("Delete remaining files…")!.disabled, "Linux deletion changed");
  button("Delete remaining files…")?.click(); check(deleted === 1, "Linux delete callback stopped working");
  dispose(); report({ failures, cases: ["local Mac refusal and manual actions", "remote Mac guidance", "Linux unchanged"] });
}
void run().catch((error) => report({ failures: [String(error)] }));
