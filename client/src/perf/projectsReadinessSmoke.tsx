// Real Projects screen and synthetic metadata; all mutation IPC is refused.
// With Vite running: python3 scripts/menu-smoke.py --accessibility
//   --url http://127.0.0.1:1420/scripts/projects-readiness-smoke.html
//   --title "Zerus Projects readiness regression (synthetic)"
import { render } from "solid-js/web";
import { setProjectsPage } from "../store/projects";
import { emptyBatch, emptyProjects, type Project } from "../types";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/screens.css";

const failures: string[] = [], calls: string[] = [];
const allowed = new Set(["pending_read_marks", "attention_marks", "cancel_content_search", "settings_get", "new_session_context"]);
const check = (value: unknown, message: string) => { if (!value) failures.push(message); };
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
const report = (value: unknown) => (window as unknown as { webkit: { messageHandlers: { result: { postMessage(text: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify(value));
Object.assign(window, { __TAURI_INTERNALS__: { invoke: async (command: string) => {
  calls.push(command);
  if (command === "pending_read_marks") return [];
  if (command === "attention_marks") return { marks: {}, replies: {} };
  if (command === "cancel_content_search") return null;
  if (command === "settings_get") return { values: {}, revision: 0 };
  if (command === "new_session_context") return { branch: "work-example", own_windows_session: null, build_style: "posix", remembered: { agent: "claude", accounts: {}, open_terminal: false }, swarm: "" };
  failures.push(`Unexpected synthetic IPC: ${command}`); throw Error(`Unexpected synthetic IPC: ${command}`);
} } });

async function run() {
  const organization = await import("../store/organization");
  const projects: Project[] = ["Example", "Other"].map((name, index) => ({ id: name.toLowerCase(), name, color: null, accessible: true, is_default: index === 1, palette: index, folders: [] }));
  const change = (ready: boolean, reason: string | null) => {
    const batch = emptyBatch();
    batch.grouping = { ...organization.emptyGrouping(), loaded: true, default_project: "other", headers: projects.map((p) => ({ ...p, collapsed: false })), ready, reason };
    organization.applyGrouping(batch);
  };
  setProjectsPage({ ...emptyProjects(), loaded: true, projects, status: "2 synthetic projects" });
  change(false, "Synthetic project sync is reconnecting.");
  const { ProjectsScreen } = await import("../ui/screens/Projects");
  const dispose = render(() => <ProjectsScreen />, document.getElementById("root")!);
  try {
    await frame();
    const button = (text: string) => [...document.querySelectorAll<HTMLButtonElement>(".projects button")].find((row) => row.textContent?.trim() === text)!;
    const name = () => document.querySelector<HTMLInputElement>("#proj-name")!;
    const defaultProject = () => document.querySelector<HTMLInputElement>(".proj-default input")!;
    const targets = () => [name(), defaultProject(), ...["New project…", "Delete project…", "Worktrees…", "Clone repository…", "Add folder…", "Edit folder…", "Remove folder"].map(button)];
    const status = () => document.getElementById("project-edit-readiness");
    const blocked = (reason: string) => {
      check(status()?.textContent === reason && status()?.getAttribute("role") === "status", "readiness reason is in the live status line");
      check(document.querySelector(".proj-swarm > span")?.textContent === "2 synthetic projects", "readiness reason preserves the original project status");
      check(targets().every((target) => target.getAttribute("aria-describedby") === "project-edit-readiness"), "readiness controls describe the current reason");
      check(name().readOnly && defaultProject().disabled && button("New project…").disabled && button("Delete project…").disabled, "original edit guards remain blocked");
      button("New project…").click(); button("Delete project…").click(); defaultProject().click();
      check(!document.querySelector("dialog"), "blocked activation opens no writer dialog");
    };
    blocked("Synthetic project sync is reconnecting.");
    change(false, null); await frame(); blocked("Project sync is starting.");
    change(true, "Stale readiness reason must be ignored."); await frame();
    check(!status() && document.querySelector(".proj-swarm > span")?.textContent === "2 synthetic projects", "ready status keeps the original project summary without a stale reason");
    check(targets().every((target) => !target.hasAttribute("aria-describedby")), "ready controls retain no stale readiness description");
    check(!name().readOnly && !defaultProject().disabled && !button("New project…").disabled && !button("Delete project…").disabled, "ready controls preserve their normal enablement");
    const reason = "Synthetic project catalog is waiting for a verified source.";
    change(false, reason); await frame(); blocked(reason);
    // Remove every hover title so AT-SPI must use the programmatic association.
    document.querySelectorAll(".projects [title]").forEach((element) => element.removeAttribute("title"));
    check(calls.every((command) => allowed.has(command)), "status transitions submit no metadata mutation");
    report({ failures, accessibility_descriptions: ["New project…", "Delete project…", "Default for sessions started outside Zerus"].map((name) => ({ name, description: reason })), cases: ["live readiness reason", "fallback and updated reasons", "ready clears descriptions", "unchanged guarded actions", "native descriptions without titles"] });
  } catch (error) { dispose(); throw error; }
}
void run().catch((error) => report({ failures: [String(error)] }));
