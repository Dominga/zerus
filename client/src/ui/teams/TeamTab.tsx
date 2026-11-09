import { For, Show, createEffect, createMemo, createSignal, on, onCleanup, onMount, type JSX } from "solid-js";
import { tr } from "../../i18n";
import { AgentGlyph, Icon } from "../lib/icons";
import { agentKind, computerOffline } from "../../store/catalog";
import { coordinatorActivity, activity } from "../../store/session";
import { memberRow, showCatalog, showTeam, teams } from "../../store/teams";
import { SHARE, SIDE, dragShare, dragSide, teamLayout, type TeamLayout } from "../../store/teamLayout";
import { selected, setDetailTab, setPage, setSelectedKey, subagentOpen, toast } from "../../store/ui";
import { teamAction, teamBuilds, teamTests, teamTimeline, teamsProfile } from "../../bridge";
import { AddMemberDialog, MemberCard, toComposer } from "./MemberCard";
import { TeamBar } from "./team-bar";
import { openRow } from "./openRow";
import { focusedTasks, memberTree } from "./members";
import { sideLists, type SideTarget } from "./sideLists";
import { TeamActions as TeamActionsModel } from "./actions";
import { useModel, windowPort } from "./ProposalCards";
import { PlanView } from "./PlanView";
import { TeamFeed, TeamInbox } from "./TeamDrawers";
import { problemTasks as planProblemTasks } from "./planTasks";
import { TeamCatalogEditor } from "./TeamCatalogEditor";
import { TeamSettingsDialog } from "./TeamSettingsDialog";
import type { TeamBuildsView, TeamTestsView } from "../../types";
import { ActivityView } from "../sessions/Activity";
import { Composer } from "../sessions/Composer";
import { QuestionCard } from "../sessions/QuestionCard";
import { QueuePanel } from "../sessions/QueuePanel";
import { sendBackMessage } from "./actions";
import { duration, memberProgress, memberStatus, progress, shownMembers, stateLabel, stateTone, teamState, type MemberLive, type MemberStatus } from "./team-model";
import { roleStyle } from "./roles";
import { TeamProposals } from "./ProposalCards";
import { ledTeamOf, leadsTeam } from "./leadTeam";
import { ArchivedTaskReport, TeamReports } from "./TeamReports";
import { WorkingModeRow } from "./WorkingModeRow";
import { closeWorkingMode, markWorkingModeKept, workingModeShown } from "../../store/workingMode";
import { WaitingBanner } from "./Waiting";
import { Blockers, PhaseActions, TaskActions, TeamQuestions, createPlanEditing } from "./TeamActions";
import { noReportReply } from "./noReport";
import { TeamActions as TeamActionModel } from "./actions";
import { composer } from "../../store/composer";
import type { ComputerId, TeamCall, TeamMemberView, TeamTaskView, TeamView } from "../../types";
import type { Session } from "../sessions/types";
import { TextArea } from "../lib/TextField";

// The Team tab (Qt's TeamWorkspace, design D3, HANDOFF §14): the team bar and
// its strips, the canvas, and a side column split between the members and the
// coordinator's conversation. Both dividers are the owner's (store/teamLayout):
// a drag shows the size live and stores it when the pointer lets go; nothing
// moves them by itself, and the session list keeps its own width.

/** What the session list says about a member's session. */
function liveOf(computer: ComputerId, member: TeamMemberView): MemberLive {
  const row = memberRow(computer, member);
  return {
    offline: member.live === false || (!!row && (row.status === "offline" || computerOffline(row.computer))),
    attention: row?.status === "attention",
    working: row?.status === "working",
  };
}

export function TeamTab(props: { computer: ComputerId; team: string; owner?: boolean }) {
  const team = () => teams.team(props.computer, props.team);
  const failure = () => teams.failure(props.computer, `team:${props.team}`) ?? teams.followError(props.computer);
  return (
    <div class="team-tab">
      <Show when={team()} fallback={
        <div class="team-loading" role="status">
          <Show when={failure()} fallback={<span class="dim">{tr("Reading the team…")}</span>}>{(text) => <span class="error-text">{text()}</span>}</Show>
        </div>
      }>{(shown) => <TeamWorkspace computer={props.computer} team={shown()} owner={props.owner ?? true} failure={failure()} />}</Show>
    </div>
  );
}

