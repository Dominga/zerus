// Task 200: New team as Qt's TeamNewTab shows it, beside its Qt shots: the
// template list with search and favourites, the lead pane (Chat, Goal, Plan),
// the project memory modes when the selected session's project has its
// memory on, and a draft opened again with the lead's plan.
// Synthetic: no CLI; the IPC answered is the settings read, the folder check,
// the project's memory switch and an account's models.
//   http://127.0.0.1:<port>/scripts/new-team-smoke.html?case=new|project|draft[&tab=chat|goal|plan][&skin=...]
const query = new URLSearchParams(location.search);
const native = window as unknown as { __TAURI_INTERNALS__?: unknown };
native.__TAURI_INTERNALS__ = {
  transformCallback: () => 1,
  convertFileSrc: (path: string) => path,
  invoke: async (command: string) => {
    if (command === "settings_get") return { values: {}, revision: 0 };
    if (command.startsWith("plugin:event|")) return 1;
    if (command === "local_folder_probe") return { exists: true, git: query.get("git") !== "0" };
    if (command === "project_memory") return { outcome: "done", view: { enabled: query.get("case") === "project" } };
    if (command === "team_role_models") return [{ id: "opus", label: "Opus", effort_options: [] }, { id: "sonnet", label: "Sonnet", effort_options: [] }];
    if (command === "teams_follow") return null;
    throw new Error(`synthetic smoke refuses ${command}`);
  },
};

import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/sessions.css";
import "../ui/styles/detail.css";
import "../ui/styles/teams.css";
import "../ui/styles/newteam.css";
import "../ui/styles/client.css";
import { render } from "solid-js/web";
import { Show } from "solid-js";
import { setTeamHelpSystemLanguage, teamHelpOpen } from "../store/teamHelp";
import { TeamHelpDialog } from "../ui/teams/TeamHelpDialog";
import { setSystemLocale } from "../ui/sessions/usage-number";
import { NewTeam } from "../ui/teams/NewTeam";
import { createNewTeam } from "../store/teamsNew";
import { catalogOf } from "../store/newTeamSources";
import { setSetting } from "../store/settings";
import { applySkin, skin } from "../skin/runtime";
import type { NewTeamView } from "../ui/teams/types";

const catalog = catalogOf({
  version: 3,
  roles: [
    { id: "lead", name: "Lead", agent: "claude", model: "opus" },
    { id: "backend", name: "Backend engineer", agent: "codex", model: "gpt-5.5-codex" },
    { id: "frontend", name: "Frontend engineer", agent: "claude", model: "sonnet" },
    { id: "qa", name: "Tester", agent: "claude", model: "sonnet" },
  ],
  templates: [
    { id: "feature", name: "Feature team", description: "Backend, frontend and a tester for one feature", roster: [{ role: "backend", count: 1 }, { role: "frontend", count: 1 }, { role: "qa", count: 1 }], tasks: [{ title: "API" }, { title: "UI" }] },
    { id: "audit", name: "Code audit", description: "Two reviewers read the code", roster: [{ role: "qa", count: 2 }] },
    { id: "bugfix", name: "Bug fix", description: "One engineer and a tester", roster: [{ role: "backend", count: 1 }, { role: "qa", count: 1 }] },
    { id: "spike", name: "Research spike", description: "One engineer explores", roster: [{ role: "backend", count: 1 }] },
  ],
});
const accounts = [{ id: "work", label: "Work", provider: "claude", signedIn: true }, { id: "home", label: "Home", provider: "codex", signedIn: false }];
const studio = [{ id: "studio-max", label: "Studio Max", provider: "claude", signedIn: true }];
const computers = [{ alias: "", label: "this computer", online: true }, { alias: "studio", label: "Studio", online: true }];

const draftView: NewTeamView = {
  team: "t-orbit", name: "Saved cards", goal: "Customers save a card at checkout and pick it next time. Done: the API, the form and contract tests pass on staging.",
  addendum: "", folder: "D:/work/orbit", settings: { memory: { mode: "project" } },
  members: [
    { id: "lead", role: "Lead", agent: "claude", model: "opus", effort: "", account: "", permissions: "", computer: "", folder: "", prompt: "", lead: true,
      session: { host: "", name: "claude/orbit/saved-cards-lead", run_id: "r1", conversation_id: "c1" } },
    { id: "backend", role: "Backend engineer", agent: "codex", model: "gpt-5.5-codex", effort: "high", account: "", permissions: "", computer: "studio", folder: "worktree", prompt: "", lead: false, session: null },
    { id: "frontend", role: "Frontend engineer", agent: "claude", model: "sonnet", effort: "", account: "work", permissions: "", computer: "", folder: "worktree", prompt: "", lead: false, session: null },
  ],
  tasks: [
    { id: 1, title: "Saved cards API", status: "planned", assignee: "backend", phase: 1 },
    { id: 2, title: "Card form in checkout", status: "planned", assignee: "frontend", phase: 1 },
    { id: 3, title: "Contract tests on staging", status: "planned", assignee: "backend", phase: 2 },
  ] as unknown as NewTeamView["tasks"],
  draft: { revision: 4, unseen: query.get("tab") === "plan" ? ["tasks"] : [], owner_edited: [] },
  validation: { errors: [], warnings: [] },
  updated: 0,
};

async function run() {
  await document.fonts.ready;
  setTeamHelpSystemLanguage(query.get("language") ?? "en-US");
  setSystemLocale(query.get("region") ?? "en-US");
  applySkin(skin());
  void setSetting("teams/templateFavourites", ["audit"]);
  const kind = query.get("case") ?? "new";
  const store = createNewTeam({ action: async () => ({ outcome: "refused", detail: "synthetic", request_id: "" } as never) });
  store.openNew();
  if (kind === "draft") { store.openDraft(draftView.team); store.applyView(draftView); }
  else store.setFolder("D:/work/orbit");
  render(() => (
    <div style={{ height: "100vh", display: "flex", "flex-direction": "column", background: "var(--z-color-page)" }}>
      <NewTeam store={store} computer="" computers={computers} accounts={accounts} accountsFor={(computer) => (computer === "studio" ? studio : accounts)}
        catalog={catalog} git={query.get("git") !== "0"} folderOk coordinator={{ name: "claude/orbit/checkout", agent: "claude", model: "opus" }}
        project={kind === "project" ? "Orbit" : undefined} projectId={kind === "project" ? "p-orbit" : undefined}
        onOpenSettings={() => {}} onAskDesigner={() => {}} />
      <Show when={teamHelpOpen()}><TeamHelpDialog /></Show>
    </div>
  ), document.getElementById("root")!);
  await new Promise((done) => setTimeout(done, 400));
  const tab = query.get("tab");
  if (tab) document.querySelector<HTMLElement>(`[data-lead-page="${tab}"]`)?.click();
  const search = query.get("search");
  if (search) {
    const field = document.querySelector<HTMLInputElement>(".nt-template-search")!;
    field.value = search;
    field.dispatchEvent(new InputEvent("input", { bubbles: true }));
  }
  if (query.get("pick")) document.querySelector<HTMLElement>(`[data-template="${query.get("pick")}"]`)?.click();
  await new Promise((done) => setTimeout(done, 500));
  document.title = `${document.title}: ready`;
}
void run().catch((error: Error) => { document.title = `failed: ${String(error)} ${(error.stack ?? "").split("\n").slice(1, 3).join(" | ")}`; });
