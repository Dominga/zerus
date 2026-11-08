import { For, Show } from "solid-js";
import { tr, trNoop } from "../../i18n";
import { AGENT_LABEL, AgentLogo, Icon } from "../lib/icons";
import { computerFresh, computerOffline } from "../../store/catalog";
import { visibleUnreadReply } from "../../store/attention";
import { optionalQuestionCount, optionalQuestionCountText } from "./optional-questions";
import { now } from "../../store/ui";
import { settings } from "../../store/settings";
import { machineAppearance } from "../../store/machineAppearance";
import type { AgentKind, Session, Status, Subagent } from "./types";
import type { TeamSummary } from "../../types";
import { currentChildren, workingChildren } from "./subagents";
import { presentState, stateFacts, STATE_GLYPH, type Shown, type StateIcon } from "./session-states";

// Card anatomy follows tray/src/SessionCardDelegate.h paintCard() at the
// compact density (94 px rows, the only density in the web client):
//   line 1  title (14px/600) + status badge (20px) [+ unread] [+ questions] ... provider badge (18px)
//   line 2  branch/folder icon + "folder / branch" (11px) ... git marks, machine badge (18px)
//   line 3  current action (12px)
//   footer  model (10px) + effort chip (16px) ... agents counter (20px)
// Rows are transparent; hover and selection paint a neutral surface, and
// attention, unread, draft and error rows carry a 3px edge mark.

/** The row's status; rows of a computer whose stream is down show as last known (offline). */
export const statusOf = (s: Session): Status => (computerOffline(s.computer) ? "offline" : s.status);

/** Seconds of the current turn, while the row is working and its start is known. */
function elapsed(s: Session): number | undefined {
  const status = statusOf(s);
  if ((status !== "working" && status !== "background") || !s.turnStarted) return undefined;
  return Math.max(0, Math.floor(now() - s.turnStarted));
}

// The core's closed status vocabulary (SessionPresentation::status). Only these
// exact captions are translated: a composite caption keeps its English text.
const CAPTIONS = new Set<string>([
  trNoop("Agent exited"), trNoop("Archived"), trNoop("Background"), trNoop("Compacting"), trNoop("Connecting"), trNoop("Error"),
  trNoop("Interrupted"), trNoop("Model unavailable"), trNoop("Needs approval"), trNoop("Needs attention"), trNoop("Needs input"),
  trNoop("Not tracked"), trNoop("Paused"), trNoop("Provider error"), trNoop("Ready"), trNoop("Retrying"), trNoop("Scheduled"),
  trNoop("Sign in"), trNoop("Sign in required"), trNoop("Starting"), trNoop("Status unknown"), trNoop("Stopped"),
  trNoop("Using a tool"), trNoop("Waiting for acceptance"), trNoop("Waiting for result"), trNoop("Waiting to retry"), trNoop("Working"),
]);
export const captionText = (caption: string) => (CAPTIONS.has(caption) ? tr(caption) : caption);

/** The Qt caption (SessionPresentation::status) with the Working clock, as Qt shows it. */
export function statusCaption(s: Session): string {
  if (statusOf(s) === "offline") return tr("Offline");
  const state = cardState(s);
  if (state.kind && state.caption) return state.caption;
  if (s.status === "ready" && s.draft) return tr("Draft");
  const seconds = elapsed(s);
  if (seconds === undefined) return captionText(s.caption);
  const h = Math.floor(seconds / 3600), m = Math.floor((seconds % 3600) / 60), sec = seconds % 60;
  return `${captionText(s.caption)} (${h ? `${h}h ${m}m` : `${m}m ${String(sec).padStart(2, "0")}s`})`;
}

/** Short state for strip tiles: elapsed time while working, otherwise the caption. */
export function stripCaption(s: Session): string {
  const seconds = elapsed(s);
  return seconds === undefined ? statusCaption(s) : `${Math.floor(seconds / 60)}m`;
}