function TeamWorkspace(props: { computer: ComputerId; team: TeamView; owner: boolean; failure?: string }) {
  const live = (member: TeamMemberView) => liveOf(props.computer, member);
  const state = createMemo(() => teamState(props.team, live));
  const statuses = createMemo(() => new Map(props.team.members.map((member) => [member.id, memberStatus(props.team, member, live(member))])));
  const problems = createMemo(() => shownMembers(props.team).filter((member) => statuses().get(member.id)?.problem));
  const problemTasks = createMemo(() => new Set(problems().map((member) => statuses().get(member.id)!.task).filter((task) => task > 0)));
  // Task 197 (P2): the plan tints each problem task with its reason (Qt
  // problemTasks/problemTexts): the members' problems, then each task's own.
  const planProblems = createMemo(() => planProblemTasks(props.team, statuses()));
  // Task 197 (P8): a click on a member's card shows only its tasks.
  const workingOn = (task: TeamTaskView) => {
    const member = props.team.members.find((candidate) => candidate.id === task.assignee);
    return !!member && (live(member).working || member.activity === "busy");
  };
  // Task 197 (P6): only the switch's own field; the server merges it.
  const setAutoAccept = async (on: boolean) => {
    try {
      const outcome = await teamAction({ computer: props.computer, team: props.team.id, team_name: props.team.name,
        action: { kind: "settings", settings: { auto_accept: on } } as TeamCall["action"] });
      if (outcome.outcome === "failed" || outcome.outcome === "uncertain") toast(tr("Auto-accept drafts"), outcome.detail, "error", true);
    } catch (cause) {
      toast(tr("Auto-accept drafts"), String(cause), "error", true);
    }
  };
  const [settingsOpen, setSettingsOpen] = createSignal(false);
  const [catalogOpen, setCatalogOpen] = createSignal(false);
  const [tests, setTests] = createSignal<TeamTestsView | null>(null);
  const [reportTask, setReportTask] = createSignal<number | null>(null);
  const reportTeam = createMemo(() => props.team.id);
  createEffect(() => { props.computer; reportTeam(); setReportTask(null); });
  const graph = () => teams.graph(props.computer, props.team.id);
  const profile = () => teams.profile(props.computer, props.team.id);
  // Task 196: the phase header's actions (PhaseActions) replace the plan's
  // lone Approve (0706a4dc4); editing the plan (Qt planning mode, a draft's
  // links) goes through core's plan edit and edit-draft.
  const plan = createPlanEditing(() => ({ computer: props.computer, team: props.team }));
  let body!: HTMLDivElement;
  // Task #226: the Lanes mode renames a lane or moves a task through the same
  // `edit-draft` the Qt workspace sends (`tasks_update`), never a browser
  // confirm; a refusal is a toast and the view says so.
  const editLanes = async (phase: number, updates: { id: number; lane: { n: number; label: string } | null }[]) => {
    const call: TeamCall = { computer: props.computer, team: props.team.id, action: { kind: "edit_tasks", phase, tasks: updates } };
    try {
      const outcome = await teamAction(call);
      if (outcome.outcome === "done") return true;
      const detail = outcome.outcome === "confirm" ? outcome.confirmation.text : outcome.detail ?? outcome.outcome;
      toast(tr("Lane not changed"), detail, "error", true);
    } catch (cause) {
      toast(tr("Lane not changed"), String(cause), "error", true);
    }
    return false;
  };
  createEffect(() => {
    const hide = showTeam(props.computer, props.team.id);
    onCleanup(hide);
  });
  createEffect(() => {
    if (!catalogOpen()) return;
    const hide = showCatalog(props.computer);
    onCleanup(hide);
  });
  // Task 199: Load older appends the next page; the first page replaces.
  const loadTests = async (before: number | null) => {
    try {
      const page = await teamTests(props.computer, props.team.id, before);
      const shown = tests();
      setTests(before !== null && shown ? { ...page, runs: [...shown.runs, ...page.runs.filter((run) => !shown.runs.some((old) => old.id === run.id))] } : page);
    } catch {
      // The panel keeps what it showed; the follow error strip names reads.
    }
  };
  const [builds, setBuilds] = createSignal<TeamBuildsView | null>(null);
  const [buildsError, setBuildsError] = createSignal("");
  const loadBuilds = async () => {
    try { setBuilds(await teamBuilds(props.computer, props.team.id)); setBuildsError(""); }
    catch (cause) { setBuildsError(String(cause instanceof Error ? cause.message : cause)); }
  };
  const { model: actions } = useModel((changed) => new TeamActionsModel(windowPort, () => ({ computer: props.computer, team: props.team }), changed));
  const forceTest = async (suite: string) => {
    const outcome = await actions.forceTest(suite);
    if (outcome && outcome !== "cancelled" && outcome.outcome === "done") void loadTests(null);
    return outcome && outcome !== "cancelled" && outcome.outcome !== "done" ? actions.notice : "";
  };
  // Task 199: a member card's click shows only its tasks; the side column
  // opens a task in the plan (Needs your answer, a member's queue).
  const [focus, setFocus] = createSignal<string | null>(null);
  const [opened, setOpened] = createSignal<{ task: number } | null>(null);
  const openTask = (task: number) => setOpened({ task });
  const shownTeam = createMemo(() => {
    const id = focus();
    return id ? { ...props.team, tasks: focusedTasks(props.team.tasks, id) } : props.team;
  });
  const shownGraph = (graph: NonNullable<ReturnType<typeof teams.graph>>) => {
    const id = focus();
    if (!id) return graph;
    const keep = new Set(focusedTasks(props.team.tasks, id).map((task) => task.id));
    return { ...graph, nodes: graph.nodes.filter((node) => keep.has(node.id)), edges: graph.edges.filter((edge) => keep.has(edge.from) && keep.has(edge.to)) };
  };
  const goTo = (target: SideTarget) => {
    if (!target) return;
    if ("task" in target) return openTask(target.task);
    if ("phase" in target) return body?.querySelector<HTMLElement>(`[data-phase="${target.phase}"]`)?.scrollIntoView({ block: "nearest" });
    const row = memberRow(props.computer, props.team.members.find((member) => member.id === target.member));
    if (row) { setSelectedKey(row.key); setDetailTab("activity"); }
  };
  // Task #451: the working-mode block's buttons answer visibly. A refusal says
  // why in the block (dismissible), Keep hides it only after hgs kept it, and
  // Propose says what hgs did: a card in Waiting for you, or that the settings
  // already follow the project's profile (the block then leaves by itself).
  const [modeNotice, setModeNotice] = createSignal("");
  const [modeNoticeTone, setModeNoticeTone] = createSignal<"info" | "danger">("info");
  const [modeBusy, setModeBusy] = createSignal(false);
  const keepMode = async () => {
    if (modeBusy()) return;
    setModeBusy(true); setModeNotice("");
    const outcome = await actions.keepWorkingMode();
    setModeBusy(false);
    if (!outcome || outcome === "cancelled") return;
    if (outcome.outcome === "done") {
      markWorkingModeKept(props.team);
      toast(tr("Working mode kept"), tr("This team's settings stay; the block returns when the project's profile changes."));
      return;
    }
    setModeNoticeTone("danger");
    setModeNotice(actions.notice || tr("The working mode was not kept."));
  };
  const proposeMode = async () => {
    if (modeBusy()) return;
    setModeBusy(true); setModeNotice("");
    const outcome = await actions.proposeWorkingMode();
    setModeBusy(false);
    if (!outcome || outcome === "cancelled") return;
    if (outcome.outcome === "failed" || outcome.outcome === "uncertain") {
      setModeNoticeTone("danger");
      setModeNotice(actions.notice || tr("The proposal was not sent."));
      return;
    }
    setModeNoticeTone("info");
    const receipt = ("receipt" in outcome ? outcome.receipt : undefined) ?? {};
    const status = typeof receipt.status === "string" ? receipt.status : "";
    const named = [receipt.title, receipt.working_mode].find((value): value is string => typeof value === "string" && value !== "") || "";
    const text = status === "already"
      ? tr("The project's profile selects %1, and this team's settings already follow it.", named)
      : tr("The switch is on its card in Waiting for you: Allow applies it.");
    setModeNotice(text);
    toast(tr("Working mode"), text);
  };
  return (
    <>
      <TeamBar computer={props.computer} team={props.team} state={state()} problemTasks={problemTasks()}
        onPhase={(phase) => body?.querySelector<HTMLElement>(`[data-phase="${phase}"]`)?.scrollIntoView({ block: "nearest" })}
        onSettings={() => setSettingsOpen(true)}
        onCatalog={() => setCatalogOpen(true)} />
      <WaitingBanner />
      <TeamReports computer={props.computer} team={props.team} onOpenReport={setReportTask} />
      <Show when={reportTask()}>{(task) => <ArchivedTaskReport computer={props.computer} team={reportTeam()} task={task()} onClose={() => setReportTask(null)} />}</Show>
      <Show when={workingModeShown(props.team)}>
        <WorkingModeRow preset={props.team.working_mode!.preset}
          title={props.team.working_mode!.proposal?.title || props.team.working_mode!.proposal?.preset || props.team.working_mode!.preset}
          reasons={props.team.working_mode!.proposal?.reasons ?? []}
          disagreeing={props.team.working_mode!.disagreeing ?? []}
          busy={modeBusy()}
          notice={modeNotice()}
          noticeTone={modeNoticeTone()}
          onDismissNotice={() => setModeNotice("")}
          onKeep={() => void keepMode()}
          onPropose={() => void proposeMode()}
          onClose={() => closeWorkingMode(props.team)} />
      </Show>
      <Show when={props.failure}><div class="team-strip danger" role="status"><Icon name="circle-alert" size={14} /><span class="ell">{props.failure}</span></div></Show>
      <Show when={state() === "stuck"}>
        <div class="team-strip warn" role="status">
          <Icon name="triangle-alert" size={14} />
          <b>{tr("Nothing can move without you")}</b>
          <For each={problems()}>{(member) => <span class="team-strip-item">{member.role}: {statuses().get(member.id)?.text}</span>}</For>
          <Blockers team={props.team} onTask={openTask} />
        </div>
      </Show>
      <Show when={props.team.status === "planning" && props.team.validation.errors.length > 0}>
        <div class="team-strip danger" role="status">
          <Icon name="circle-alert" size={14} />
          <b>{props.team.validation.errors.length === 1 ? tr("1 plan error") : tr("%1 plan errors", props.team.validation.errors.length)}</b>
          <For each={props.team.validation.errors.slice(0, 3)}>{(finding) => <span class="team-strip-item ell">{finding.text}</span>}</For>
        </div>
      </Show>
      <div class="team-body" ref={body}>
        <div class="team-main">
        <div class="team-canvas">
          <Show when={plan.planning()}>
            <div class="team-plan-tools" role="toolbar" aria-label={tr("Plan editing")}>
              <button type="button" class="btn small" data-action="plan-add-task" title={tr("Add a task to the plan")} onClick={plan.addTask}>
                <Icon name="plus" size={12} />{tr("Task")}
              </button>
              <span class="dim">{tr("Click a task to edit it; drag from a node's right edge onto a task to connect them; click a dependency to remove it.")}</span>
              <Show when={plan.notice()}><span class="danger" role="status">{plan.notice()}</span></Show>
            </div>
          </Show>
          {plan.dialog()}
          <Show when={focus()}>{(id) => (
            <div class="team-focus-strip" role="status">
              <button type="button" class="team-chip accent" data-action="focus-clear" title={tr("Show every member")} onClick={() => setFocus(null)}>
                {tr("Only %1's tasks", props.team.members.find((member) => member.id === id())?.role || id())} <Icon name="x" size={11} />
              </button>
            </div>
          )}</Show>
          <Show when={graph()} fallback={<TeamPhases computer={props.computer} team={props.team} owner={props.owner} />}>
            {(shown) => (
              <PlanView
                team={shownTeam()}
                graph={shownGraph(shown())}
                openTask={opened()}
                builds={builds()}
                buildsError={buildsError()}
                onLoadBuilds={() => void loadBuilds()}
                onForceTest={forceTest}
                profile={profile()}
                tests={tests()}
                onLoadProfile={() => { void teamsProfile(props.computer); }}
                loadTimeline={(phase) => teamTimeline(props.computer, props.team.id, phase)}
                onLoadTests={loadTests}
                onOpenReport={setReportTask}
                taskActions={(id) => {
                  const task = props.team.tasks.find((candidate) => candidate.id === id);
                  return task && !task.archived ? <TaskActions computer={props.computer} team={props.team} task={task} /> : <></>;
                }}
                noReportReply={(id) => noReportReply(teams.inbox(props.computer, props.team.id)?.items ?? [], id)}
                onUseLastReply={(task) => void new TeamActionModel(windowPort, () => ({ computer: props.computer, team: props.team })).useLastReply(task)}
                phaseActions={(n) => <PhaseActions computer={props.computer} team={props.team} phase={n} owner={props.owner} />}
                editing={plan.editing()}
                problems={planProblems()}
                working={workingOn}
                member={focus() ?? ""}
                onMember={(id) => setFocus(id || null)}
                onAutoAccept={(on) => void setAutoAccept(on)}
                onLaneEdit={editLanes}
              />
            )}
          </Show>
        </div>
        {/* Task 198 (W1): the lead's inbox and the team's feed under the plan, as Qt. */}
        <TeamInbox computer={props.computer} team={props.team} />
        <TeamFeed computer={props.computer} team={props.team} focusRole={props.team.members.find((member) => member.id === focus())?.role} />
        </div>
        <Divider orientation="vertical" label={tr("Side column width")} layout={teamLayout}
          value={teamLayout.sideWidth()} min={SIDE.min} max={SIDE.max}
          onDrag={(start, from, to) => teamLayout.previewSide(dragSide(start, from.x, to.x, body.clientWidth))}
          start={() => teamLayout.sideWidth()} onCommit={teamLayout.commitSide} onReset={teamLayout.resetSide}
          onStep={(direction) => teamLayout.stepSide(-direction)} />
        <aside class="team-side" style={{ width: `${teamLayout.sideWidth()}px` }} aria-label={tr("Members and coordinator")}>
          <SideColumn computer={props.computer} team={props.team} statuses={statuses()} focus={focus()}
            onFocus={(id) => setFocus(focus() === id ? null : id)} onTask={openTask} onGo={goTo} />
        </aside>
      </div>
      <Show when={settingsOpen()}>
        <TeamSettingsDialog team={props.team} onClose={() => setSettingsOpen(false)} />
      </Show>
      <Show when={catalogOpen()}>
        <TeamCatalogEditor onOpenDesigner={() => { setCatalogOpen(false); setPage("designer"); }} />
      </Show>
    </>
  );
}

