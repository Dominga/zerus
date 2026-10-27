import { Show } from "solid-js";
import { AgentGlyph, AGENT_LABEL, Icon } from "../../lib/icons";
import { machineOf } from "./model";
import { projects } from "../../mock/data";
import type { AgentKind, Session, Status, Subagent } from "../../mock/types";

// Card anatomy follows tray/src/SessionCardDelegate.h paintCard():
//   line 1  title (14px/600) + status badge (20px) [+ unread] [+ questions] ... agent badge (18px)
//   line 2  folder/branch meta (11px, elided in the middle) ... git badge, machine badge (18px)
//   line 3  activity excerpt (12px)
//   footer  model (10px) + effort badge (16px) ... last change | agents counter
// Cards are transparent rows; hover and selection paint a neutral surface,
// and attention/unread/draft/error rows carry a 3px edge mark.

export function statusCaption(s: Session): string {
  switch (s.status) {
    case "working": return `Working (${s.turnStartedMin ?? 0}m ${String(((s.turnStartedMin ?? 0) * 7) % 60).padStart(2, "0")}s)`;
    case "attention": return s.attention === "approval" ? "Needs approval" : s.attention === "question" ? "Needs input" : "Needs attention";
    case "background": return s.backgroundJobs && s.backgroundJobs > 1 ? `Background (${s.backgroundJobs})` : "Background";
    case "scheduled": return `Scheduled ${s.scheduleNext ?? ""}`.trim();
    case "ready": return s.draft ? "Draft" : s.unread ? "New reply" : "Ready";
    case "error": return "Error";
    case "offline": return "Offline";
    case "paused": return "Paused";
    case "waiting": return "Waiting (3m 10s)";
    case "starting": return "Starting…";
    case "ended": return "Archived";
  }
}

// Badge precedence (SessionCardDelegate::statusOf): attention/error, working or
// background, draft, new reply, scheduled, waiting, paused, neutral.
export function badgeKind(s: Session): string {
  const muted = s.muted ? "muted-" : "";
  if (s.status === "attention") return `${muted}attention`;
  if (s.status === "error" || s.status === "working" || s.status === "background") return s.status;
  if (s.draft && s.status !== "offline") return "draft";
  if (s.unread) return `${muted}unread`;
  if (s.status === "ready" || s.status === "offline" || s.status === "ended") return "neutral";
  return s.status;
}

// Edge mark: attention/error > draft > unread; muted rows turn it grey.
export function emphasis(s: Session): string | undefined {
  if (s.muted && (s.status === "attention" || s.unread)) return "muted";
  if (s.status === "attention" || s.reviewLater) return "attention";
  if (s.status === "error") return "error";
  if (s.draft) return "draft";
  if (s.unread) return "unread";
  return undefined;
}

export function StatusBadge(props: { kind: string; caption: string }) {
  return (
    <span class={`sbadge k-${props.kind}`}>
      <i class="sb-ico" aria-hidden="true" />
      <span class="sb-label">{props.caption}</span>
    </span>
  );
}

export function AgentBadge(props: { agent: AgentKind; narrow?: boolean }) {
  return (
    <span class="agent-badge" classList={{ narrow: props.narrow }} style={{ "--agent": `var(--z-agent-${props.agent})` }} title={AGENT_LABEL[props.agent]}>
      <AgentGlyph agent={props.agent} size={12} />
      <span class="ab-label">{AGENT_LABEL[props.agent]}</span>
    </span>
  );
}

export function MachineBadge(props: { alias: string; vivid?: boolean; offline?: boolean }) {
  const m = () => machineOf(props.alias);
  return (
    <span class="entity mbadge" classList={{ vivid: props.vivid, offline: m()?.connection === "offline" }}
      style={{ "--entity": `var(--z-entity-palette-${m()?.colorIndex ?? 0})` }} title={m()?.thisComputer ? `${props.alias} (this computer)` : props.alias}>
      {props.alias}{props.offline ? " (offline)" : ""}
    </span>
  );
}

function GitBadge(props: { git: NonNullable<Session["git"]> }) {
  return (
    <span class="gbadge" classList={{ [props.git.state]: true }} title="Git status: changed files, commits to push, commits to pull">
      <Show when={props.git.changed}><span><Icon name="git-compare-arrows" size={11} />{props.git.changed}</span></Show>
      <Show when={props.git.ahead}><span><Icon name="arrow-up" size={11} />{props.git.ahead}</span></Show>
      <Show when={props.git.behind}><span><Icon name="arrow-down" size={11} />{props.git.behind}</span></Show>
      <Show when={props.git.state === "synced"}><span><Icon name="check" size={11} /></span></Show>
    </span>
  );
}

