// Real dialog, synthetic read-only status replies and no filesystem effects.
import { render } from "solid-js/web";
import { applyMachines } from "../store/machines";
import { emptyBatch, type MachineMeta, type Project } from "../types";
import type { CloneView } from "../ui/screens/ProjectCloneDialog";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/screens.css";
const failures: string[] = [], calls: string[] = [];
const check = (value: unknown, message: string) => { if (!value) failures.push(message); };
const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));
async function until(test: () => unknown) { const end = performance.now() + 10_000; while (!test()) { if (performance.now() >= end) throw Error("reattached dialog did not settle"); await sleep(20); } }
const report = (value: unknown) => (window as unknown as { webkit: { messageHandlers: { result: { postMessage(text: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify(value));
const initial: CloneView = { id: "synthetic-clone", url: "https://example.org/repo.git", parent: "/srv/example", name: "repo", destination: "/srv/example/repo", status: "Checking destination…", busy: true, cloning: false, uncertain: false, ready: false, locked: true, can_submit: false };
let settled = false;
Object.assign(window, { __TAURI_INTERNALS__: { invoke: async (command: string) => {
  calls.push(command);
  if (command === "project_clone_open") return initial;
  if (command === "project_clone_status") return settled ? { ...initial, busy: false, locked: false, url: "", name: "", status: "" } : initial;
  if (command === "project_clone_step") return { ...initial, busy: false, uncertain: true };
  if (command === "project_clone_close") return;
  throw Error(`Unexpected synthetic IPC: ${command}`);
} } });
async function run() {
  const batch = emptyBatch(); batch.machines = [{ computer: "", machine: { host: "workstation", machine_id: "11111111-1111-4111-8111-111111111111", platform: "linux", ok: true } as MachineMeta }]; applyMachines(batch);
  const { default: ProjectCloneDialog } = await import("../ui/screens/ProjectCloneDialog");
  const project: Project = { id: "p", name: "Example", color: null, accessible: true, is_default: true, palette: 0, folders: [] };
  let dispose = () => {};
  dispose = render(() => <ProjectCloneDialog project="p" projects={[project]} origin={document.getElementById("origin")!} close={() => dispose()} placed={() => { throw Error("no placement is authorized by this fixture"); }} />, document.getElementById("root")!);
  try {
    const button = (name: string) => [...document.querySelectorAll<HTMLButtonElement>("dialog button")].find((button) => button.textContent?.trim() === name);
    await until(() => button("Continue")); button("Continue")!.click();
    const input = () => document.querySelector<HTMLInputElement>("#cl-url");
    await until(() => input()?.disabled);
    await until(() => calls.includes("project_clone_status"));
    check(!calls.includes("project_clone_step"), "reattachment automatically dispatched a clone step");
    settled = true; await until(() => input() && !input()!.disabled);
    check(input()!.value === initial.url, "settling the old check overwrote the reopened form");
    const reads = calls.filter((command) => command === "project_clone_status").length;
    await sleep(400); check(calls.filter((command) => command === "project_clone_status").length === reads, "idle reattachment kept polling");
    check(button("Clone repository") && !button("Clone repository")!.disabled, "the reopened form stayed blocked");
    button("Clone repository")!.click(); await until(() => calls.includes("project_clone_step"));
    await sleep(250); check(calls.filter((command) => command === "project_clone_step").length === 1, "one explicit click dispatched more than once");
    button("Cancel")!.click(); await until(() => !document.querySelector("dialog"));
    await sleep(50); check(calls.filter((command) => command === "project_clone_close").length === 1, "explicit Cancel was sent again during unmount");
  } finally { dispose(); }
  report({ failures, cases: ["busy reattachment follows status", "idle unlock preserves form", "only explicit clone dispatch"] });
}
void run().catch((error) => report({ failures: [String(error)] }));
