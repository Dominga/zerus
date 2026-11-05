import { For, Show, createEffect, createMemo, createSignal, onCleanup, type JSX } from "solid-js";
import { tr } from "../../i18n";
import { AgentGlyph, Icon } from "../lib/icons";
import { agentKind, computerOffline } from "../../store/catalog";
import { coordinatorActivity, activity } from "../../store/session";
import { memberRow, teams } from "../../store/teams";
import { SHARE, SIDE, dragShare, dragSide, teamLayout, type TeamLayout } from "../../store/teamLayout";
import { selected, setDetailTab, setSelectedKey } from "../../store/ui";
import { ActivityView } from "../sessions/Activity";
import { Composer } from "../sessions/Composer";
import { QuestionCard } from "../sessions/QuestionCard";
import { QueuePanel } from "../sessions/QueuePanel";
import { sendBackMessage } from "./actions";
import { duration, memberProgress, memberStatus, progress, shownMembers, stateLabel, stateTone, teamState, type MemberLive, type MemberStatus } from "./team-model";
import { roleStyle } from "./roles";
import { TeamProposals } from "./ProposalCards";
import { TeamReports } from "./TeamReports";
import { WaitingBanner } from "./Waiting";
import { Blockers, TeamBarActions, TeamBarChips } from "./TeamActions";
import { composer } from "../../store/composer";
import type { ComputerId, TeamMemberView, TeamView } from "../../types";
import type { Session } from "../sessions/types";

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

export function TeamTab(props: { computer: ComputerId; team: string }) {
  const team = () => teams.team(props.computer, props.team);
  const failure = () => teams.failure(props.computer, `team:${props.team}`) ?? teams.followError(props.computer);
  return (
    <div class="team-tab">
      <Show when={team()} fallback={
        <div class="team-loading" role="status">
          <Show when={failure()} fallback={<span class="dim">{tr("Reading the team…")}</span>}>{(text) => <span class="error-text">{text()}</span>}</Show>
        </div>
      }>{(shown) => <TeamWorkspace computer={props.computer} team={shown()} failure={failure()} />}</Show>
    </div>
  );
}

function TeamWorkspace(props: { computer: ComputerId; team: TeamView; failure?: string }) {
  const live = (member: TeamMemberView) => liveOf(props.computer, member);
  const state = createMemo(() => teamState(props.team, live));
  const statuses = createMemo(() => new Map(props.team.members.map((member) => [member.id, memberStatus(props.team, member, live(member))])));
  const problems = createMemo(() => shownMembers(props.team).filter((member) => statuses().get(member.id)?.problem));
  const problemTasks = createMemo(() => new Set(problems().map((member) => statuses().get(member.id)!.task).filter((task) => task > 0)));
  let body!: HTMLDivElement;
  return (
    <>
      <TeamBar computer={props.computer} team={props.team} state={state()} problemTasks={problemTasks()}
        onPhase={(phase) => body?.querySelector<HTMLElement>(`[data-phase="${phase}"]`)?.scrollIntoView({ block: "nearest" })} />
      <WaitingBanner />
      <TeamReports computer={props.computer} team={props.team} />
      <Show when={props.failure}><div class="team-strip danger" role="status"><Icon name="circle-alert" size={14} /><span class="ell">{props.failure}</span></div></Show>
      <Show when={state() === "stuck"}>
        <div class="team-strip warn" role="status">
          <Icon name="triangle-alert" size={14} />
          <b>{tr("Nothing can move without you")}</b>
          <For each={problems()}>{(member) => <span class="team-strip-item">{member.role}: {statuses().get(member.id)?.text}</span>}</For>
          <Blockers team={props.team} />
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
        <div class="team-canvas"><TeamPhases team={props.team} /></div>
        <Divider orientation="vertical" label={tr("Side column width")} layout={teamLayout}
          value={teamLayout.sideWidth()} min={SIDE.min} max={SIDE.max}
          onDrag={(start, from, to) => teamLayout.previewSide(dragSide(start, from.x, to.x, body.clientWidth))}
          start={() => teamLayout.sideWidth()} onCommit={teamLayout.commitSide} onReset={teamLayout.resetSide}
          onStep={(direction) => teamLayout.stepSide(-direction)} />
        <aside class="team-side" style={{ width: `${teamLayout.sideWidth()}px` }} aria-label={tr("Members and coordinator")}>
          <SideColumn computer={props.computer} team={props.team} statuses={statuses()} />
        </aside>
      </div>
    </>
  );
}