// Badge precedence (SessionCardDelegate::statusOf): attention/error, working or
// background, draft, new reply, scheduled, waiting, paused, neutral.
export type StateKind = "deps" | "build" | "cooling" | "cold" | "stopped";
export type BadgeKind =
  | "attention" | "error" | "working" | "background" | "draft" | "unread" | "scheduled" | "waiting" | "paused"
  | "neutral" | "muted-attention" | "muted-unread" | StateKind;

export const isStateKind = (kind: BadgeKind | undefined): kind is StateKind =>
  kind === "deps" || kind === "build" || kind === "cooling" || kind === "cold" || kind === "stopped";

/**
 * The host's session state over the card (SessionCardDelegate cardStatus, U30):
 * Waiting for dependencies, Building, Cooling, Cold and Stopped take the badge of a calm
 * or working card; attention, an error, an archived row and an unsent draft on an idle
 * card keep theirs; the user state names the lead's "Needs you". An older host sends no
 * state and the card keeps its plain presentation.
 */
export interface CardState { shown: Shown; kind?: BadgeKind; caption?: string }
export function cardState(s: Session): CardState {
  const online = !computerOffline(s.computer);
  const shown = presentState(stateFacts(s, online, visibleUnreadReply(s, computerFresh(s.computer))), now());
  const blocked = s.status === "attention" || s.status === "error";
  const key = shown.key;
  if ((key === "deps" || key === "build" || key === "cooling" || key === "cold" || key === "stopped")
    && !blocked && s.state !== "archived" && !(s.draft && (key === "cooling" || key === "cold"))) return { shown, kind: key, caption: shown.badge };
  if (key === "user" && shown.badge === tr("Needs you") && !blocked) return { shown, kind: "attention", caption: shown.badge };
  return { shown };
}

export function badgeKind(s: Session): BadgeKind {
  const status = statusOf(s);
  if (status === "attention") return s.muted ? "muted-attention" : "attention";
  const state = cardState(s).kind;
  if (state) return state;
  if (status === "error" || status === "working" || status === "background") return status;
  if (s.draft && status !== "offline") return "draft";
  if (visibleUnreadReply(s, computerFresh(s.computer))) return s.muted ? "muted-unread" : "unread";
  if (status === "scheduled" || status === "waiting" || status === "paused") return status;
  return "neutral";
}

// Edge mark: attention/error > draft > unread; muted rows turn it grey.
export function emphasis(s: Session): string | undefined {
  const status = statusOf(s);
  if (s.muted && (status === "attention" || visibleUnreadReply(s, computerFresh(s.computer)))) return "muted";
  if (status === "attention" || s.reviewLater) return "attention";
  if (status === "error") return "error";
  if (s.draft) return "draft";
  if (visibleUnreadReply(s, computerFresh(s.computer))) return "unread";
  return undefined;
}

// Status glyphs from SessionStatusBadge::paint, on a 12px box centred where
// Qt centres them (x+11 inside the badge).
const BADGE_GLYPH: Partial<Record<BadgeKind, string>> = {
  paused: `<path d="M4 3V9M8 3V9" stroke-width="1.5"/>`,
  draft: `<path d="M3 9L8.5 3.5M7.5 2.5L9.5 4.5M3 9L2.5 9.5" stroke-width="1.4"/>`,
  scheduled: `<circle cx="6" cy="6" r="4.5"/><path d="M6 6V3.2M6 6H8.2"/>`,
  waiting: `<circle cx="6" cy="6" r="4.5"/><path d="M6 6V3.2M6 6H8.2"/>`,
  unread: `<rect x="2" y="3" width="8" height="6" rx="1" stroke-width="1"/><path d="M2 3L6 6L10 3" stroke-width="1"/>`,
  deps: STATE_GLYPH.hourglass,
  build: STATE_GLYPH.build,
  cooling: STATE_GLYPH.thermo,
  cold: STATE_GLYPH.snow,
  stopped: STATE_GLYPH.stop,
  "muted-unread": `<rect x="2" y="3" width="8" height="6" rx="1" stroke-width="1"/><path d="M2 3L6 6L10 3" stroke-width="1"/>`,
};

