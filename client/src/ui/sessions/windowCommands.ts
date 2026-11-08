import { tr } from "../../i18n";
import { fleetRefresh } from "../../bridge";
import { catalog, computerFresh } from "../../store/catalog";
import { needsAttention } from "../../store/attention";
import { loadMachines } from "../../store/machines";
import { groupOf, setProjectCollapsed } from "../../store/organization";
import { onCommand } from "../../store/shortcuts";
import { listedOrder, stepAttention, stepSession } from "../../store/navigation";
import { page, revealRow, selected, selectedKey, setPage, setSelectedKey, stripMode, toast, toggleSessionsList } from "../../store/ui";
import { listRows, listedSessionIgnoringCollapse, membersByGroup } from "./model";
import { actionEntries } from "./ActionDialogs";

// Task 242: the window commands Settings > Keyboard documents beyond go.*,
// help, session.new and zoom, as Qt's SessionsWindow::runShortcut runs them
// (SessionsWindowShortcuts.cpp 220-243). The Sessions page's tab and focus
// commands live with its views (SessionDetail.tsx).

const order = () => listedOrder(listRows(), membersByGroup(), listedSessionIgnoringCollapse);

/** As the attention badge counts: never an archived or muted session. */
const wantsAttention = (key: string) => {
  const session = catalog.rows[key];
  return !!session && session.state !== "archived" && !session.muted && needsAttention(session, computerFresh(session.computer));
};

function select(key: string) {
  setSelectedKey(key);
  revealRow(key);
}

/** Qt stepSession: another page shows the session list first. */
export function stepSessions(direction: 1 | -1, attention: boolean) {
  if (page() !== "sessions") setPage("sessions");
  if (!attention) {
    const key = stepSession(order(), selectedKey(), direction);
    if (key) select(key);
    return;
  }
  const found = stepAttention(order(), selectedKey(), direction, wantsAttention);
  if (!found) { toast(tr("No sessions need attention"), ""); return; }
  // In a collapsed project: the project opens to show it.
  if (found.hidden) setProjectCollapsed(groupOf(found.key), false);
  select(found.key);
}

/** Qt session.search: the session list shows, its search field takes focus with its text selected. */
export function focusSessionSearch() {
  if (page() !== "sessions") setPage("sessions");
  if (stripMode()) toggleSessionsList();
  requestAnimationFrame(() => requestAnimationFrame(() => {
    const field = document.querySelector<HTMLInputElement>(".sessions-pane input[type=search]");
    field?.focus();
    field?.select();
  }));
}

/** Qt session.rename: on the Sessions page, the selected session's Rename (as its menu offers it). */
export function renameSelected() {
  const session = selected();
  if (page() !== "sessions" || !session) return;
  const rename = actionEntries(session).rename;
  if (!rename.disabled) rename.action();
}

/** Qt view.refresh: the sessions and the machines are read again now. */
export function refreshAll() {
  void fleetRefresh().catch((error) => toast(tr("Refresh"), String(error), "error"));
  void loadMachines(true).catch(() => undefined);
}

export function installWindowCommands(): () => void {
  const off = [
    onCommand("session.next", () => stepSessions(1, false)),
    onCommand("session.previous", () => stepSessions(-1, false)),
    onCommand("session.nextAttention", () => stepSessions(1, true)),
    onCommand("session.previousAttention", () => stepSessions(-1, true)),
    onCommand("session.rename", renameSelected),
    onCommand("session.search", focusSessionSearch),
    onCommand("view.refresh", refreshAll),
  ];
  return () => off.forEach((undo) => undo());
}