/**
 * Task 317 (F13): the lead's own session carries its team outside the Team tab
 * too, as Qt's lead header does: the team bar (state, working mode, progress,
 * problems, settings, catalog and the team actions) and a strip naming the team
 * the session leads, with Open team panel. A member's session shows none; the
 * Team tab has its own bar, so the host leaves this out there.
 */
export function LeadTeamHeader() {
  const row = () => selected();
  const led = createMemo(() => {
    const now = row();
    return now && now.state !== "archived" && !subagentOpen() ? ledTeamOf(teams.list(now.computer), now) : undefined;
  });
  // The team's own read, as LeadProposals follows it: only a different team re-subscribes.
  createEffect(on(() => {
    const now = row(), summary = led();
    return now && summary && summary.status !== "draft" ? `${now.computer}\n${summary.id}` : "";
  }, (key) => {
    if (!key) return;
    const at = key.indexOf("\n");
    onCleanup(showTeam(key.slice(0, at), key.slice(at + 1)));
  }));
  const team = createMemo(() => {
    const now = row(), summary = led();
    if (!now || !summary || summary.status === "draft") return undefined;
    const view = teams.team(now.computer, summary.id);
    return view && leadsTeam(view, now) ? view : undefined;
  });
  return <Show when={team()}>{(view) => <LeadTeamBar computer={row()!.computer} team={view()} />}</Show>;
}