export function StatusBadge(props: { kind: BadgeKind; caption: string; iconOnly?: boolean; heat?: number; title?: string }) {
  return (
    <span class={`sbadge k-${props.kind}`} classList={{ "icon-only": props.iconOnly }} title={props.title || undefined}>
      <Show when={props.kind !== "neutral"}>
        <Show when={BADGE_GLYPH[props.kind]} fallback={<i class="sb-mark" aria-hidden="true" />}>
          {(g) => <svg class="sb-glyph" width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" innerHTML={g()} />}
        </Show>
      </Show>
      <Show when={!props.iconOnly}><span class="sb-label">{props.caption}</span></Show>
      <Show when={props.kind === "cooling" && !props.iconOnly && props.heat !== undefined && props.heat >= 0}><i class="sb-heat" style={{ "--heat": props.heat }} aria-hidden="true" /></Show>
    </span>
  );
}

// Provider glyphs from IdentityBadges::paint (pen 1.4), centred at x+10.
const PROVIDER_GLYPH: Record<AgentKind, string> = {
  claude: Array.from({ length: 8 }, (_, i) => `<path transform="rotate(${i * 45} 6 6)" d="M6 8V11"/>`).join(""),
  kimi: `<path d="M3 2V10M3 6L9 2M3 6L9 10"/>`,
  opencode: `<path d="M4 3L1 6L4 9M8 3L11 6L8 9"/>`,
  codex: `<path d="M2 3L5 6L2 9M7 9H10"/>`,
  deepseek: `<path d="M2 3L5 6L2 9M7 9H10"/>`,
  ollama: `<path d="M4 2V5M8 2V5M2.5 6A3.5 3.5 0 0 1 9.5 6V8A3.5 3.5 0 0 1 2.5 8Z"/>`,
  sh: `<path d="M2 3L5 6L2 9M7 9H10"/>`,
};

/** The stroke glyph, for an agent without a logo. */
function StrokeGlyph(props: { agent: AgentKind; size?: number }) {
  return (
    <svg width={props.size ?? 12} height={props.size ?? 12} viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" innerHTML={PROVIDER_GLYPH[props.agent]} />
  );
}

/** The provider's logo alone (as Qt's agentLogoIcon), for badges, menus and filter chips; the stroke glyph without one. */
export function ProviderGlyph(props: { agent: AgentKind; size?: number }) {
  return <AgentLogo agent={props.agent} size={props.size} fallback={<StrokeGlyph agent={props.agent} size={props.size} />} />;
}

export function AgentBadge(props: { agent: AgentKind; narrow?: boolean; max?: number }) {
  return (
    <span class="agent-badge" classList={{ narrow: props.narrow }} style={{ "--agent": `var(--z-agent-${props.agent})`, "max-width": `${props.max ?? 95}px` }} title={AGENT_LABEL[props.agent]}>
      <ProviderGlyph agent={props.agent} />
      <Show when={!props.narrow}><span class="ab-label">{AGENT_LABEL[props.agent]}</span></Show>
    </span>
  );
}

/** A stable palette slot (0-7) for a name, until machine and project colours come from the listing. */
export function paletteIndex(name: string): number {
  let hash = 0;
  for (let i = 0; i < name.length; i++) hash = (hash * 31 + name.charCodeAt(i)) | 0;
  return Math.abs(hash) % 8;
}

export function MachineBadge(props: { alias: string; computer: string; vivid?: boolean; offline?: boolean }) {
  const appearance = () => machineAppearance(settings, props.alias, `var(--z-entity-palette-${paletteIndex(props.alias)})`);
  return (
    <span class="entity mbadge" classList={{ vivid: props.vivid ?? appearance().vivid, offline: computerOffline(props.computer) }}
      style={{ "--entity": appearance().color }} title={props.computer === "" ? tr("%1 (this computer)", props.alias) : props.alias}>
      {props.alias}{props.offline ? tr(" (offline)") : ""}
    </span>
  );
}

type Git = NonNullable<Session["git"]>;
interface GitMark { icon: string; count?: string; tone: "muted" | "attention" | "incoming" | "synced" | "conflict" }

// GitStatusBadge::marks: changed files, outgoing, incoming, then unverified or synced.
export function gitMarks(g: Git): GitMark[] {
  const verified = g.state !== "unknown";
  const count = (n: number) => (n > 99 ? "99+" : String(n));
  const marks: GitMark[] = [];
  if (g.changed) marks.push({ icon: "git-diff", count: count(g.changed), tone: !verified ? "muted" : g.conflicts ? "conflict" : "attention" });
  if (g.ahead) marks.push({ icon: "git-push", count: count(g.ahead), tone: verified ? "attention" : "muted" });
  if (g.behind) marks.push({ icon: "git-pull", count: count(g.behind), tone: verified ? "incoming" : "muted" });
  if (!verified) marks.push({ icon: "git-unknown", tone: "muted" });
  else if (!g.changed && !g.ahead && !g.behind) marks.push({ icon: "git-synced", tone: "synced" });
  return marks;
}

export function gitTooltip(s: Session): string {
  const g = s.git!;
  const verified = g.state !== "unknown";
  const ref = verified ? tr("verified upstream") : tr("last known upstream; remote unverified");
  return [
    tr("Git status (current checkout)"),
    s.cwd,
    s.branch ? tr("Branch: %1", s.branch) : "",
    s.branch ? tr("Upstream: %1", `origin/${s.branch}`) : "",
    tr("%1 changed or new files (including staged and untracked files)", g.changed),
    g.conflicts ? tr("%1 files with merge conflicts", g.conflicts) : "",
    tr("%1 outgoing commits (%2)", g.ahead, ref),
    tr("%1 incoming commits (%2)", g.behind, ref),
    !verified ? tr("Remote publication has not been verified recently.") : !g.changed && !g.ahead && !g.behind ? tr("Clean checkout. Branch matches the verified remote.") : "",
  ].filter(Boolean).join("\n");
}

const countText = (n: number) => (n > 99 ? "99+" : String(n));
const when = (seconds: number) => new Date(seconds * 1000).toLocaleString(undefined, { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });

/** What a chip counts, in Qt's words (SessionRowChips::tooltip). */
export function chipTooltip(s: Session, kind: "services" | "background" | "changes" | "outgoing" | "incoming" | "synced" | "unchecked"): string {
  const g = s.git;
  switch (kind) {
    case "services":
      return [...(s.services ?? []).map((v) => tr("%1, since %2", v.name, v.since > 0 ? when(v.since) : tr("unknown"))), tr("These live with the session and are not Background work.")].join("\n");
    case "background": {
      const jobs = s.backgroundJobs ?? 0;
      return (jobs === 1 ? tr("1 running command") : tr("%1 running commands", jobs)) + (s.backgroundCommand ? `: ${s.backgroundCommand}` : "");
    }
    case "changes": {
      const files = g?.changed ?? 0;
      let text = (files === 1 ? tr("1 changed file") : tr("%1 changed files", files)) + (s.branch ? tr(" on %1", s.branch) : "");
      if (g?.conflicts) text += `\n${tr("%1 files with merge conflicts", g.conflicts)}`;
      return text;
    }
    case "outgoing": return g?.ahead === 1 ? tr("1 outgoing commit") : tr("%1 outgoing commits", g?.ahead ?? 0);
    case "incoming": return g?.behind === 1 ? tr("1 incoming commit") : tr("%1 incoming commits", g?.behind ?? 0);
    case "synced": return tr("Clean and in sync with the checked upstream");
    default: return tr("Not checked yet");
  }
}

const GIT_CHIP: Record<string, "changes" | "outgoing" | "incoming" | "synced" | "unchecked"> =
  { "git-diff": "changes", "git-push": "outgoing", "git-pull": "incoming", "git-synced": "synced", "git-unknown": "unchecked" };

/**
 * The chips right of the folder (SessionRowChips): services, background commands, then
 * git (changed files, outgoing, incoming, in sync or not checked). Each names what it
 * counts and a click opens where it lives.
 */
export function RowChips(props: { session: Session; onChip?: (kind: "services" | "background" | "git") => void }) {
  const s = () => props.session;
  const click = (kind: "services" | "background" | "git") => (e: MouseEvent) => { e.stopPropagation(); props.onChip?.(kind); };
  const services = () => s().services?.length ?? 0;
  const jobs = () => ((s().backgroundJobs ?? 0) > 0 ? s().backgroundJobs! : 0);
  const git = () => (s().git && s().status !== "ended" ? gitMarks(s().git!) : []);
  return (
    <Show when={services() || jobs() || git().length}>
      <span class="gmarks">
        <Show when={services()}>
          <button type="button" class="gmark chip counted t-muted" title={chipTooltip(s(), "services")} onClick={click("services")} onDblClick={(e) => e.stopPropagation()}>
            <Icon name="settings" size={16} /><span>{services()}</span>
          </button>
        </Show>
        <Show when={jobs()}>
          <button type="button" class="gmark chip counted t-bg" title={chipTooltip(s(), "background")} onClick={click("background")} onDblClick={(e) => e.stopPropagation()}>
            <Icon name="active" size={16} /><span>{countText(jobs())}</span>
          </button>
        </Show>
        <For each={git()}>{(m) => (
          <button type="button" class={`gmark chip t-${m.tone}`} classList={{ counted: !!m.count }} title={chipTooltip(s(), GIT_CHIP[m.icon] ?? "unchecked")} onClick={click("git")} onDblClick={(e) => e.stopPropagation()}>
            <Icon name={m.icon} size={16} />
            <Show when={m.count}><span>{m.count}</span></Show>
          </button>
        )}</For>
      </span>
    </Show>
  );
}

export function GitMarks(props: { session: Session }) {
  return (
    <span class="gmarks" title={gitTooltip(props.session)}>
      <For each={gitMarks(props.session.git!)}>{(m) => (
        <span class={`gmark t-${m.tone}`} classList={{ counted: !!m.count }}>
          <Icon name={m.icon} size={16} />
          <Show when={m.count}><span>{m.count}</span></Show>
        </span>
      )}</For>
    </span>
  );
}

/** Children counter: two-person glyph, compact count, chevron (SessionCardDelegate.h:520-543). */
export function AgentsCounter(props: { label: string; expandable: boolean; expanded: boolean; onToggle?: () => void }) {
  const glyph = (
    <svg class="kids-glyph" width="16" height="12" viewBox="0 0 16 12" fill="none" stroke="currentColor" stroke-width="1.2" aria-hidden="true">
      <circle cx="4" cy="3" r="2" /><circle cx="10" cy="4" r="2" />
      <path d="M0 9A4 3 0 0 1 8 9M6 9.5A3.5 2.5 0 0 1 13 9.5" />
    </svg>
  );
  return (
    <Show when={props.expandable} fallback={<span class="sc-kids static">{glyph}<span>{props.label}</span></span>}>
      <button class="sc-kids" classList={{ open: props.expanded }} aria-expanded={props.expanded}
        title={props.expanded ? tr("Hide subagents") : tr("Show subagents")}
        onClick={(e) => { e.stopPropagation(); props.onToggle?.(); }} onDblClick={(e) => e.stopPropagation()}>
        {glyph}
        <span>{props.label}</span>
        <svg class="chev" width="8" height="6" viewBox="0 0 8 6" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M1 2L4 5L7 2" /></svg>
      </button>
    </Show>
  );
}

// Qt keeps min(72, natural width) of the title before badges shrink (60 on
// compact rows). CSS cannot read the natural width, so estimate it from the
// 14px DemiBold average advance.
const titleReserve = (name: string, max: number) => ({ "min-width": `${Math.min(max, Math.ceil(name.length * 7.6))}px` });

function detailText(s: Session): string {
  if (s.summary) return s.summary;
  if (visibleUnreadReply(s, computerFresh(s.computer))) return tr("Response ready to read");
  if (s.status === "ready") return tr("Waiting for your next message");
  if (s.status === "paused") return tr("Resume to continue");
  return "";
}

/**
 * Line 3 (SessionCardDelegate.h:757-790): a state card says what its state is about
 * with its glyph (the reason of deps, what a build runs, the warmth left, how long the
 * cache has been cold, how to go on after a stop); the user state says what it needs
 * when the card has nothing of its own to show.
 */
export function lineThree(s: Session): { text: string; icon?: StateIcon; tone?: StateKind } {
  const state = cardState(s);
  const stateLine = !!state.kind && (isStateKind(state.kind) || (state.shown.key === "user" && state.caption === tr("Needs you")));
  if (stateLine && state.shown.line) return { text: state.shown.line, icon: state.shown.icon || undefined, tone: isStateKind(state.kind) ? state.kind : undefined };
  const text = detailText(s);
  if (!text && state.shown.key === "user" && state.shown.line) return { text: state.shown.line };
  return { text };
}

export const agentsLabel = (s: Session) => {
  const kids = s.subagents ?? [];
  return `${workingChildren(s, !computerOffline(s.computer))}/${kids.length}`;
};

/** The checkout's folder name, as the card's second line shows it. */
export function folderOf(s: Session) {
  return s.cwd.split(/[\\/]/).filter(Boolean).pop() ?? "";
}

/** Optional questions remain separate from blocking input and session status. */
function OptionalQuestionsChip(props: { session: Session }) {
  const optionalCount = () => optionalQuestionCount(props.session, computerFresh(props.session.computer));
  return <Show when={optionalCount()}>
    <span class="qchip" classList={{ muted: !!props.session.muted }} title={tr(optionalCount() === 1 ? "%1 optional question waiting\nThe agent can continue without an answer. Answer or skip it in Activity.\nClick to open." : "%1 optional questions waiting\nThe agent can continue without an answer. Answer or skip it in Activity.\nClick to open.", optionalCount())}>
      <Icon name="question" size={16} />{optionalQuestionCountText(optionalCount())}
    </span>
  </Show>;
}

const TEAM_STATUS: Record<string, string> = {
  running: trNoop("Running"), planning: trNoop("Planning"), paused: trNoop("Paused"),
  stuck: trNoop("Stuck"), done: trNoop("Done"), stopped: trNoop("Stopped"), archived: trNoop("Archived"),
};

/** Qt's 26 x 3 mini bar with its done/total caption (SessionCardDelegate.h:494-507). */
function TeamBar(props: { label: string; done: number; total: number; muted?: boolean }) {
  const pct = () => (props.total > 0 ? Math.min(100, Math.round((props.done / props.total) * 100)) : 0);
  return (
    <span class="tbar" classList={{ muted: props.muted }}>
      <span class="tbar-label">{props.label}</span>
      <span class="tbar-track"><span class="tbar-fill" style={{ width: `${pct()}%` }} /></span>
    </span>
  );
}

/** The lead's team strip (SessionCardDelegate.h:468-541): both bars, status, ?N, members. */
export function TeamStrip(props: { team: TeamSummary; expanded: boolean; onToggle: () => void }) {
  const status = () => TEAM_STATUS[props.team.status] ?? props.team.status;
  return (
    <div class="team-strip" title={`${props.team.name}\n${tr("%1 of %2 phases finished; discarded drafts do not count", props.team.phases_done, props.team.phases_total)}`}>
      <button class="team-toggle" aria-expanded={props.expanded} title={props.expanded ? tr("Hide team members") : tr("Show team members")}
        onClick={(e) => { e.stopPropagation(); props.onToggle(); }}>
        <Icon name="users" size={14} />
      </button>
      <TeamBar label={`${props.team.phase ? `P${props.team.phase} ` : ""}${props.team.phase_done}/${props.team.phase_total}`}
        done={props.team.phase_done} total={props.team.phase_total} />
      <TeamBar label={`${props.team.phases_done}/${props.team.phases_total}`} done={props.team.phases_done}
        total={props.team.phases_total} muted />
      <span class="team-status" classList={{ stuck: props.team.status === "stuck" }}>{tr(status())}</span>
      <Show when={props.team.attention > 0}><span class="team-answers">?{props.team.attention}</span></Show>
      <span class="grow" />
      <span class="team-members">{props.team.members.length}</span>
    </div>
  );
}

/** Qt's full row tooltip (SessionsWindowRows.cpp:692-716): identity, state, meta, model, attention. */
export function fullTooltip(s: Session, team?: TeamSummary): string {
  const lines = [
    s.name,
    `${s.machine} / ${s.agent} / ${statusCaption(s)}`,
    folderOf(s) + (s.branch ? ` / ${s.branch}` : ""),
    s.cwd,
    s.model + (s.effort ? `  ${s.effort}` : ""),
    s.summary,
    visibleUnreadReply(s, computerFresh(s.computer)) ? tr("New reply (not viewed yet)") : "",
    (s.subagents ?? []).length ? tr("Subagents: %1", agentsLabel(s)) : "",
    team ? tr("Team: %1, %2 of %3 phases", team.name, team.phases_done, team.phases_total) : "",
  ];
  return lines.filter(Boolean).join("\n");
}

export function SessionCard(props: {
  session: Session;
  selected: boolean;
  expandable: boolean;
  expanded: boolean;
  onToggleChildren: () => void;
  /** A cold card's + New and Resume; the click never opens the card. */
  onCold?: (action: "new" | "resume") => void;
  /** A chip's click: where its count lives. */
  onChip?: (kind: "services" | "background" | "git") => void;
  team?: TeamSummary;
  depth?: number;
  teamFirst?: boolean;
  teamLast?: boolean;
}) {
  const s = () => props.session;
  const kind = () => badgeKind(s());
  const state = () => cardState(s());
  const line = () => lineThree(s());
  return (
    <div class="scard" classList={{ selected: props.selected, offline: statusOf(s()) === "offline", muted: !!s().muted,
      "team-lead": !!props.team, "team-member": !!props.depth, "team-first": !!props.teamFirst, "team-last": !!props.teamLast }}
      style={{ left: `${2 + (props.depth ?? 0) * 22}px` }} data-emphasis={emphasis(s())}
      title={fullTooltip(s(), props.team)}>
      <div class="sc-l1">
        <Show when={s().muted}><Icon name="mute" size={14} class="sc-mute" /></Show>
        <span class="sc-title ell" style={titleReserve(s().title, 72)} title={s().name}>{s().title}</span>
        <StatusBadge kind={kind()} caption={statusCaption(s())} heat={state().shown.heat} title={state().kind ? state().shown.tooltip : undefined} />
        <Show when={visibleUnreadReply(s(), computerFresh(s().computer)) && (s().status === "working" || s().status === "attention" || s().draft)}>
          <span class="sc-extra"><StatusBadge kind={s().muted ? "muted-unread" : "unread"} caption="" iconOnly /></span>
        </Show>
        <OptionalQuestionsChip session={s()} />
        <span class="grow" />
        <AgentBadge agent={s().agent} />
      </div>
      <div class="sc-l2">
        <Icon name={s().branch ? "git-branch" : "folder"} size={14} class="sc-meta-ico" />
        <span class="sc-meta ell">{folderOf(s())}{s().branch ? ` / ${s().branch}` : ""}</span>
        <span class="grow" />
        <RowChips session={s()} onChip={props.onChip} />
        <MachineBadge alias={s().machine} computer={s().computer} offline={statusOf(s()) === "offline"} />
      </div>
      <div class="sc-l3" classList={{ "s-deps": line().tone === "deps" }}>
        <Show when={s().reviewLater}><Icon name="attention" size={14} class="sc-review" /></Show>
        <Show when={line().icon}>
          {(icon) => <svg class={`sc-state-ico k-${line().tone ?? ""}`} width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" innerHTML={STATE_GLYPH[icon()]} />}
        </Show>
        <span class="ell" title={state().shown.reason && line().tone ? state().shown.reason : undefined}>{line().text}</span>
        <Show when={state().shown.coldActions && props.onCold}>
          <span class="grow" />
          <span class="sc-cold">
            <button type="button" class="new" onClick={(e) => { e.stopPropagation(); props.onCold?.("new"); }} onDblClick={(e) => e.stopPropagation()}>{tr("+ New")}</button>
            <button type="button" onClick={(e) => { e.stopPropagation(); props.onCold?.("resume"); }} onDblClick={(e) => e.stopPropagation()}>{tr("Resume")}</button>
          </span>
        </Show>
      </div>
      <div class="sc-foot">
        <span class="sc-model ell">{s().model}</span>
        <Show when={s().effort}><span class="sc-effort">{s().effort}</span></Show>
        <span class="grow" />
        <Show when={(s().subagents ?? []).length}>
          <AgentsCounter label={agentsLabel(s())} expandable={props.expandable} expanded={props.expanded} onToggle={props.onToggleChildren} />
        </Show>
      </div>
      <Show when={props.team}><TeamStrip team={props.team!} expanded={props.expanded} onToggle={props.onToggleChildren} /></Show>
    </div>
  );
}

const CHILD_STATUS: Record<Subagent["state"], { kind: BadgeKind; caption: string }> = {
  working: { kind: "working", caption: trNoop("Working") },
  ready: { kind: "neutral", caption: trNoop("Ready") },
  attention: { kind: "attention", caption: trNoop("Needs input") },
  error: { kind: "error", caption: trNoop("Error") },
  paused: { kind: "paused", caption: trNoop("Paused") },
  unknown: { kind: "neutral", caption: trNoop("Unknown") },
  ended: { kind: "neutral", caption: trNoop("Ended") },
};
export const childStatus = (c: Subagent, current = true): { kind: BadgeKind; caption: string } => current
  ? { ...CHILD_STATUS[c.state], caption: tr(CHILD_STATUS[c.state].caption) }
  : { kind: "neutral", caption: tr("Recorded") };

/** Native subagent row (64 px): card indented 24 px, one muted detail line. */
export function ChildCard(props: { parent: Session; child: Subagent; last: boolean; first: boolean; selected: boolean }) {
  const status = () => childStatus(props.child, currentChildren(props.parent, !computerOffline(props.parent.computer)));
  return (
    <div class="child-row" classList={{ last: props.last, first: props.first }}>
      <i class="tree" aria-hidden="true" />
      <div class="scard child" classList={{ selected: props.selected }}>
        <div class="sc-l1">
          <span class="sc-title ell" style={titleReserve(props.child.name, 72)}>{props.child.name}</span>
          <StatusBadge kind={status().kind} caption={status().caption} />
          <span class="grow" />
          <AgentBadge agent={props.parent.agent} max={85} />
        </div>
        <div class="sc-child-detail ell">{props.child.summary}</div>
      </div>
    </div>
  );
}

/**
 * Compact (collapsed) row, 40 px: title, then status, optional questions,
 * provider and mute at the right, then the expand button.
 */
export function CompactCard(props: { session: Session; selected: boolean; onExpand: () => void }) {
  const s = () => props.session;
  return (
    <div class="scard compact" classList={{ selected: props.selected }} data-emphasis={emphasis(s())}
      aria-description={tr("Collapsed session row. Press Right to expand.")}>
      <span class="sc-title ell" style={titleReserve(s().title, 60)} title={s().name}>{s().title}</span>
      <StatusBadge kind={badgeKind(s())} caption={statusCaption(s())} />
      <OptionalQuestionsChip session={s()} />
      <AgentBadge agent={s().agent} />
      <Show when={s().muted}><Icon name="mute" size={14} class="sc-mute" /></Show>
      <button class="cc-expand" title={tr("Expand session row")} onClick={(e) => { e.stopPropagation(); props.onExpand(); }} onDblClick={(e) => e.stopPropagation()}>
        <Icon name="expand-row" size={16} />
      </button>
    </div>
  );
}
