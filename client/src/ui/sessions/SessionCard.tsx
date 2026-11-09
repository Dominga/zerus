import { For, Show, createEffect, createMemo, createSignal, on, onCleanup, onMount } from "solid-js";
import { steadyTitle } from "../lib/steady-title";
import { tr, trNoop } from "../../i18n";
import { AGENT_LABEL, AgentLogo, Icon } from "../lib/icons";
import { computerFresh, computerOffline } from "../../store/catalog";
import { quietAttention, visibleUnreadReply } from "../../store/attention";
import { optionalQuestionCount, optionalQuestionCountText } from "./optional-questions";
import { now } from "../../store/ui";
import { settings } from "../../store/settings";
import { machineAppearance } from "../../store/machineAppearance";
import { sessionMark, type AccountMark } from "../../store/accountMarks";
import type { AgentKind, Session, Status, Subagent } from "./types";
import type { TeamSummary } from "../../types";
import type { Team } from "./group-rows";
import type { LogoPath } from "../lib/agent-logos";
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
  trNoop("Using a tool"), trNoop("Waiting for acceptance"), trNoop("Waiting for result"), trNoop("Waiting to retry"), trNoop("Warming cache"), trNoop("Working"),
]);
export const captionText = (caption: string) => (CAPTIONS.has(caption) ? tr(caption) : caption);

/** The Qt caption (SessionPresentation::status) with the Working clock, as Qt shows it. */
/**
 * The status a row shows, with its Working time. `seconds: false` is for a
 * title or tooltip, which never carries a live seconds counter (zerus-5ryrx):
 * minutes only, as past the first hour.
 */
export function statusCaption(s: Session, seconds = true): string {
  if (statusOf(s) === "offline") return tr("Offline");
  const state = cardState(s);
  if (state.kind && state.caption) return state.caption;
  if (s.status === "ready" && s.draft) return tr("Draft");
  // Task 315 (F2): the unread badge reads "New reply", as Qt's card says it
  // (SessionCardDelegate::statusOf), not the state under it ("Stopped").
  const kind = badgeKind(s);
  if (kind === "unread" || kind === "muted-unread") return tr("New reply");
  const worked = elapsed(s);
  if (worked === undefined) return captionText(s.caption);
  const h = Math.floor(worked / 3600), m = Math.floor((worked % 3600) / 60), sec = worked % 60;
  return `${captionText(s.caption)} (${h ? `${h}h ${m}m` : seconds ? `${m}m ${String(sec).padStart(2, "0")}s` : `${m}m`})`;
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
  // The host reports the cache's lifetime, stretched by Keep cache warm (cache_ttl.kept_warm).
  const shown = presentState(stateFacts(s, online, visibleUnreadReply(s, computerFresh(s.computer))), now());
  const blocked = s.status === "attention" || s.status === "error";
  const key = shown.key;
  if ((key === "deps" || key === "build" || key === "cooling" || key === "cold" || key === "stopped")
    && !blocked && s.state !== "archived" && !(s.draft && (key === "cooling" || key === "cold"))) return { shown, kind: key, caption: shown.badge };
  if (key === "user" && shown.badge === tr("Needs you") && !blocked) return { shown, kind: "attention", caption: shown.badge };
  return { shown };
}

/**
 * Task 315 (F3): the detail header's pill (SessionsWindow.cpp:4932-4959) shows
 * the card's state: Waiting for dependencies, Building, Cooling, Cold and
 * Stopped unless the session needs action or is archived, and the lead's
 * "Needs you" unless it needs action. Undefined: the header keeps its status.
 */
export function headerState(s: Session): { kind: BadgeKind; caption: string; title: string } | undefined {
  const online = !computerOffline(s.computer);
  const shown = presentState(stateFacts(s, online, visibleUnreadReply(s, computerFresh(s.computer))), now());
  const key = shown.key;
  const needsYou = key === "user" && shown.badge === tr("Needs you") && !s.needsAction;
  if (needsYou) return { kind: "attention", caption: shown.badge, title: shown.reason };
  if ((key === "deps" || key === "build" || key === "cooling" || key === "cold" || key === "stopped") && !s.needsAction && s.state !== "archived")
    return { kind: key, caption: shown.badge, title: shown.reason };
  return undefined;
}

