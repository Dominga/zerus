// Keep the shared layer order first, just as main.tsx does.
import "../ui/styles/base.css";
import "../ui/styles/tasks.css";
import "../ui/styles/sessions.css";
import { For, Show, createComponent, onMount, type JSX } from "solid-js";
import { render } from "solid-js/web";
import { NewTeam } from "../ui/teams/NewTeam";
import { NewTeamPage } from "../ui/teams/NewTeamPage";
import { TeamUncertain, TeamWorkspace } from "../ui/teams/TeamTab";
import { WaitingList } from "../ui/teams/Waiting";
import type { WaitingView } from "../store/teams";
import { Icon } from "../ui/lib/icons";
import { TeamConfirmation } from "../ui/teams/TeamConfirmDialog";
import { Dropdown } from "../ui/lib/Dropdown";
import { TeamChip } from "../ui/teams/TeamChips";
import { createSignal } from "solid-js";
import { MenuLayer } from "../ui/components/Overlays";
import { MemberCard } from "../ui/teams/MemberCard";
import { catalog, teamView, fidelityView } from "../ui/teams/fixtures";
import type { TeamView } from "../store/teams";
import { applySkin, skins } from "../skin/runtime";
import { TeamsSection } from "../ui/screens/settings/Teams";
import { teams } from "../store/teams";

// Fixture-only views: no store follows, IPC or desktop application. Query:
// ?skin=zerus-dark|zerus-light|fjord|paper&screen=team|new|states|waiting|send|settings&narrow=1
// screen=settings&confirm=1 opens the live-team preview-off confirmation.
// &detail=1 selects the review card; &ready=1 fills the name and checks the
// synthetic folder. Without screen, the page shows every fixture.
//   ./node_modules/.bin/vite --port 5181 --strictPort --host 127.0.0.1
//   http://127.0.0.1:5181/scripts/teams-smoke.html
const params = new URLSearchParams(location.search);
if (params.get("screen") === "settings") {
  void import("../ui/styles/client.css");
  void import("../ui/styles/settings.css");
}
const skin = skins.find((skin) => skin.id === params.get("skin")) ?? skins[0];
applySkin(skin);
document.documentElement.dataset.motion = "off";

const members = [
  teamView.members[0],
  teamView.members[1],
  { ...teamView.members[3], state: "paused" },
  { ...teamView.members[2], id: "tester", role: "tester", state: "not_created" },
  { ...teamView.members[2], id: "old", state: "removed" },
];
const full: TeamView = { ...teamView, members, next: "fixture-next-page", truncated: true };
const empty: TeamView = {
  ...teamView, members: members.slice(0, 2),
  groups: teamView.groups.map((group) => ({ ...group, cards: [] })),
  bar: { ...teamView.bar, counts: { ...teamView.bar.counts, nodes: { waiting: 0, pending: 0, in_progress: 0, reported: 0, accepted: 0, cancelled: 0 }, inbox: 0, cards: 0 } },
};
const TEAM_A = "00000000-0000-4000-8000-00000000aaaa";
const TEAM_B = "00000000-0000-4000-8000-00000000bbbb";
const waitingFixture: WaitingView = {
  items: [
    { id: `${TEAM_A}:3`, team: TEAM_A, card: 3, kind: "note", node: null, text: "Tell the team the release moved to Monday.", state: "open", by: "local", created_ms: Date.now() - 6 * 60_000 },
    { id: `${TEAM_B}:4`, team: TEAM_B, card: 4, kind: "integration", node: 2, text: "Accept the merge of Task 2 into the integration branch?", state: "open", by: "local", created_ms: Date.now() - 3 * 3_600_000, decisions: ["approve", "reject"] },
    { id: `${TEAM_A}:5`, team: TEAM_A, card: 5, kind: "task_waits_owner", node: 1, text: "Task 1 is reported; its work is decided on the team's board.", state: "open", by: "lead", created_ms: Date.now() - 2 * 86_400_000 },
  ],
  next: "fixture-waiting-next",
};
const teamNames = new Map([[TEAM_A, "Fixes"], [TEAM_B, "Docs"]]);
const previewCatalog = { ...catalog, templates: [...catalog.templates, {
  ...catalog.templates[0], id: "small", name: "Small", description: "One implementer and an independent checker.",
  roster: catalog.templates[0].roster.map((seat) => ({ ...seat, count: 1 })),
}] };

function Section(props: { title: string; note?: string; children: JSX.Element; form?: boolean }) {
  return <section class="smoke-section" data-smoke={props.title} classList={{ "smoke-form": props.form }}>
    <h2>{props.title}</h2>
    <Show when={props.note}><p class="dim">{props.note}</p></Show>
    <div class="smoke-frame">{props.children}</div>
  </section>;
}