function LeadTeamBar(props: { computer: ComputerId; team: TeamView }) {
  const live = (member: TeamMemberView) => liveOf(props.computer, member);
  const state = createMemo(() => teamState(props.team, live));
  const problemTasks = createMemo(() => {
    const tasks = new Set<number>();
    for (const member of shownMembers(props.team)) {
      const status = memberStatus(props.team, member, live(member));
      if (status.problem && status.task > 0) tasks.add(status.task);
    }
    return tasks;
  });
  const [settingsOpen, setSettingsOpen] = createSignal(false);
  const [catalogOpen, setCatalogOpen] = createSignal(false);
  createEffect(() => {
    if (!catalogOpen()) return;
    onCleanup(showCatalog(props.computer));
  });
  return (
    <div class="lead-team" data-lead-team={props.team.id}>
      <TeamBar computer={props.computer} team={props.team} state={state()} problemTasks={problemTasks()}
        onPhase={() => setDetailTab("team")} onSettings={() => setSettingsOpen(true)} onCatalog={() => setCatalogOpen(true)} />
      <div class="lead-team-strip" data-lead-team-strip>
        <span>{tr("Team")}</span><b class="ell">{props.team.name}</b><span class="team-pill">{tr("Lead")}</span>
        <span class="grow" />
        <button type="button" class="btn ghost small" data-open-team-panel onClick={() => setDetailTab("team")}>{tr("Open team panel")}</button>
      </div>
      <Show when={settingsOpen()}>
        <TeamSettingsDialog team={props.team} onClose={() => setSettingsOpen(false)} />
      </Show>
      <Show when={catalogOpen()}>
        <TeamCatalogEditor onOpenDesigner={() => { setCatalogOpen(false); setPage("designer"); }} />
      </Show>
    </div>
  );
}