export function badgeKind(s: Session): BadgeKind {
  const status = statusOf(s);
  if (status === "attention") return quietAttention(s) ? "muted-attention" : "attention";
  // A supervised child's error is its parent's to handle: gray, like its other requests (zerus-rxz9k).
  if (status === "error" && s.quiet && !s.muted) return "muted-attention";
  const state = cardState(s).kind;
  if (state) return state;
  if (status === "error" || status === "working" || status === "background") return status;
  if (s.draft && status !== "offline") return "draft";
  if (visibleUnreadReply(s, computerFresh(s.computer))) return quietAttention(s) ? "muted-unread" : "unread";
  if (status === "scheduled" || status === "waiting" || status === "paused") return status;
  return "neutral";
}

// Edge mark: attention/error > draft > unread; muted rows turn it grey.
export function emphasis(s: Session): string | undefined {
  const status = statusOf(s);
  if (quietAttention(s) && !(s.quiet && !s.muted && s.reviewLater) && (status === "attention" || visibleUnreadReply(s, computerFresh(s.computer)))) return "muted";
  if (status === "attention" || s.reviewLater) return "attention";
  if (status === "error") return s.quiet && !s.muted ? "muted" : "error";
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

/**
 * A supervised child: its parent handles its requests (spec 3.4). A dim
 * arrow turning down and right, before the title: handled by the parent
 * above (owner, 2026-11-07, zerus-sa9qn.16; a bell read as notifications, a
 * crossed bell as muted).
 */
export function SupervisedMark(props: { session: Session }) {
  const name = () => props.session.parent?.name.split("/").pop() || props.session.parent?.name || "";
  return (
    <span class="sc-supervised" role="img" aria-label={tr("Supervised by %1", name())}
      title={tr("Supervised by %1\nOnly requests that need you notify you.", props.session.parent?.name ?? "")}>
      <Icon name="corner-down-right" size={13} />
    </span>
  );
}

// The prototype's agent artwork loads once, outside the entry chunk; until
// then, and for agents without artwork, tiles show the provider glyph.
const [agentLogos, setAgentLogos] = createSignal<Readonly<Record<string, readonly LogoPath[]>>>();
let logosRequested = false;
function logoOf(agent: AgentKind): readonly LogoPath[] | undefined {
  if (!logosRequested) {
    logosRequested = true;
    void import("../lib/agent-logos").then((module) => setAgentLogos(module.AGENT_LOGOS)).catch(() => {});
  }
  return agentLogos()?.[agent];
}

/** The large agent mark at the card's left, on a bordered tile (spec 3.1, the owner's prototype). */
export function AgentTile(props: { agent: AgentKind; size?: number }) {
  const size = () => props.size ?? 34;
  // 20 px artwork on the 34 px tile, 14 px on the compact row's 22 px tile.
  const mark = () => (size() <= 24 ? 14 : Math.round(size() * 0.59));
  return (
    <span class="agent-tile" style={{ "--agent": `var(--z-agent-${props.agent})`, "--tile": `${size()}px` }} title={AGENT_LABEL[props.agent]}>
      <Show when={logoOf(props.agent)} fallback={
        <svg width={mark()} height={mark()} viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" innerHTML={PROVIDER_GLYPH[props.agent]} />
      }>
        {(paths) => (
          <svg class="agent-logo" width={mark()} height={mark()} viewBox="0 0 24 24" aria-hidden="true">
            <For each={paths()}>{(path) => <path d={path[0]} fill={path[1]} />}</For>
          </svg>
        )}
      </Show>
    </span>
  );
}

/** A parent's child sessions: working of all, collapsing them (spec 3.3). */
export function TeamToggle(props: { team: Team; onToggle?: () => void }) {
  return (
    <button class="sc-team" classList={{ open: !props.team.collapsed }} aria-expanded={!props.team.collapsed}
      title={props.team.collapsed ? tr("Show subagents and child sessions") : tr("Hide subagents and child sessions")}
      onClick={(e) => { e.stopPropagation(); props.onToggle?.(); }} onDblClick={(e) => e.stopPropagation()}>
      <svg class="team-glyph" width="16" height="12" viewBox="0 0 16 12" fill="none" stroke="currentColor" stroke-width="1.2" aria-hidden="true">
        <circle cx="8" cy="3" r="2" /><circle cx="3" cy="5" r="1.6" /><circle cx="13" cy="5" r="1.6" />
        <path d="M4 11A4 3 0 0 1 12 11M0 10.5A3 2.4 0 0 1 4.5 9M16 10.5A3 2.4 0 0 0 11.5 9" />
      </svg>
      <span>{tr("%1 of %2", props.team.working, props.team.total)}</span>
      <Show when={props.team.asking}><span class="team-asking" title={tr("Child sessions waiting for their parent's answer")}>{tr("Asking: %1", props.team.asking ?? 0)}</span></Show>
      <Show when={props.team.needsYou}><span class="team-needs" title={tr("Child sessions with a request only you can answer")}>{tr("Needs you: %1", props.team.needsYou ?? 0)}</span></Show>
      <svg class="chev" width="8" height="6" viewBox="0 0 8 6" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M1 2L4 5L7 2" /></svg>
    </button>
  );
}

/** A child shown away from its parent: the parent's name, opening it when listed. */
export function FromParent(props: { parent: { name: string; key?: string }; onOpen?: (key: string) => void }) {
  const short = () => props.parent.name.split("/").pop() || props.parent.name;
  return (
    <Show when={props.parent.key} fallback={<span class="sc-from" title={tr("Started by %1\nThat session is not listed.", props.parent.name)}>{tr("from %1", short())}</span>}>
      {(key) => <button class="sc-from link" title={tr("Started by %1\nClick to open it.", props.parent.name)}
        onClick={(e) => { e.stopPropagation(); props.onOpen?.(key()); }} onDblClick={(e) => e.stopPropagation()}>{tr("from %1", short())}</button>}
    </Show>
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

/** The account's colored circle: its letter, or its emoji with emoji account marks (spec 3.2). */
export function AccountMarkCircle(props: { mark: AccountMark; size?: number }) {
  return (
    <span class="acct-mark" classList={{ emoji: props.mark.isEmoji }} role="img" aria-label={tr("Account: %1", props.mark.label)}
      style={{ "--mark": props.mark.color, "--mark-ink": props.mark.ink, "--mark-size": `${props.size ?? 16}px` }}>
      <span aria-hidden="true">{props.mark.glyph}</span>
    </span>
  );
}

const markTooltip = (s: Session, mark: AccountMark) =>
  [tr("Account: %1", mark.label), mark.machine ? tr("%1 on %2", AGENT_LABEL[s.agent], mark.machine) : AGENT_LABEL[s.agent]].join("\n");

/** The expanded card's account: the circle and the label. */
export function AccountChip(props: { session: Session }) {
  const mark = () => sessionMark(props.session);
  return (
    <Show when={mark()}>{(m) => (
      <span class="acct-chip" title={markTooltip(props.session, m())}>
        <AccountMarkCircle mark={m()} />
        <span class="acct-chip-label ell" aria-hidden="true">{m().label}</span>
      </span>
    )}</Show>
  );
}

type Git = NonNullable<Session["git"]>;
interface GitMark { icon: string; count?: string; tone: "muted" | "attention" | "incoming" | "synced" | "conflict" }

// GitStatusBadge::marks: changed files, outgoing, incoming, then unverified or synced.
function gitFreshness(g: Git, at = Date.now()) {
  const age = g.receivedAt === undefined ? 0 : at - g.receivedAt;
  const fresh = (g.localFresh ?? g.state !== "unknown") && age >= 0 && (g.streamed === true || age < 30_000);
  const verified = fresh && (g.remoteVerified ?? g.state !== "unknown")
    && (g.remoteAge ?? 0) + age / 1000 < 60 && g.countsKnown !== false;
  return { fresh, verified };
}
export function gitMarks(g: Git, at = Date.now()): GitMark[] {
  const { fresh, verified } = gitFreshness(g, at);
  const count = (n: number) => (n > 99 ? "99+" : String(n));
  const marks: GitMark[] = [];
  if (g.changed) marks.push({ icon: "git-diff", count: count(g.changed), tone: !fresh ? "muted" : g.conflicts ? "conflict" : "attention" });
  if (g.ahead) marks.push({ icon: "git-push", count: count(g.ahead), tone: verified ? "attention" : "muted" });
  if (g.behind) marks.push({ icon: "git-pull", count: count(g.behind), tone: verified ? "incoming" : "muted" });
  if (!verified) marks.push({ icon: "git-unknown", tone: "muted" });
  else if (!g.changed && !g.ahead && !g.behind && !g.detached && !g.unborn) marks.push({ icon: "git-synced", tone: "synced" });
  return marks;
}

export function gitTooltip(s: Session, at = Date.now()): string {
  const g = s.git!;
  const { fresh, verified } = gitFreshness(g, at);
  const ref = verified ? tr("verified upstream") : tr("last known upstream; remote unverified");
  return [
    tr("Git status (current checkout)"),
    g.root || s.gitRoot || s.cwdPath || s.cwd,
    s.branch ? tr("Branch: %1", s.branch) : "",
    g.upstream ? tr("Upstream: %1", g.upstream) : "",
    g.changedKnown !== false ? tr("%1 changed or new files (including staged and untracked files)", g.changed) : "",
    g.conflicts ? tr("%1 files with merge conflicts", g.conflicts) : "",
    g.aheadKnown !== false ? tr("%1 outgoing commits (%2)", g.ahead, ref) : "",
    g.behindKnown !== false ? tr("%1 incoming commits (%2)", g.behind, ref) : "",
    !fresh ? tr("Git status is unavailable or out of date.") : "",
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

/** One chip of line 2: what it is, its icon, count and tone (SessionRowChips::Chip). */
export interface RowChip { kind: "services" | "background" | "git" | "more"; icon: string; count?: string; tone: string; tip: string }

/** Qt SessionRowChips::width: the icon, the count and the padding, at the chips' 11 px. */
export const chipWidth = (chip: Pick<RowChip, "count">) => 4 + 16 + (chip.count ? 3 + chip.count.length * 7 : 0) + 4;

/**
 * Task 252 (Qt SessionRowChips::layout): the chips that fit `room`. When all
 * do not fit, git folds into one More chip before services or background
 * work go; then chips are dropped from the end until the rest fit.
 */
export function fitChips(chips: readonly RowChip[], room: number, more: Omit<RowChip, "kind" | "icon" | "tone">): RowChip[] {
  const total = (list: readonly RowChip[]) => Math.max(0, list.reduce((sum, chip) => sum + chipWidth(chip) + 4, 0) - 4);
  let shown = [...chips];
  if (total(shown) > room) {
    const folded: RowChip[] = [];
    let added = false;
    for (const chip of shown) {
      if (chip.kind !== "git") folded.push(chip);
      else if (!added) { folded.push({ kind: "more", icon: "more", tone: "muted", ...more }); added = true; }
    }
    shown = folded;
  }
  while (shown.length && total(shown) > room) shown.pop();
  return shown;
}

/** The chips of a row in Qt's order: services, background commands, then git. */
export function rowChips(s: Session, at = Date.now()): RowChip[] {
  const chips: RowChip[] = [];
  const services = s.services?.length ?? 0;
  if (services) chips.push({ kind: "services", icon: "settings", count: String(services), tone: "muted", tip: chipTooltip(s, "services") });
  const jobs = (s.backgroundJobs ?? 0) > 0 ? s.backgroundJobs! : 0;
  if (jobs) chips.push({ kind: "background", icon: "active", count: countText(jobs), tone: "bg", tip: chipTooltip(s, "background") });
  if (s.git && s.status !== "ended") {
    for (const mark of gitMarks(s.git, at)) chips.push({ kind: "git", icon: mark.icon, count: mark.count, tone: mark.tone, tip: chipTooltip(s, GIT_CHIP[mark.icon] ?? "unchecked") });
  }
  return chips;
}

/**
 * The chips right of the folder (SessionRowChips): services, background commands, then
 * git (changed files, outgoing, incoming, in sync or not checked). Each names what it
 * counts and a click opens where it lives. Task 252: in a narrow line they fit as
 * Qt's do: git folds into one More chip (the full git tooltip) before the machine
 * badge is squeezed.
 */
export function RowChips(props: { session: Session; room?: number; onChip?: (kind: "services" | "background" | "git") => void }) {
  const s = () => props.session;
  const click = (kind: RowChip["kind"]) => (e: MouseEvent) => { e.stopPropagation(); props.onChip?.(kind === "more" ? "git" : kind); };
  const chips = () => fitChips(rowChips(s(), now() * 1000), props.room ?? Infinity, { tip: s().git ? gitTooltip(s(), now() * 1000) : "" });
  return (
    <Show when={chips().length}>
      <span class="gmarks">
        <For each={chips()}>{(chip) => (
          <button type="button" class={`gmark chip t-${chip.tone}`} classList={{ counted: !!chip.count }} data-chip={chip.kind} title={chip.tip} onClick={click(chip.kind)} onDblClick={(e) => e.stopPropagation()}>
            <Icon name={chip.icon} size={16} />
            <Show when={chip.count}><span>{chip.count}</span></Show>
          </button>
        )}</For>
      </span>
    </Show>
  );
}

export function GitMarks(props: { session: Session }) {
  const marks = createMemo(() => props.session.git ? gitMarks(props.session.git, now() * 1000) : [], [], {
    equals: (a, b) => a.length === b.length && a.every((m, i) => m.icon === b[i].icon && m.count === b[i].count && m.tone === b[i].tone),
  });
  const tip = steadyTitle(() => (props.session.git ? gitTooltip(props.session, now() * 1000) : ""));
  return (
    <span class="gmarks" title={tip.title()} onPointerEnter={tip.enter} onPointerLeave={tip.leave}>
      <For each={marks()}>{(m) => (
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
    <span class="qchip" classList={{ muted: quietAttention(props.session) }} title={tr(optionalCount() === 1 ? "%1 optional question waiting\nThe agent can continue without an answer. Answer or skip it in Activity.\nClick to open." : "%1 optional questions waiting\nThe agent can continue without an answer. Answer or skip it in Activity.\nClick to open.", optionalCount())}>
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
export function TeamStrip(props: { team: TeamSummary; expanded: boolean; onToggle: () => void; shown?: number }) {
  const status = () => TEAM_STATUS[props.team.status] ?? props.team.status;
  const fold = () => (props.expanded ? tr("Fold team") : tr("Unfold team"));
  // Task 455: the count is the member rows the list shows under the lead (live
  // members, wherever their sessions are filed), else the team's members.
  const members = () => props.shown ?? props.team.members.filter((member) => !member.lead).length;
  const count = () => (props.expanded ? tr("%1 members", members()) : tr("%1 hidden", members()));
  // Task 425: the session list's own class names (sc-team-*): the Team tab's
  // .team-strip, .team-status and .team-members rules must not reach them.
  // Task 452 (owner, 2026-11-09): the fold control is a chevron button at the
  // strip's right end, beside the member count, and the whole strip folds or
  // unfolds the team on a click; the button keeps the keyboard.
  return (
    <div class="sc-team-strip" data-team-strip data-folded={props.expanded ? undefined : "1"}
      title={`${props.team.name}
${tr("%1 of %2 phases finished; discarded drafts do not count", props.team.phases_done, props.team.phases_total)}
${fold()}`}
      onClick={(e) => { e.stopPropagation(); props.onToggle(); }} onDblClick={(e) => e.stopPropagation()}>
      <TeamBar label={`${props.team.phase ? `P${props.team.phase} ` : ""}${props.team.phase_done}/${props.team.phase_total}`}
        done={props.team.phase_done} total={props.team.phase_total} />
      <TeamBar label={`${props.team.phases_done}/${props.team.phases_total}`} done={props.team.phases_done}
        total={props.team.phases_total} muted />
      <span class={props.team.status === "stuck" ? "sc-team-status stuck" : "sc-team-status"}>{tr(status())}</span>
      <Show when={props.team.attention > 0}><span class="sc-team-answers">?{props.team.attention}</span></Show>
      <span class="grow" />
      <span class="sc-team-count">{count()}</span>
      <button type="button" class="icon-btn small sc-team-toggle" data-team-toggle aria-expanded={props.expanded}
        aria-label={fold()} title={fold()}
        onClick={(e) => { e.stopPropagation(); props.onToggle(); }} onDblClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") e.stopPropagation(); }}>
        <Icon name="chevron-down" size={16} class="chev" />
      </button>
    </div>
  );
}

/** Qt's full row tooltip (SessionsWindowRows.cpp:692-716): identity, state, meta, model, attention. */
export function fullTooltip(s: Session, team?: TeamSummary): string {
  const lines = [
    s.name,
    `${s.machine} / ${s.agent} / ${statusCaption(s, false)}`,
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
  team?: Team;
  onToggleTeam?: () => void;
  fromParent?: { name: string; key?: string };
  onOpenParent?: (key: string) => void;
  /** A Teams lead's team: its strip, and the fold of its members under it. */
  teamSummary?: TeamSummary;
  onToggleMembers?: () => void;
  /** Task 455: the member rows listed under this lead. */
  teamMembersShown?: number;
  /** A Teams member listed under its lead: indented, first and last of the group get the connector's ends. */
  member?: boolean;
  depth?: number;
  teamFirst?: boolean;
  teamLast?: boolean;
}) {
  const s = () => props.session;
  const kind = () => badgeKind(s());
  const state = () => cardState(s());
  const line = () => lineThree(s());
  // Task 252 (Qt SessionRowChips::layout): the chips' room on line 2, between
  // the folder (its icon and 48 px of text, as Qt keeps) and the machine badge.
  const [chipRoom, setChipRoom] = createSignal(Infinity);
  const measureLine = (el: HTMLDivElement) => {
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      const badge = el.querySelector<HTMLElement>(".mbadge");
      setChipRoom(Math.max(0, el.clientWidth - (badge?.offsetWidth ?? 0) - 18 - 48 - 8));
    });
    observer.observe(el);
    onCleanup(() => observer.disconnect());
  };
  return (
    <div class="scard tiled" classList={{ selected: props.selected, offline: statusOf(s()) === "offline", muted: !!s().muted,
      "sc-team-lead": !!props.teamSummary, "sc-member": !!props.member, "sc-member-first": !!props.teamFirst, "sc-member-last": !!props.teamLast }}
      // Task #330: a member's box spans the row (its content is indented by
      // the stylesheet), so the hover highlight and the card reach the pane
      // edge; the old inline left shrank both and left the indent unpainted.
      // Task 425: a member's card is an ordinary card inside the tree's indent
      // (the stylesheet places it); the connector runs in the indent beside it.
      style={props.member ? undefined : { left: "2px" }} data-emphasis={emphasis(s())}
      title={fullTooltip(s(), props.teamSummary)}>
      <span class="sc-tile"><AgentTile agent={s().agent} /></span>
      <div class="sc-l1">
        {/* Supervision and Mute are independent: the arrow before the name,
            the crossed bell right after it (owner, 2026-11-08). */}
        <Show when={s().parent?.state === "live"}><SupervisedMark session={s()} /></Show>
        {/* Task 455 (attempt 3): a member card is narrower by the tree's indent;
            its title gives way first (an ellipsis) so the state badge and the
            machine chip after it keep their whole box. */}
        <span class="sc-title ell" style={titleReserve(s().title, props.member ? 0 : 72)} title={s().name}>{s().title}</span>
        <Show when={s().muted}><Icon name="mute" size={14} class="sc-mute" /></Show>
        <StatusBadge kind={kind()} caption={statusCaption(s())} heat={state().shown.heat} title={state().kind ? state().shown.tooltip : undefined} />
        <Show when={visibleUnreadReply(s(), computerFresh(s().computer)) && (s().status === "working" || s().status === "attention" || s().draft)}>
          <span class="sc-extra"><StatusBadge kind={quietAttention(s()) ? "muted-unread" : "unread"} caption="" iconOnly /></span>
        </Show>
        <OptionalQuestionsChip session={s()} />
        <span class="grow" />
      </div>
      <div class="sc-l2" ref={measureLine}>
        <Icon name={s().branch ? "git-branch" : "folder"} size={14} class="sc-meta-ico" />
        <span class="sc-meta ell">{folderOf(s())}{s().branch ? ` / ${s().branch}` : ""}</span>
        <span class="grow" />
        <RowChips session={s()} room={chipRoom()} onChip={props.onChip} />
        <MachineBadge alias={s().machine} computer={s().computer} offline={statusOf(s()) === "offline"} />
      </div>
      <div class="sc-l3" classList={{ "s-deps": line().tone === "deps" }}>
        <Show when={props.fromParent}>{(parent) => <FromParent parent={parent()} onOpen={props.onOpenParent} />}</Show>
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
        <AccountChip session={s()} />
        <span class="grow" />
        <Show when={!props.team && (s().subagents ?? []).length}>
          <AgentsCounter label={agentsLabel(s())} expandable={props.expandable} expanded={props.expanded} onToggle={props.onToggleChildren} />
        </Show>
        <Show when={props.team}>{(team) => <TeamToggle team={team()} onToggle={props.onToggleTeam} />}</Show>
      </div>
      <Show when={props.teamSummary}><TeamStrip team={props.teamSummary!} expanded={props.expanded} shown={props.teamMembersShown} onToggle={() => props.onToggleMembers?.()} /></Show>
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

/**
 * Native subagent row (40 px), indented under its parent: a compact session
 * row that never expands (owner, 2026-11-07, zerus-sa9qn.16): the agent's
 * tile, the parent's account mark, the name and the status, then the expand
 * button's empty slot; the summary is its tooltip.
 */
export function ChildCard(props: { parent: Session; child: Subagent; last: boolean; first: boolean; selected: boolean }) {
  const status = () => childStatus(props.child, currentChildren(props.parent, !computerOffline(props.parent.computer)));
  return (
    <div class="child-row" classList={{ last: props.last, first: props.first }}>
      <i class="tree" aria-hidden="true" />
      <div class="scard compact child" classList={{ selected: props.selected }} title={props.child.summary || undefined}>
        <span class="cc-tile"><AgentTile agent={props.parent.agent} size={22} /></span>
        {/* A subagent runs on its parent's account (owner, 2026-11-08). */}
        <Show when={sessionMark(props.parent)}>{(m) => <span class="cc-mark" title={markTooltip(props.parent, m())}><AccountMarkCircle mark={m()} size={14} /></span>}</Show>
        <span class="sc-title ell" style={titleReserve(props.child.name, 60)}>{props.child.name}</span>
        <StatusBadge kind={status().kind} caption={status().caption} />
        {/* The expand button's slot, empty, so the status lines up with other rows. */}
        <span class="cc-expand cc-slot" aria-hidden="true" />
      </div>
    </div>
  );
}

/** The text of a parent's child sessions summary, for its compact row and its accessible description. */
export function teamSummary(team: Team): string[] {
  return [
    tr("%1 of %2 working", team.working, team.total),
    team.needsYou ? tr("Needs you: %1", team.needsYou) : "",
    team.asking ? tr("Asking: %1", team.asking) : "",
    team.errors ? tr("Errors: %1", team.errors) : "",
  ].filter(Boolean);
}

/**
 * A compact parent's second line: its child sessions at a glance, collapsing
 * them. The chevron keeps its place; on a narrow row the counters shorten to
 * their numbers (zerus-sa9qn.11), and the button and row descriptions keep
 * every count in words.
 */
/**
 * Whether the summary shows short counters (zerus-sa9qn.12): it shortens once
 * its long labels overflow, in any language, and lengthens again only when
 * the width they needed is available. `needed` is that width (0 while long).
 */
export function fitCounters(state: { narrow: boolean; needed: number }, scrollWidth: number, clientWidth: number): { narrow: boolean; needed: number } {
  if (!state.narrow) return scrollWidth > clientWidth + 1 ? { narrow: true, needed: scrollWidth } : state;
  return clientWidth >= state.needed ? { narrow: false, needed: 0 } : state;
}

function CompactTeam(props: { team: Team; onToggle?: () => void }) {
  const title = () => (props.team.collapsed ? tr("Show subagents and child sessions") : tr("Hide subagents and child sessions"));
  // Short counters when the long ones do not fit; the container query below
  // about 300 px is the first approximation before any measurement.
  let body: HTMLSpanElement | undefined;
  let fit = { narrow: false, needed: 0 };
  const [narrow, setNarrow] = createSignal(false);
  const measure = () => {
    if (!body) return;
    fit = fitCounters(fit, body.scrollWidth, body.clientWidth);
    setNarrow(fit.narrow);
  };
  onMount(() => {
    if (!body || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(body);
    onCleanup(() => observer.disconnect());
  });
  // Other counts or language: measure the long labels again.
  createEffect(on(() => [props.team.working, props.team.total, props.team.needsYou, props.team.asking, props.team.errors, tr("Asking: %1", 0)], () => {
    fit = { narrow: false, needed: 0 };
    setNarrow(false);
    queueMicrotask(measure);
  }, { defer: true }));
  const counter = (kind: string, count: number | undefined, text: string) => (
    <Show when={count}><span class={kind} title={text}><span class="long">{text}</span><span class="short" aria-hidden="true">{count}</span></span></Show>
  );
  return (
    <button class="cc-team" classList={{ open: !props.team.collapsed, narrow: narrow() }} aria-expanded={!props.team.collapsed}
      title={title()} aria-label={[title(), ...teamSummary(props.team)].join("\n")}
      onClick={(e) => { e.stopPropagation(); props.onToggle?.(); }} onDblClick={(e) => e.stopPropagation()}>
      <span class="cc-team-body" ref={body}>
        <svg class="team-glyph" width="16" height="12" viewBox="0 0 16 12" fill="none" stroke="currentColor" stroke-width="1.2" aria-hidden="true">
          <circle cx="8" cy="3" r="2" /><circle cx="3" cy="5" r="1.6" /><circle cx="13" cy="5" r="1.6" />
          <path d="M4 11A4 3 0 0 1 12 11M0 10.5A3 2.4 0 0 1 4.5 9M16 10.5A3 2.4 0 0 0 11.5 9" />
        </svg>
        <span class="cc-team-count">
          <span class="long">{tr("%1 of %2 working", props.team.working, props.team.total)}</span>
          <span class="short" aria-hidden="true">{tr("%1 of %2", props.team.working, props.team.total)}</span>
        </span>
        {counter("team-needs", props.team.needsYou, tr("Needs you: %1", props.team.needsYou ?? 0))}
        {counter("team-asking", props.team.asking, tr("Asking: %1", props.team.asking ?? 0))}
        {counter("team-errors", props.team.errors, tr("Errors: %1", props.team.errors ?? 0))}
      </span>
      <svg class="chev" width="8" height="6" viewBox="0 0 8 6" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M1 2L4 5L7 2" /></svg>
    </button>
  );
}

/**
 * Compact (collapsed) row, 40 px: title, then status, optional questions,
 * provider and mute at the right, then the expand button. A parent of child
 * sessions takes a second line with their summary (62 px).
 */
/**
 * What a compact row shows only in part (Qt's compactDescription,
 * SessionCardDelegate.h:125-128): its title, status and provider, which the
 * row draws as an icon only. The session's name follows a title that differs.
 */
export function compactDescription(s: Session): string {
  return [s.title, s.name !== s.title ? s.name : "", statusCaption(s, false), AGENT_LABEL[s.agent]].filter(Boolean).join("\n");
}

export function CompactCard(props: { session: Session; selected: boolean; onExpand: () => void; team?: Team; onToggleTeam?: () => void }) {
  const s = () => props.session;
  // Tooltips hold still under the pointer (zerus-5ryrx).
  const described = steadyTitle(() => compactDescription(s()));
  const expand = steadyTitle(() => `${tr("Expand session row")}\n${compactDescription(s())}`);
  return (
    <div class="scard compact" classList={{ selected: props.selected, two: !!props.team }} data-emphasis={emphasis(s())}
      aria-description={tr("Collapsed session row. Press Right to expand.")}>
      <div class="cc-line">
      <span class="cc-tile"><AgentTile agent={s().agent} size={22} /></span>
      <Show when={sessionMark(s())}>{(m) => <span class="cc-mark" title={markTooltip(s(), m())}><AccountMarkCircle mark={m()} size={14} /></span>}</Show>
      <Show when={s().parent?.state === "live"}><SupervisedMark session={s()} /></Show>
      <span class="sc-title ell" style={titleReserve(s().title, 60)} title={described.title()} onPointerEnter={described.enter} onPointerLeave={described.leave}>{s().title}</span>
      <Show when={s().muted}><Icon name="mute" size={14} class="sc-mute" /></Show>
      <StatusBadge kind={badgeKind(s())} caption={statusCaption(s())} />
      <OptionalQuestionsChip session={s()} />
      <button class="cc-expand" title={expand.title()} onPointerEnter={expand.enter} onPointerLeave={expand.leave} aria-description={compactDescription(s())}
        onClick={(e) => { e.stopPropagation(); props.onExpand(); }} onDblClick={(e) => e.stopPropagation()}>
        <Icon name="expand-row" size={16} />
      </button>
      </div>
      <Show when={props.team}>{(team) => <CompactTeam team={team()} onToggle={props.onToggleTeam} />}</Show>
    </div>
  );
}
