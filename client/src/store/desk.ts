import { deskPage, deskTakePending, onDeskNotice, onOpenSession, type OpenSession } from "../bridge";
import { catalog, rowId } from "./catalog";
import {
  collapsed, revealRow, setCollapsed, setDetailTab, setMachineFilter, setPage, setProjectFilter, setSearch,
  setSelectedKey, setStateFilter, setSubagentOpen, toast,
} from "./ui";

// The menu bar and attention notifications open sessions and show notices
// through the native side (`src-tauri/src/desk.rs`).

/**
 * Shows the session's row (Qt's `SessionsWindow::showSession`): every filter
 * cleared, the session list shown, its project expanded, the row selected
 * and scrolled to. A clicked notification's session opens on its Activity
 * tab first (`SessionsWindow::showAttentionSession`).
 */
export function openSession(target: OpenSession): void {
  if (target.attention) setDetailTab("activity");
  const id = rowId(target.computer, target.key);
  setStateFilter("all");
  setMachineFilter([]);
  setProjectFilter([]);
  setSearch("");
  setPage("sessions");
  const project = catalog.rows[id]?.project;
  if (project !== undefined && collapsed[project]) setCollapsed(project, false);
  setSubagentOpen(undefined);
  setSelectedKey(id);
  revealRow(id);
}

/**
 * Starts the wiring: it listens first and only then takes what waited, so
 * nothing falls between the two. Resolves with its stop once listening.
 */
export async function startDesk(): Promise<() => void> {
  const stops = await Promise.all([
    onOpenSession(openSession),
    onDeskNotice((notice) => toast(notice.title, notice.text)),
  ]);
  void deskPage()
    .then(deskTakePending)
    .then(
      (pending) => {
        if (pending.open) openSession(pending.open);
        for (const notice of pending.notices) toast(notice.title, notice.text);
      },
      (error) => console.warn("Zerus Next: desk_take_pending failed", error),
    );
  return () => stops.forEach((stop) => stop());
}
