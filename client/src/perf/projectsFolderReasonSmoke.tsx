// Real Projects screen, synthetic computers/folders, no launches or writes.
// With Vite running: python3 scripts/menu-smoke.py --accessibility
//   --url http://127.0.0.1:1420/scripts/projects-folder-reason-smoke.html
//   --title "Zerus Projects folder reason regression (synthetic)"
import { render } from "solid-js/web";
import { setProjectsPage } from "../store/projects";
import { emptyBatch, emptyProjects, type Folder, type MachineMeta, type Project } from "../types";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/screens.css";

const failures: string[] = [], calls: string[] = [];
const allowed = new Set(["pending_read_marks", "attention_marks", "cancel_content_search", "settings_get", "new_session_context", "worktree_snapshot", "worktree_catalog"]);
const check = (value: unknown, message: string) => { if (!value) failures.push(message); };
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
const report = (value: unknown) => (window as unknown as { webkit: { messageHandlers: { result: { postMessage(text: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify(value));
Object.assign(window, { __TAURI_INTERNALS__: { invoke: async (command: string, args: { computer?: string; path?: string } = {}) => {
  calls.push(command);
  if (command === "pending_read_marks") return [];
  if (command === "attention_marks") return { marks: {}, replies: {} };
  if (command === "cancel_content_search") return null;
  if (command === "settings_get") return { values: {}, revision: 0 };
  if (command === "new_session_context") return { branch: "work-example", own_windows_session: null, build_style: "posix", remembered: { agent: "claude", accounts: {}, open_terminal: false }, swarm: "" };
  if (["worktree_snapshot", "worktree_catalog"].includes(command) && args.computer === "fixture-peer" && args.path === "/srv/example") return {};
  failures.push(`Unexpected synthetic IPC: ${command}`); throw Error(`Unexpected synthetic IPC: ${command}`);
} } });
async function run() {
  const organization = await import("../store/organization");
  const { applyBatch } = await import("../store/catalog");
  const project: Project = { id: "example", name: "Example", color: null, accessible: true, is_default: true, palette: 0, folders: [] };
  const batch = emptyBatch();
  batch.grouping = { ...organization.emptyGrouping(), loaded: true, default_project: project.id, headers: [{ ...project, collapsed: false }], reason: "Unrelated synthetic project-sync refusal." };
  organization.applyGrouping(batch);
  const computer = (live: boolean) => {
    const next = emptyBatch();
    next.computers = [{ computer: "", status: "live", attempt: 0 }, { computer: "fixture-peer", status: live ? "live" : "reconnecting", attempt: 1 }];
    next.machines = ["", "fixture-peer"].map((computer) => ({ computer, machine: { host: computer || "fixture-local", platform: "linux", ok: true, local_sessions: true } as MachineMeta }));
    applyBatch(next);
  };
  const folders = (rows: Folder[]) => setProjectsPage({ ...emptyProjects(), loaded: true, projects: [{ ...project, folders: rows }], status: "1 synthetic project" });
  computer(false); folders([]);
  const { ProjectsScreen } = await import("../ui/screens/Projects");
  const dispose = render(() => <ProjectsScreen />, document.getElementById("root")!);
  try {
    await frame();
    const button = (text: string) => [...document.querySelectorAll<HTMLButtonElement>(".projects button")].find((row) => row.textContent?.trim() === text)!;
    const launch = button("New session…"), hint = () => document.getElementById("project-folder-launch-reason");
    const geometry = () => { const { x, y, width, height } = launch.getBoundingClientRect(); return { x, y, width, height }; };
    const original = geometry();
    const blocked = (reason: string) => {
      check(launch.disabled && hint()?.textContent === reason && hint()?.getAttribute("role") === "status", "folder reason is visible with the blocked launch");
      check(launch.getAttribute("aria-describedby") === "project-folder-launch-reason", "launch describes its own availability reason");
      check(JSON.stringify(geometry()) === JSON.stringify(original), "reason changes do not move or resize the launch button");
      launch.click(); check(!document.querySelector("dialog"), "blocked launch opens no dialog");
    };
    blocked("Select a folder for the new session.");
    folders([{ id: "folder", key: "folder", name: "Example folder", path: "/srv/example", computer: "fixture-peer", machine: "fixture-peer" }]);
    await frame(); blocked("Reconnect fixture-peer to start a session in this folder.");
    computer(true); await frame();
    check(!launch.disabled && !launch.hasAttribute("aria-describedby") && hint()?.textContent === "", "available folder clears the launch reason and association");
    check(button("New project…").disabled, "folder availability does not bypass project-sync edit guards");
    check(JSON.stringify(geometry()) === JSON.stringify(original), "available launch keeps its toolbar geometry");
    computer(false); await frame();
    const reason = "Reconnect fixture-peer to start a session in this folder.";
    blocked(reason);
    check(button("New session…") === launch, "availability updates keep the launch button mounted");
    document.querySelectorAll(".projects [title]").forEach((element) => element.removeAttribute("title"));
    check(calls.every((command) => allowed.has(command)), "availability updates submit no launch or mutation IPC");
    report({ failures, launch_geometry: geometry(), accessibility_descriptions: [{ name: "New session…", description: reason }], cases: ["missing and offline folders", "available clears hint", "separate edit guard", "stable toolbar", "native description without titles"] });
  } catch (error) { dispose(); throw error; }
}
void run().catch((error) => report({ failures: [String(error)] }));