function FormPreview() {
  let dialog: HTMLDialogElement | undefined;
  onMount(() => {
    dialog?.showModal();
    if (params.has("ready")) queueMicrotask(() => {
      const field = document.querySelector<HTMLInputElement>('input[placeholder="Fixes"]')!;
      field.value = "Fixes"; field.dispatchEvent(new Event("input", { bubbles: true }));
      document.querySelector<HTMLButtonElement>("[data-check-folder]")!.click();
    });
  });
  const form = () => <NewTeamPage onClose={() => {}}>{createComponent(NewTeam, {
    catalog: previewCatalog, busy: false, machineId: "00000000-0000-4000-8000-0000000000aa",
    defaultProject: "example-project", defaultFolder: "/workspace/example",
    onCheckFolder: async () => ({ git: true, head: "0123456789abcdef0123456789abcdef01234567", branch: "main", reason: "" }),
    onCreate: async () => ({ outcome: "done", receipt: {}, request_id: "fixture" }), onSaveTemplate: async () => {}, onClose: () => {},
  })}</NewTeamPage>;
  return params.get("screen") === "new"
    ? <dialog ref={dialog} class="dlg newteam-page-dialog" aria-labelledby="new-team-title">{form()}</dialog>
    : form();
}

function App() {
  const show = (screen: string) => !params.has("screen") || params.get("screen") === screen;
  onMount(() => {
    if (params.get("screen") === "settings" && params.has("confirm")) queueMicrotask(() => document.querySelector<HTMLButtonElement>("#set-teams-preview")?.click());
    if (params.has("stages")) queueMicrotask(() => {
      const board = document.querySelector<HTMLElement>(".team-board");
      if (board) board.scrollLeft = board.scrollWidth;
    });
    if (params.has("detail")) queueMicrotask(() => {
      const card = document.querySelector<HTMLButtonElement>('[data-group="review"] .team-task-card');
      card?.click();
      if (window.innerWidth <= 900) card?.scrollIntoView({ block: "nearest", inline: "nearest" });
      window.scrollTo(0, 0);
    });
  });
  if (params.get("screen") === "settings") {
    // One synthetic discovery page; no probe, follow or native invocation.
    teams.apply([{ computer: "local", key: "list", kind: "list", payload: { teams: [{ id: teamView.bar.id, project: "example-project", name: "Example team", status: "running", lead: "lead", home: "/workspace/example", revision: 1 }], truncated: false, next: null } }]);
    return <div class="smoke-board-viewport"><div class="set-page"><div class="set-shell">
      <nav class="set-nav" aria-label="Settings sections"><h1>Settings</h1><div class="set-nav-item on"><Icon name="users" size={15} /><span>Agent Teams</span></div></nav>
      <div class="set-scroll"><div class="set-content"><TeamsSection /></div></div>
    </div></div></div>;
  }
  if (params.get("screen") === "confirm") return <TeamConfirmation name="teamStopConfirm" title="Stop team?" text="Stop Orbit platform / checkout? Every unfinished task is cancelled." action="Stop team" onCancel={() => { document.body.dataset.confirmResult = "cancel"; }} onConfirm={() => { document.body.dataset.confirmResult = "confirm"; }} />;
  if (["board", "fallback"].includes(params.get("screen") ?? "")) {
    const view = params.get("screen") === "fallback" ? { ...fidelityView, members: fidelityView.members.map(({ model: _model, ...member }) => member), groups: fidelityView.groups.map((group) => ({ ...group, cards: group.cards.map(({ rounds: _rounds, kind: _kind, ...card }) => ({ ...card, stages: card.stages.map(({ kind: _stageKind, round: _round, target: _target, ...stage }) => stage) })) })) } : fidelityView;
    return <div class="smoke-board-viewport"><TeamWorkspace view={view} busy={false} failure={null} onAction={() => {}} onPage={() => {}} onAddTask={() => {}} /><MenuLayer /></div>;
  }
  return <div class="smoke" classList={{ "smoke-narrow": params.has("narrow") }}>
    <MenuLayer /><header class="smoke-head"><h1>Teams screens (M1)</h1><nav aria-label="Preview skins"><For each={skins}>{(choice) => {
      const query = new URLSearchParams(params); query.set("skin", choice.id);
      return <a class="btn small" aria-current={choice.id === skin.id ? "page" : undefined} href={`?${query}`}>{choice.name}</a>;
    }}</For></nav></header>
    <Show when={show("team")}>
      <Section title="Team tab: a full board with stages" note="Select a card for its facts and decisions. More uses a fixture cursor.">
        <TeamWorkspace view={full} busy={false} failure={null} onAction={() => {}} onPage={() => {}} onAddTask={() => {}} />
      </Section>
      <Section title="An uncertain action"><TeamUncertain detail="The action may have been applied. Check before doing anything else." busy={false} onCheck={() => {}} onDismiss={() => {}} /></Section>
      <Section title="Team tab: an empty queue and a read failure">
        <TeamWorkspace view={empty} busy={false} failure={{ code: "read_failed", message: "The computer is offline." }} onAction={() => {}} onPage={() => {}} onAddTask={() => {}} />
      </Section>
    </Show>
    <Show when={show("states")}>
      <Section title="Members in each state" note="active, paused, not created and removed, with the lead marked.">
        <div class="smoke-members"><For each={members}>{(member) => <MemberCard member={member} teamStatus="running" busy={false} onAction={() => {}} />}</For></div>
      </Section>
      <Section title="Teams off"><p class="smoke-off dim">With the preview off, the rail's Teams and Waiting entries, the team.new command and the Team tab have no entry point.</p></Section>
    </Show>
    <Show when={show("new")}>
      <Section title="New team" form note="The real dialog frame. Use ready=1 to check the synthetic Git folder."><FormPreview /></Section>
    </Show>
    <Show when={show("waiting")}>
      <Section title="Waiting for you: the owner cards" note="Oldest first, each with its team, kind, age and decisions; More uses a fixture cursor.">
        <WaitingList view={waitingFixture} teamNames={teamNames} now={Date.now()} busy={false}
          onDecide={() => {}} onOpenTeam={() => {}} onPage={() => {}} />
      </Section>
      <Section title="Waiting for you: the requests of team sessions" note="From the session attention data: only a question or an approval that asks for the person.">
        <div class="waiting-requests">
          <button type="button" class="waiting-request"><Icon name="shield-check" size={14} /><b>dev-1</b><span class="dim">Fixes</span><span class="waiting-request-caption">The member asks to run the test suite on the shared folder.</span></button>
          <button type="button" class="waiting-request"><Icon name="message-square" size={14} /><b>lead</b><span class="dim">Docs</span><span class="waiting-request-caption">Needs input on the branch name.</span></button>
        </div>
      </Section>
      <Section title="Waiting for you: nothing waits" note="The empty page and the read failure share the same notice.">
        <WaitingList view={{ items: [], next: null }} teamNames={teamNames} now={Date.now()} busy={false} onDecide={() => {}} onOpenTeam={() => {}} onPage={() => {}} />
        <div class="notice danger team-notice" role="alert"><Icon name="triangle-alert" size={16} /><span>Waiting could not be read: The computer is offline.</span></div>
      </Section>
    </Show>
    <Show when={show("send")}>
      <Section title="Send to team on a task card" note="A card in To do with the project's two teams: each is one menu item; the card stays where it was.">
        <div class="tk-panel">
          <div class="tk-head"><span class="tk-scope">1 task</span>{(() => { const [choice, setChoice] = createSignal(""); return <Dropdown class="field tk-team" aria-label="Team" value={choice()} onChange={setChoice} options={[{ value: "", label: "No team" }, ...["Fixes", "Docs"].map((name) => ({ value: name, textValue: name, label: () => <TeamChip name={name} /> }))]} />; })()}</div>
          <article class="tk-card st-todo" data-task="fixture-task">
            <span class="tk-glyph st-todo" aria-hidden="true" />
            <div class="tk-card-main">
              <div class="tk-card-title">Move the release notes to Monday</div>
              <div class="tk-meta"><span class="mono tk-id">a01</span><span class="tk-target ell">Any session</span></div>
            </div>
            <div class="tk-card-acts"><button type="button" class="icon-btn tk-act" aria-label="Task actions" title="Task actions"><Icon name="ellipsis" size={15} /></button></div>
            <div class="menu" style={{ position: "static", "margin-top": "8px" }}>
              <button type="button" class="menu-item"><Icon name="users" size={14} />Send to team: Fixes</button>
              <button type="button" class="menu-item"><Icon name="users" size={14} />Send to team: Docs</button>
            </div>
          </article>
        </div>
      </Section>
      <Section title="Send to team on a task card: the refusal" note="A queue's own reason is shown as the notice, in its words.">
        <div class="tk-panel"><div class="notice danger team-notice" role="alert"><Icon name="triangle-alert" size={16} /><span>The queue is full. Task 1 waits for the lead.</span></div>
          <article class="tk-card st-backlog" data-task="fixture-refused">
            <span class="tk-glyph st-backlog" aria-hidden="true" />
            <div class="tk-card-main"><div class="tk-card-title">Fix the flaky upload test</div><div class="tk-meta"><span class="mono tk-id">a02</span><span class="tk-target ell">Any session</span></div></div>
          </article>
        </div>
      </Section>
    </Show>
  </div>;
}

render(() => <App />, document.getElementById("root")!);