/**
 * Task 40: Send back's message reaches the coordinator through its composer
 * (the Tauri client sends a session message only from a composer): the
 * coordinator becomes the selected session, keeps the Team tab, and the
 * message waits in its composer for the owner's Send.
 */
function toCoordinator(computer: ComputerId, team: TeamView, text: string) {
  const row = memberRow(computer, team.members.find((member) => member.lead));
  if (!row) return;
  if (selected()?.key !== row.key) { setSelectedKey(row.key); setDetailTab("team"); }
  let tries = 0;
  const put = () => {
    const view = composer.view();
    if (selected()?.key === row.key && view && !composer.locked()) {
      // A draft already typed stays; the message follows it.
      const draft = composer.text();
      const next = draft.trim() ? `${draft}\n${text}` : text;
      composer.input(next, next.length, next.length);
      return;
    }
    if (++tries < 40) setTimeout(put, 50);
  };
  put();
}

function TeamBar(props: { computer: ComputerId; team: TeamView; state: string; problemTasks: ReadonlySet<number>; onPhase: (phase: number) => void }) {
  const [finishOpen, setFinishOpen] = createSignal(false);
  // U59: the owner's planning mode for the approval (none: as the lead planned).
  const [freeMode, setFreeMode] = createSignal(false);
  const counts = createMemo(() => progress(props.team, props.problemTasks));
  const [now, setNow] = createSignal(Date.now() / 1000);
  const clock = setInterval(() => setNow(Date.now() / 1000), 60_000);
  onCleanup(() => clearInterval(clock));
  const share = (n: number) => (counts().total ? `${(n / counts().total) * 100}%` : "0%");
  return (
    <header class="team-bar">
      <Icon name="users" size={16} />
      <b class="team-name ell" title={props.team.goal || props.team.name}>{props.team.name}</b>
      <span class={`team-badge tone-${stateTone(props.state)}`} data-state={props.state}>{stateLabel(props.state)}</span>
      <div class="team-progress" role="img" aria-label={tr("%1 of %2 tasks accepted", counts().accepted, counts().total)}
        title={tr("%1 of %2 tasks accepted", counts().accepted, counts().total)}>
        <i class="seg accepted" style={{ width: share(counts().accepted) }} />
        <i class="seg submitted" style={{ width: share(counts().submitted) }} />
        <i class="seg active" style={{ width: share(counts().active) }} />
        <i class="seg problem" style={{ width: share(counts().problems) }} />
      </div>
      <span class="mono team-count">{counts().accepted}/{counts().total}</span>
      <Show when={props.team.created > 0}><span class="dim team-age" title={tr("Since the team was created")}>{duration(now() - props.team.created)}</span></Show>
      <span class="grow" />
      <TeamBarChips team={props.team} onPhase={props.onPhase} onFinish={() => setFinishOpen(true)} />
      <Show when={props.state === "planning"}><span class="team-note">{tr("The plan waits for your approval")}</span></Show>
      <Show when={props.state === "done"}><span class="team-note done"><Icon name="check" size={13} />{tr("All tasks are decided")}</span></Show>
      <Show when={props.team.status === "planning"}>
        <label class="team-proposal-option" title={tr("One open phase that takes tasks as they come until you close it")}>
          <input type="checkbox" checked={freeMode()} onChange={(event) => setFreeMode(event.currentTarget.checked)} />{tr("Free planning")}
        </label>
      </Show>
      <TeamBarActions computer={props.computer} team={props.team} planMode={freeMode() ? "free" : ""}
        sendToCoordinator={(text) => toCoordinator(props.computer, props.team, sendBackMessage(text))}
        finishOpen={finishOpen} setFinishOpen={setFinishOpen} />
    </header>
  );
}

/** The canvas until the plan view takes it: the phases with their progress. */
function TeamPhases(props: { team: TeamView }) {
  return (
    <div class="team-phases">
      <For each={props.team.phases} fallback={<p class="dim team-empty">{tr("No phases yet")}</p>}>{(phase) => (
        <div class="team-phase" data-phase={phase.n} classList={{ draft: phase.state === "draft", running: phase.state === "running", done: phase.state === "done" }}>
          <span class="team-phase-n mono">{phase.n}</span>
          <b class="ell">{phase.title || tr("Phase %1", phase.n)}</b>
          <span class="grow" />
          <span class="team-phase-bar"><i style={{ width: phase.total ? `${(phase.done / phase.total) * 100}%` : "0%" }} /></span>
          <span class="mono dim">{phase.done}/{phase.total}</span>
        </div>
      )}</For>
    </div>
  );
}

