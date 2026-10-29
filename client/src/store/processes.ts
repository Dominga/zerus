import { createSignal } from "solid-js";
import * as bridge from "../bridge";
import type { ProcessesView, ProcessTarget } from "../types";
import type { Session } from "../ui/sessions/types";
import type { MenuEntry } from "./ui";

// The Processes tab's view (Qt's ProcessView), held by the Rust hub
// (src-tauri processes.rs) and sent with the batches when it changes. The
// selection follows QTreeWidget's ExtendedSelection; the hub keeps it, the
// current row and the output, and answers with a new view.

/** The hub's calls; each names the view it was made in, so a changed view refuses it. */
export interface ProcessesPort {
  select(key: string, ids: string[], current: string | null): void;
  stop(key: string, ids: string[]): void;
}

/** Ctrl or Cmd toggles a row, Shift extends from the current row; a plain click selects one. */
export interface ClickMods { toggle: boolean; range: boolean }

/**
 * QTreeWidget's ExtendedSelection over the rows in tree order: a click
 * selects one, Ctrl/Cmd toggles one, Shift selects the range from the
 * current row to the clicked one. The clicked row becomes current.
 */
export function nextSelection(order: readonly string[], selected: readonly string[], current: string | null, clicked: string, mods: ClickMods): { selected: string[]; current: string } {
  if (mods.range && current !== null && order.includes(current) && order.includes(clicked)) {
    const [from, to] = [order.indexOf(current), order.indexOf(clicked)].sort((a, b) => a - b);
    return { selected: order.slice(from, to + 1), current: clicked };
  }
  if (mods.toggle) {
    const next = selected.includes(clicked) ? selected.filter((id) => id !== clicked) : order.filter((id) => id === clicked || selected.includes(id));
    return { selected: next, current: clicked };
  }
  return { selected: [clicked], current: clicked };
}

/**
 * The session the tab follows (SessionsWindow.cpp:3783-3784). `live` is Qt's
 * `!recorded`: online, not archived or paused, a process that has not exited
 * and no inspect error. `machine` is the row's own machine and `generation`
 * its details' `host_generation`.
 */
export function processTarget(s: Session, facts: { online: boolean; generation: string | null; inspectFailed: boolean }): ProcessTarget {
  const recorded = !facts.online || s.state === "archived" || s.state === "paused" || s.processState === "exited" || facts.inspectFailed;
  return {
    computer: s.computer, key: s.catalogKey, name: s.name, run_id: s.runId ?? "", conversation_id: s.conversationId ?? "",
    archive_id: s.archiveId ?? null, machine: s.rawMachine, online: facts.online, live: !recorded, generation: facts.generation,
  };
}

/**
 * Qt's backgroundIdle (SessionPresentation.h:120-123): idle with confirmed
 * background jobs. The row carries no job count, so its status says it,
 * with the attention acknowledged or not.
 */
const backgroundIdle = (s: Pick<Session, "status" | "ackStatus">) => s.status === "background" || s.ackStatus === "background";

/**
 * The session menu's Show running commands (SessionsWindow.cpp:5297-5299,
 * 5463-5465): listed for a session with background work, enabled while its
 * computer is online and Processes is on; `open` selects the tab.
 */
export function runningCommandsEntry(s: Pick<Session, "state" | "status" | "ackStatus">, online: boolean, enabled: boolean, open: () => void): Extract<MenuEntry, { label: string }> | undefined {
  if (s.state === "archived" || !backgroundIdle(s)) return undefined;
  return {
    label: "Show running commands", icon: "activity", disabled: !(online && enabled),
    hint: enabled ? "Follow this session's background work in Processes" : "Enable Settings > Processes to inspect running commands",
    action: open,
  };
}

/** The rows in tree order: the groups as the view lists them, Finished included even while collapsed. */
export const rowOrder = (view: ProcessesView) => view.groups.flatMap((g) => g.rows.map((r) => r.id));

export function createProcesses(port: ProcessesPort) {
  const [view, setView] = createSignal<ProcessesView | undefined>();
  return {
    view,
    apply(v: ProcessesView) { setView(v); },
    /** A row click; the hub answers with the new selection. */
    click(id: string, mods: ClickMods) {
      const v = view();
      if (!v) return;
      const next = nextSelection(rowOrder(v), v.selected, v.current, id, mods);
      port.select(v.key, next.selected, next.current);
    },
    /** A right click: an unselected row becomes the selection first (ProcessView.cpp:66). */
    contextOn(id: string) {
      const v = view();
      if (v && !v.selected.includes(id)) port.select(v.key, [id], id);
    },
    /** Stop selected: the ids this view shows selected. */
    stop() {
      const v = view();
      if (v?.stop.enabled) port.stop(v.key, [...v.selected]);
    },
  };
}

export const processes = createProcesses({
  select: (key, ids, current) => bridge.processesSelect(key, ids, current),
  stop: (key, ids) => bridge.processesStop(key, ids),
});