/** The canvas until the plan view takes it: the phases with their progress. */
function TeamPhases(props: { computer: ComputerId; team: TeamView; owner: boolean }) {
  return (
    <div class="team-phases">
      <For each={props.team.phases} fallback={<p class="dim team-empty">{tr("No phases yet")}</p>}>{(phase) => (
        <div class="team-phase" data-phase={phase.n} classList={{ draft: phase.state === "draft", running: phase.state === "running", done: phase.state === "done" }}>
          <span class="team-phase-n mono">{phase.n}</span>
          <b class="ell">{phase.title || tr("Phase %1", phase.n)}</b>
          <PhaseActions computer={props.computer} team={props.team} phase={phase.n} owner={props.owner} />
          <span class="grow" />
          <span class="team-phase-bar"><i style={{ width: phase.total ? `${(phase.done / phase.total) * 100}%` : "0%" }} /></span>
          <span class="mono dim">{phase.done}/{phase.total}</span>
        </div>
      )}</For>
    </div>
  );
}

function SideColumn(props: {
  computer: ComputerId; team: TeamView; statuses: Map<string, MemberStatus>;
  focus: string | null; onFocus: (id: string) => void; onTask: (id: number) => void; onGo: (target: SideTarget) => void;
}) {
  const members = createMemo(() => shownMembers(props.team));
  const lead = createMemo(() => props.team.members.find((member) => member.lead));
  // Task #347: the Members header explains the card colours on hover.
  const memberLegend = () => tr("State colours: working, building and running tests are blue; waiting for its own job, for the lead or for a person is amber; needs your answer, blocked or failed is red; idle or done is grey; paused or offline is muted and hatched.");
  // Spec 5.6: the members as a tree; a folded sub-lead hides its reports.
  const [folded, setFolded] = createSignal<Set<string>>(new Set());
  const tree = createMemo(() => {
    const shown = new Set(members().map((member) => member.id));
    return memberTree(props.team, folded()).filter((node) => !node.hidden && shown.has(node.member.id));
  });
  const fold = (id: string) => { const next = new Set(folded()); if (!next.delete(id)) next.add(id); setFolded(next); };
  const [adding, setAdding] = createSignal(false);
  const { model } = useModel((changed) => new TeamActionsModel(windowPort, () => ({ computer: props.computer, team: props.team }), changed));
  const canAdd = () => ["planning", "running", "paused"].includes(props.team.status);
  let column!: HTMLDivElement;
  return (
    <div class="team-side-inner" ref={column}>
      <SideLists team={props.team} onGo={props.onGo} />
      <TeamQuestions computer={props.computer} team={props.team} />
      <section class="team-members" style={{ height: `${teamLayout.membersShare()}%` }} aria-label={tr("Members")}>
        <div class="team-section-head" title={memberLegend()}><b>{tr("Members")}</b><span class="dim mono">{members().length}</span>
          <span class="grow" />
          <Show when={canAdd()}>
            <button type="button" class="btn ghost small" data-action="add-member" title={tr("Add a member; it starts with the first task it gets")} onClick={() => setAdding(true)}>{tr("+ Member")}</button>
          </Show>
        </div>
        <div class="team-members-list">
          <For each={tree()}>{(node) => (
            <MemberCard computer={props.computer} team={props.team} member={node.member as TeamMemberView} status={props.statuses.get(node.member.id)!}
              depth={node.depth} focused={props.focus === node.member.id} onFocus={() => props.onFocus(node.member.id)}
              folded={folded().has(node.member.id)} onFold={() => fold(node.member.id)} onTask={props.onTask} />
          )}</For>
        </div>
        <Show when={adding()}><AddMemberDialog close={() => setAdding(false)} submit={(member) => void model.addMember(member)} /></Show>
      </section>
      <Divider orientation="horizontal" label={tr("Members and coordinator")} layout={teamLayout}
        value={teamLayout.membersShare()} min={SHARE.min} max={SHARE.max}
        onDrag={(start, from, to) => teamLayout.previewShare(dragShare(start, from.y, to.y, column.clientHeight))}
        start={() => teamLayout.membersShare()} onCommit={teamLayout.commitShare} onReset={teamLayout.resetShare}
        onStep={(direction) => teamLayout.stepShare(direction)} />
      <Coordinator computer={props.computer} team={props.team} lead={lead()} status={lead() ? props.statuses.get(lead()!.id) : undefined} />
    </div>
  );
}

