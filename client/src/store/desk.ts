import { deskPage, deskTakePending, onDeskNotice, onOpenSession, onOpenUpdates, type DeskNotice, type OpenSession } from "../bridge";
import { catalog, rowId } from "./catalog";
import { groupOf, headerOf, setProjectCollapsed } from "./organization";
import {
  revealRow, search, setDetailTab, setMachineFilter, setPage, setProjectFilter, setSearch, setSelectedKey,
  setStateFilter, setSubagentOpen, toast,
} from "./ui";

// The menu bar and attention notifications open sessions and show notices
// through the native side (`src-tauri/src/desk.rs`).

export interface OpenSessionOptions {
  keepFiltersIf?: (id: string) => boolean;
}

/**
 * Shows the session's row (Qt's `SessionsWindow::showSession`): the session
 * list shown, its project expanded, the row selected and scrolled to. Ordinary
 * opens clear every filter. Saved drafts may keep them after the reveal when
 * the search is empty and its owner is listed (Qt's `activateGoTo`).
 */
export function openSession(target: OpenSession, options: OpenSessionOptions = {}): void {
  if (target.attention) setDetailTab("activity");
  const id = rowId(target.computer, target.key);
  // The project the list groups the row under; this device's layout keeps
  // the collapse (the Rust side), and the next batch shows it expanded.
  if (catalog.rows[id] !== undefined) {
    const project = groupOf(id);
    if (headerOf(project)?.collapsed) setProjectCollapsed(project, false);
  }
  const keep = target.preserveFilters && search() === "" && options.keepFiltersIf?.(id) === true;
  if (!keep) {
    setStateFilter("all");
    setMachineFilter([]);
    setProjectFilter([]);
    setSearch("");
  }
  setPage("sessions");
  setSubagentOpen(undefined);
  setSelectedKey(id);
  revealRow(id);
}

/**
 * A tray or menu-bar notice. "Zerus: ..." titles (a terminal that did not
 * open, a mode not saved, an open that was not the way asked) carry what to
 * do by hand, so they stay until dismissed; plain "Zerus" ones pass.
 */
export function showDeskNotice(notice: DeskNotice): void {
  const advisory = notice.title !== "Zerus";
  toast(notice.title, notice.text, advisory ? "warn" : "info", advisory);
}

/** Opens Updates without putting the lazy Settings chunk in the main bundle. */
export function openUpdates(): void {
  void import("../ui/screens/settings/sections")
    .then(({ openSettings }) => openSettings("updates"))
    .catch((error) => toast("Updates", String(error), "error", true));
}

/**
 * Starts the wiring: it listens first and only then takes what waited, so
 * nothing falls between the two. Resolves with its stop once listening.
 */
export async function startDesk(): Promise<() => void> {
  const stops = await Promise.all([
    onOpenSession(openSession),
    onOpenUpdates(openUpdates),
    onDeskNotice(showDeskNotice),
  ]);
  void deskPage()
    .then(deskTakePending)
    .then(
      (pending) => {
        if (pending.open) openSession(pending.open);
        if (pending.updates) openUpdates();
        for (const notice of pending.notices) showDeskNotice(notice);
      },
      (error) => console.warn("Zerus Next: desk_take_pending failed", error),
    );
  return () => stops.forEach((stop) => stop());
}
