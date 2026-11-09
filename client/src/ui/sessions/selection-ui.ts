import type { MenuEntry } from "../../store/ui";
import type { Row } from "./model";
import type { Session } from "./types";
import { MOVE_WAIT, SELECTION_ACTIONS, selectionText, type SelectionAction, type SelectionRow } from "./selection";
import { translate, tr, trNoop, type TranslationArgument } from "../../i18n";

// The selection bar and the selection context menu, after Qt's
// SessionsWindow::setupSelectionActions, updateSelectionActions and
// populateSelectionMenu (SessionSelection.cpp 110-218). Pure models: the
// components in SessionList.tsx render them, store/batch.ts runs the actions.

// "%2 — %3 of %n": the action, its count and the selection, which sets the plural form (zerus-wi352).
const TOOL_HINT = trNoop("%2 — %3 of %n selected session(s)");
const HEADING = trNoop("%n selected session(s)");

/**
 * A text about `selected` sessions in the interface language: Russian takes
 * its plural form from `selected`; English says "1 selected session" and
 * "2 selected sessions". `rest` fills %2 onwards.
 */
function selectedText(source: string, one: string, many: string, selected: number, ...rest: TranslationArgument[]): string {
  const text = tr(source, selected, ...rest);
  return text === translate("en", source, selected, ...rest) ? translate("en", selected === 1 ? one : many, selected, ...rest) : text;
}

/** Qt's selectionActionIcon, as client icon names (all are in the Qt glyph set). */
export const SELECTION_ICONS: Record<SelectionAction, string> = {
  mark_read: "read-all",
  review_later: "attention",
  mute: "bell-off",
  unmute: "bell",
  compact: "compact-row",
  expand: "expand-row",
  pause: "pause",
  resume: "play",
  archive: "archived",
  forget: "trash",
  terminate: "stop",
  move: "folder-move",
};

/**
 * Whether an archived row's Resume (restore) and Forget count: true since
 * store/restores.ts binds the archive calls (Qt counts them as any other row).
 */
export const ARCHIVE_ACTIONS_BOUND = true;

/** A row counts for an action when it applies and its call is bound. */
export function countable(s: Pick<SelectionRow, "state">, a: SelectionAction, applies: boolean, archiveBound = ARCHIVE_ACTIONS_BOUND): boolean {
  if (!applies) return false;
  return archiveBound || s.state !== "archived" || (a !== "resume" && a !== "forget");
}

/**
 * One bell tool: Mute while any selected session is unmuted, else Unmute
 * (zerus-qhtxf). The direction comes from every selected row's muted flag,
 * whether or not its setting can change now (zerus-qhtxf.1).
 */
const hiddenBell = (a: SelectionAction, anyUnmuted: boolean) => (a === "unmute" && anyUnmuted) || (a === "mute" && !anyUnmuted);

/** Whether any selected session is unmuted: the bell tool's direction. */
export const anyUnmuted = (rows: readonly Pick<SelectionRow, "muted">[]) => rows.some((s) => !s.muted);

/** Compact and Expand only change the presentation, so a running batch does not block them. */
export const presentation = (a: SelectionAction) => a === "compact" || a === "expand";

export interface SelectionTool {
  action: SelectionAction;
  icon: string;
  /** "{action} ({count})" */
  text: string;
  /** "{action} — {count} of {n} selected sessions" */
  hint: string;
  count: number;
  enabled: boolean;
  danger: boolean;
}

/**
 * The bar's visible tools in Qt's order: a tool shows when its count is above
 * 0. `unmuted` says whether any selected session is unmuted (`anyUnmuted`);
 * `moveRefusal` why the organization takes no edit now (zerus-5wmh.18.1).
 */
export function barTools(count: (a: SelectionAction) => number, selected: number, batchRunning: boolean, unmuted = true, moveRefusal?: string): SelectionTool[] {
  const tools: SelectionTool[] = [];
  for (const action of SELECTION_ACTIONS) {
    const n = count(action);
    if (n <= 0 || hiddenBell(action, unmuted)) continue;
    // Move waits for a lifecycle batch on the selection, then for project sync.
    const blocked = action === "move" ? (batchRunning ? tr(MOVE_WAIT) : moveRefusal) : undefined;
    tools.push({
      action,
      icon: SELECTION_ICONS[action],
      text: `${selectionText(action)} (${n})`,
      hint: blocked ?? selectedText(TOOL_HINT, "%2 — %3 of %n selected session", "%2 — %3 of %n selected sessions", selected, selectionText(action), n),
      count: n,
      enabled: action === "move" ? !blocked : presentation(action) || !batchRunning,
      danger: action === "forget" || action === "terminate",
    });
  }
  return tools;
}

/** The tools that fit `capacity` buttons stay on the bar; the rest go under the overflow button. */
export function splitTools<T>(tools: readonly T[], capacity: number): { shown: T[]; more: T[] } {
  if (tools.length <= capacity) return { shown: [...tools], more: [] };
  const keep = Math.max(0, capacity - 1);
  return { shown: tools.slice(0, keep), more: tools.slice(keep) };
}

/** An overflow entry for a tool. */
export function toolEntry(t: SelectionTool, run: (a: SelectionAction) => void): MenuEntry {
  return { label: t.text, icon: t.icon, danger: t.danger, disabled: !t.enabled, hint: t.action === "move" && !t.enabled ? t.hint : undefined, action: () => run(t.action) };
}

/** The selection menu: a section, the applicable actions, a separator and Deselect all. */
export function selectionMenu(count: (a: SelectionAction) => number, selected: number, batchRunning: boolean, run: (a: SelectionAction) => void, deselect: () => void, unmuted = true, moveRefusal?: string): MenuEntry[] {
  return [
    { heading: selectedText(HEADING, "%n selected session", "%n selected sessions", selected) },
    ...barTools(count, selected, batchRunning, unmuted, moveRefusal).map((t) => toolEntry(t, run)),
    { separator: true },
    { label: tr("Deselect all"), action: deselect },
  ];
}

/**
 * The selected rows (Qt's selectionEntries): the listed session cards whose key
 * is selected. Vanished sessions and rows hidden by a search or filter are not
 * selected for any count or action.
 */
export function visibleSelected(rows: readonly Row[], selected: Readonly<Record<string, boolean>>): Session[] {
  const out: Session[] = [];
  for (const r of rows) if (r.type === "card" && selected[r.session.key]) out.push(r.session);
  return out;
}
