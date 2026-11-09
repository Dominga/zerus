import type { Session, Subagent } from "./types";

export type SubagentMap = Record<string, Record<string, unknown>>;

const text = (value: unknown) => typeof value === "string" ? value : "";
const simplified = (value: unknown) => text(value).trim().replace(/\s+/g, " ").slice(0, 240);
export const childState = (child: Record<string, unknown>) => text(child.display_state) || text(child.state);

/** Qt/Android currentActivity: recorded children never establish current work. */
export const currentChildren = (row: Session, online: boolean) => online && !row.archiveId
  && row.state === "running" && row.processState !== "exited" && row.activity !== "unknown"
  && !(row.conversationState === "ended" && row.activity !== "idle");
export const workingChildren = (row: Session, online: boolean) => currentChildren(row, online)
  ? (row.subagents ?? []).filter((child) => child.state === "working").length : 0;

/** SessionPresentation::inactiveChild, including grouped count evidence. */
export function childInactive(child: Record<string, unknown>, countsComplete = false): boolean {
  const state = childState(child);
  const inactive = ["finished", "idle", "ready", "error", "ended"].includes(state);
  if (state && !inactive) return false;
  if (child.group === true) return countsComplete && typeof child.active_count === "number" && child.active_count === 0;
  return inactive;
}

const validId = (id: string) => id.length > 0 && id.length <= 160 && !/[/\\\u0000-\u001f\u007f]/.test(id);
/** A child conversation ID search can match: at most 256 characters and no control character (C0, DEL or C1). */
const validConversation = (id: string) => id.length > 0 && id.length <= 256 && !/\p{Cc}/u.test(id);

/** Stable native-child rows, never the hook-profile fallback. */
export function childRows(map: SubagentMap | undefined, countsComplete = false): Subagent[] {
  return Object.entries(map ?? {}).filter(([id, child]) => validId(id) && child && typeof child === "object" && !Array.isArray(child))
    .sort(([a], [b]) => a.localeCompare(b)).map(([id, child]) => {
      const raw = childState(child);
      const state = raw === "working" ? "working" : ["approval", "input", "attention"].includes(raw) ? "attention" : raw === "error" ? "error" : raw === "paused" ? "paused" : raw === "ended" ? "ended" : raw === "unknown" ? "unknown" : "ready";
      const label = typeof child.label === "string" ? child.label : typeof child.name === "string" ? child.name : "";
      const detail = typeof child.detail === "string" ? child.detail : child.task;
      const conversation = typeof child.conversation_id === "string" && validConversation(child.conversation_id) ? child.conversation_id : undefined;
      return {
        id,
        conversationId: conversation,
        name: label || id,
        state,
        summary: simplified(detail),
        inactive: childInactive(child, countsComplete),
        group: child.group === true,
      };
    });
}

/** Task 262: a child that works now (Qt's inspector: working, active members or an approval). */
const running = (child: Subagent) => child.state === "working" || child.state === "attention";

/**
 * Qt's side panel (SessionsWindow::renderDetails, owner 2026-10-28): while the
 * children are current, the finished ones fold into a count ("N finished,
 * show") unless the owner opened them; profile groups keep their rows.
 * Running children list first, each part in its own order.
 */
export function foldedChildren(children: readonly Subagent[], current: boolean, open: boolean): { shown: Subagent[]; finished: number } {
  const ordered = [...children.filter(running), ...children.filter((child) => !running(child))];
  if (!current) return { shown: [...children], finished: 0 };
  const finished = ordered.filter((child) => !running(child) && !child.group).length;
  return { shown: open ? ordered : ordered.filter((child) => running(child) || child.group), finished };
}

/** The side panel's fold of a session's finished children (one entry per session in `workspace/inspectorClosed`). */
export const finishedFoldId = (sessionKey: string) => `finished:${sessionKey}`;

/** Qt's selected-inspection roster override, only for the same fresh conversation. */
export function rosterFor(row: Session, details?: Record<string, unknown>): Subagent[] {
  const inspected = details?.subagents;
  const fresh = (typeof details?.last_event_at === "number" ? details.last_event_at : 0) >= (row.updatedAt ?? 0);
  const same = !row.conversationId || details?.conversation_id === row.conversationId;
  if (inspected && typeof inspected === "object" && !Array.isArray(inspected) && fresh && same) {
    return childRows(inspected as SubagentMap, details?.subagent_counts_complete === true);
  }
  return row.subagents ?? [];
}

/**
 * Plan & agents (owner, 2026-11-08, T-ug5hxx, zerus-5ffek): the IDs kept in
 * view, the working ones and those waiting for an answer while the session is
 * current, with the one open in Activity, and the IDs folded as finished.
 */
export function splitSubagents(children: readonly Subagent[], current: boolean, open?: string) {
  const active: string[] = [], finished: string[] = [];
  for (const child of children) ((current && (child.state === "working" || child.state === "attention")) || child.id === open ? active : finished).push(child.id);
  return { active, finished };
}