/**
 * Task 199 (Qt TeamWorkspace.cpp 3617-3700): Plan problems; each row opens
 * what it names. Task 345 removed Needs your attention and Needs your answer:
 * each member's card carries its request.
 */
function SideLists(props: { team: TeamView; onGo: (target: SideTarget) => void }) {
  const lists = createMemo(() => sideLists(props.team));
  const [open, setOpen] = createSignal<Record<string, boolean>>({});
  const shown = (id: string, initial: boolean) => open()[id] ?? initial;
  return (
    <For each={lists()}>{(list) => (
      <section class={`team-side-list team-side-${list.id}`} data-list={list.id}>
        <button type="button" class="team-section-head team-side-toggle" aria-expanded={shown(list.id, list.open)}
          onClick={() => setOpen({ ...open(), [list.id]: !shown(list.id, list.open) })}>
          <Icon name={shown(list.id, list.open) ? "chevron-down" : "chevron-right"} size={11} />
          <b>{list.title}</b><span class={`team-side-note ${list.id === "plan" && props.team.validation.errors.length ? "danger" : "warn"}`}>{list.note}</span>
        </button>
        <Show when={shown(list.id, list.open)}>
          <For each={list.rows}>{(row) => (
            <div class={`team-side-row tone-${row.tone}`} data-row={row.key}>
              <Show when={row.role}><span class="role-badge ell" style={roleStyle(row.role, false)}>{row.role}</span></Show>
              <span class="ell grow" title={row.text}>{row.text}</span>
              <Show when={row.target && row.action}>
                <button type="button" class="btn ghost small" data-action="side-go" onClick={() => props.onGo(row.target)}>{row.action}</button>
              </Show>
            </div>
          )}</For>
        </Show>
      </section>
    )}</For>
  );
}