function SideColumn(props: { computer: ComputerId; team: TeamView; statuses: Map<string, MemberStatus> }) {
  const members = createMemo(() => shownMembers(props.team));
  const lead = createMemo(() => props.team.members.find((member) => member.lead));
  let column!: HTMLDivElement;
  return (
    <div class="team-side-inner" ref={column}>
      <section class="team-members" style={{ height: `${teamLayout.membersShare()}%` }} aria-label={tr("Members")}>
        <div class="team-section-head"><b>{tr("Members")}</b><span class="dim mono">{members().length}</span></div>
        <div class="team-members-list">
          <For each={members()}>{(member) => <MemberRow computer={props.computer} team={props.team} member={member} status={props.statuses.get(member.id)!} />}</For>
        </div>
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

function openRow(row: Session | undefined) {
  if (!row) return;
  setSelectedKey(row.key);
  setDetailTab("activity");
}

function MemberRow(props: { computer: ComputerId; team: TeamView; member: TeamMemberView; status: MemberStatus }) {
  const row = () => memberRow(props.computer, props.member);
  const own = createMemo(() => memberProgress(props.team, props.member));
  const model = () => [props.member.model, props.member.effort].filter(Boolean).join(" ");
  return (
    <div class="team-member" classList={{ problem: props.status.problem, [`tone-${props.status.tone}`]: true, lead: props.member.lead, planned: props.status.kind === "not_created" }}
      data-kind={props.status.kind}>
      <span class="team-tile" title={props.member.agent}><AgentGlyph agent={agentKind(props.member.agent)} size={14} /></span>
      <div class="team-member-main">
        <div class="team-member-line">
          <span class="role-badge ell" style={roleStyle(props.member.role, props.member.lead)}>{props.member.role || props.member.id}</span>
          <Show when={props.member.question}><span class="team-q" title={tr("Needs your answer")}>?1</span></Show>
          <Show when={model()}><span class="dim ell team-model">{model()}</span></Show>
          <span class="grow" />
          <span class={`team-status tone-${props.status.tone}`} title={props.status.text}><Icon name={props.status.icon} size={12} />{props.status.short}</span>
        </div>
        <div class="team-member-line sub">
          <span class="ell team-member-text" title={props.member.error || props.status.text}>{props.member.error || props.status.text}</span>
          <span class="grow" />
          <Show when={own().total > 0}>
            <span class="team-mini-bar" title={tr("%1 of %2 tasks accepted", own().accepted, own().total)}><i style={{ width: `${(own().accepted / own().total) * 100}%` }} /></span>
            <span class="mono dim">{own().accepted}/{own().total}</span>
          </Show>
        </div>
      </div>
      <button class="icon-btn small" disabled={!row()} aria-label={tr("Open session")} data-tip={row() ? tr("Open session") : tr("No session yet")}
        onClick={() => openRow(row())}><Icon name="external-link" size={13} /></button>
    </div>
  );
}

function Coordinator(props: { computer: ComputerId; team: TeamView; lead?: TeamMemberView; status?: MemberStatus }) {
  const row = () => memberRow(props.computer, props.lead);
  /** The selected session is the coordinator: its own follow and composer serve here. */
  const own = () => !!row() && row()!.key === selected()?.key;
  const source = () => (own() ? activity : coordinatorActivity);
  // The coordinator's conversation is read only while this pane shows it.
  const follow = () => {
    const lead = row();
    const target = lead && !own() ? { id: lead.key, computer: lead.computer, key: lead.catalogKey, name: lead.name, runId: lead.runId, conversationId: lead.conversationId } : undefined;
    coordinatorActivity.follow(target);
  };
  createEffect(follow);
  onCleanup(() => coordinatorActivity.follow(undefined));
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
        <div class="activity-wrap team-conversation" style={{ "--col": "100%" } as JSX.CSSProperties}>
          <ActivityView source={source()} session={row} />
          <TeamProposals computer={props.computer} team={props.team} />
          <Show when={own()}>
            <QueuePanel /><QuestionCard />
            <div class="cmp-col"><Composer /></div>
          </Show>
        </div>
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