function detailText(s: Session): string {
  if (s.summary) return s.summary;
  if (s.unread) return "Response ready to read";
  if (s.status === "ready") return "Waiting for your next message";
  if (s.status === "paused") return "Resume to continue";
  return "";
}

export function SessionCard(props: {
  session: Session;
  selected: boolean;
  expandable: boolean;
  expanded: boolean;
  onToggleChildren: () => void;
}) {
  const s = () => props.session;
  const project = () => projects.find((p) => p.id === s().project);
  const folder = () => (project()?.folders.find((f) => f.machine === s().machine)?.name ?? s().cwd.split(/[\\/]/).pop() ?? "").toLowerCase().split(" ")[0];
  const kids = () => s().subagents ?? [];
  const activeKids = () => kids().filter((k) => k.state === "working").length;
  return (
    <div class="scard" classList={{ selected: props.selected, offline: s().status === "offline" }} data-emphasis={emphasis(s())}>
      <div class="sc-l1">
        <Show when={s().muted}><Icon name="bell-off" size={14} class="sc-muted" /></Show>
        <span class="sc-title ell">{s().name}</span>
        <StatusBadge kind={badgeKind(s())} caption={statusCaption(s())} />
        <Show when={s().unread && (s().status === "working" || s().status === "attention" || s().draft)}><span class="sbadge k-unread icon-only" title="New reply"><i class="sb-ico" /></span></Show>
        <Show when={s().optionalQuestions}><span class="qchip" title="Optional question waiting. The agent can continue without an answer. Click to open."><Icon name="circle-question-mark" size={13} />{s().optionalQuestions}</span></Show>
        <span class="grow" />
        <AgentBadge agent={s().agent} />
      </div>
      <div class="sc-l2">
        <Icon name={s().branch ? "git-branch" : "folder"} size={13} class="sc-meta-ico" />
        <span class="sc-meta ell">{folder()}{s().branch ? ` / ${s().branch}` : ""}</span>
        <span class="grow" />
        <Show when={s().git}>{(g) => <GitBadge git={g()} />}</Show>
        <MachineBadge alias={s().machine} offline={s().status === "offline"} />
      </div>
      <div class="sc-l3">
        <Show when={s().reviewLater}><Icon name="bell" size={13} class="sc-review later" /></Show>
        <span class="ell">{detailText(s())}</span>
      </div>
      <div class="sc-foot">
        <span class="sc-model ell">{s().model}</span>
        <Show when={s().effort}><span class="sc-effort">{s().effort}</span></Show>
        <span class="grow" />
        <Show when={s().status !== "working" && !kids().length}><span class="sc-modified">{s().lastEvent}</span></Show>
        <Show when={kids().length}>
          <button class="sc-kids" classList={{ open: props.expanded }} onClick={(e) => { e.stopPropagation(); props.onToggleChildren(); }}
            aria-expanded={props.expanded} title={props.expanded ? "Hide agents" : "Show agents"}>
            <Icon name="users" size={12} />
            <span>{activeKids()}/{kids().length}</span>
            <Icon name="chevron-down" size={12} class="chev" />
          </button>
        </Show>
      </div>
    </div>
  );
}

const CHILD_STATUS: Record<Subagent["state"], { kind: Status | "unread"; caption: string }> = {
  working: { kind: "working", caption: "Working" },
  ready: { kind: "ready", caption: "Ready" },
  attention: { kind: "attention", caption: "Needs input" },
};

export function ChildCard(props: { parent: Session; child: Subagent; last: boolean; selected: boolean }) {
  return (
    <div class="child-row" classList={{ last: props.last }}>
      <i class="tree" aria-hidden="true" />
      <div class="scard child" classList={{ selected: props.selected }}>
        <div class="sc-l1">
          <span class="sc-title ell">{props.child.name}</span>
          <StatusBadge kind={CHILD_STATUS[props.child.state].kind} caption={CHILD_STATUS[props.child.state].caption} />
          <span class="grow" />
          <AgentBadge agent={props.parent.agent} />
        </div>
        <div class="sc-child-detail ell">{props.child.summary}</div>
      </div>
    </div>
  );
}

export function CompactCard(props: { session: Session; selected: boolean; onExpand: () => void }) {
  const s = () => props.session;
  return (
    <div class="scard compact" classList={{ selected: props.selected }} data-emphasis={emphasis(s())}>
      <span class="sc-title ell">{s().name}</span>
      <StatusBadge kind={badgeKind(s())} caption={statusCaption(s())} />
      <span class="grow" />
      <AgentBadge agent={s().agent} narrow />
      <button class="icon-btn small" title="Show full card" onClick={(e) => { e.stopPropagation(); props.onExpand(); }}>
        <Icon name="maximize-2" size={12} />
      </button>
    </div>
  );
}