/**
 * The lead's conversation where another view shows it (the coordinator pane,
 * and task 200's New team Chat): the selected session's own follow and
 * composer when it is the lead; otherwise the lead's read-only follow.
 *
 * Task 198 (W4): Qt's coordinator conversation always has its input
 * (TeamConversation.cpp:117-163). The Tauri client sends a session message
 * only from that session's composer, one at a time, so while another session
 * is selected this field takes the text and its button carries it into the
 * lead's own composer (the Team tab stays), where it is sent as any message,
 * with its attachments, queue and question card.
 */
export function LeadConversation(props: { computer: ComputerId; lead?: Pick<TeamMemberView, "session">; label: string; placeholder: string; send: string; children?: JSX.Element }) {
  const row = () => memberRow(props.computer, props.lead);
  /** The selected session is the lead: its own follow and composer serve here. */
  const own = () => !!row() && row()!.key === selected()?.key;
  const source = () => (own() ? activity : coordinatorActivity);
  // The lead's conversation is read only while this pane shows it.
  createEffect(() => {
    const lead = row();
    const target = lead && !own() ? { id: lead.key, computer: lead.computer, key: lead.catalogKey, name: lead.name, runId: lead.runId, conversationId: lead.conversationId } : undefined;
    coordinatorActivity.follow(target);
  });
  onCleanup(() => coordinatorActivity.follow(undefined));
  const [text, setText] = createSignal("");
  const carry = () => { toComposer(props.computer, props.lead, text().trim()); setText(""); };
  return (
    <div class="activity-wrap team-conversation" style={{ "--col": "100%" } as JSX.CSSProperties}>
      <ActivityView source={source()} session={row} />
      {props.children}
      <Show when={own()} fallback={
        <div class="cmp-col team-coordinator-draft">
          <TextArea class="field" rows={2} data-coordinator-draft aria-label={props.label} placeholder={props.placeholder} value={text()}
            onInput={(event) => setText(event.currentTarget.value)}
            onKeyDown={(event) => { if (event.key === "Enter" && !event.shiftKey && !event.isComposing) { event.preventDefault(); carry(); } }} />
          <button class="btn small primary" data-coordinator-send onClick={carry}>{props.send}</button>
        </div>
      }>
        <QueuePanel /><QuestionCard />
        <div class="cmp-col"><Composer /></div>
      </Show>
    </div>
  );
}

function Coordinator(props: { computer: ComputerId; team: TeamView; lead?: TeamMemberView; status?: MemberStatus }) {
  const row = () => memberRow(props.computer, props.lead);
  const own = () => !!row() && row()!.key === selected()?.key;
  return (
    <section class="team-coordinator" aria-label={tr("Coordinator")}>
      <div class="team-section-head">
        <span class="team-tile"><AgentGlyph agent={agentKind(props.lead?.agent)} size={14} /></span>
        <b>{tr("Coordinator")}</b>
        <Show when={props.status}>{(status) => <span class={`team-status tone-${status().tone}`}><Icon name={status().icon} size={12} />{status().short}</span>}</Show>
        <span class="dim ell team-session">{props.lead?.session?.name ?? ""}</span>
        <span class="grow" />
        <Show when={row() && !own()}>
          <button class="btn small ghost" onClick={() => openRow(row())}><Icon name="external-link" size={12} />{tr("Open in Activity")}</button>
        </Show>
      </div>
      <Show when={row()} fallback={<p class="dim team-empty">{props.lead?.session ? tr("The coordinator's session is not on the session list.") : tr("The coordinator has no session yet.")}</p>}>
        <LeadConversation computer={props.computer} lead={props.lead} label={tr("Message the coordinator")} placeholder={tr("Message the coordinator…")} send={tr("Write to coordinator")}>
          <TeamProposals computer={props.computer} team={props.team} />
        </LeadConversation>
      </Show>
    </section>
  );
}

/**
 * A divider the owner moves: a pointer drag previews (`onDrag` with where it
 * started and where it is), letting go stores (`onCommit`), a lost drag keeps
 * the stored size, a double click restores the default, arrow keys step it.
 */
function Divider(props: {
  orientation: "vertical" | "horizontal"; label: string; layout: TeamLayout; value: number; min: number; max: number;
  start: () => number; onDrag: (start: number, from: { x: number; y: number }, to: { x: number; y: number }) => void;
  onCommit: () => void; onReset: () => void; onStep: (direction: number) => void;
}) {
  const [dragging, setDragging] = createSignal(false);
  let origin = { x: 0, y: 0 }, start = 0;
  const end = () => { if (dragging()) { setDragging(false); props.layout.cancel(); } };
  onCleanup(end);
  const vertical = () => props.orientation === "vertical";
  return (
    <div class={`team-divider ${props.orientation}`} classList={{ dragging: dragging() }} role="separator" tabindex="0"
      aria-orientation={props.orientation} aria-label={props.label} aria-valuemin={props.min} aria-valuemax={props.max} aria-valuenow={props.value}
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        e.preventDefault();
        e.currentTarget.setPointerCapture(e.pointerId);
        origin = { x: e.clientX, y: e.clientY };
        start = props.start();
        setDragging(true);
      }}
      onPointerMove={(e) => { if (dragging()) props.onDrag(start, origin, { x: e.clientX, y: e.clientY }); }}
      onPointerUp={(e) => {
        e.currentTarget.releasePointerCapture(e.pointerId);
        if (!dragging()) return;
        setDragging(false);
        props.onCommit();
      }}
      onPointerCancel={end}
      onLostPointerCapture={end}
      onDblClick={() => props.onReset()}
      onKeyDown={(e) => {
        const step = vertical()
          ? (e.key === "ArrowLeft" ? -1 : e.key === "ArrowRight" ? 1 : 0)
          : (e.key === "ArrowUp" ? -1 : e.key === "ArrowDown" ? 1 : 0);
        if (!step) return;
        e.preventDefault();
        props.onStep(step);
      }}
    />
  );
}
