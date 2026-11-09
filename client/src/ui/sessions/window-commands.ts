import { batch } from "solid-js";
import { fleetRefresh } from "../../bridge";
import { tr } from "../../i18n";
import { countedAttention } from "../../store/attention";
import { catalog, computerFresh, computerOffline } from "../../store/catalog";
import { activateSearchItem, searchCursor, searchItems, setSearchCursor, type SearchItem } from "../../store/contentSearch";
import { composer } from "../../store/composer";
import { detailsFor, followedFor } from "../../store/actions";
import { openSession } from "../../store/desk";
import { machineEditor } from "../../store/machineEditor";
import { onCommand } from "../../store/shortcuts";
import { detailTab, page, peek, revealRow, search, selected, selectedKey, setPage, setPeek, setSelectedKey, stripMode, toast } from "../../store/ui";
import { actionEntries } from "./ActionDialogs";
import { listRows, listedSessionIgnoringCollapse, rowsAllOpen } from "./model";
import { setTeamCollapsed, teamCollapsed } from "../../store/teamCollapse";
import type { Session } from "./types";

// The Sessions window's commands (Qt's SessionsWindow::runShortcut,
// SessionsWindowShortcuts.cpp:207-245): session and attention stepping,
// rename, search, refresh, the detail tabs and the F6 areas. App installs
// them once; a handler limited to a field (a task title's F2) still wins
// while the focus is inside it.

type Direction = 1 | -1;

const wrap = (index: number, count: number) => ((index % count) + count) % count;

/** As the attention badge counts: never an archived session. */
const attention = (s: Session | undefined) => !!s && s.state !== "archived" && countedAttention(s, computerFresh(s.computer));

/**
 * The live list's sessions in list order, as Qt's list keeps hidden rows:
 * the sessions of a collapsed project, and child sessions under a collapsed
 * parent team, in their places. `parents` names each nested card's parent
 * row, to open the teams above a chosen child; openSession opens its project.
 */
function attentionOrder(): { order: Session[]; parents: Map<string, string> } {
  const order: Session[] = [];
  const parents = new Map<string, string>();
  for (const row of rowsAllOpen()) {
    if (row.type !== "card") continue;
    order.push(catalog.rows[row.id] ?? row.session);
    if (row.parentKey) parents.set(row.id, row.parentKey);
  }
  return { order, parents };
}

/** Opens every collapsed team above a session, so the list shows its row. */
function openTeamsAbove(key: string, parents: ReadonlyMap<string, string>) {
  const seen = new Set<string>();
  for (let parent = parents.get(key); parent && !seen.has(parent); parent = parents.get(parent)) {
    seen.add(parent);
    if (teamCollapsed(parent)) setTeamCollapsed(parent, false);
  }
}

function selectListed(id: string) {
  batch(() => { setSelectedKey(id); revealRow(id); });
}

function chooseResult(item: SearchItem) {
  setSearchCursor(item.id);
  activateSearchItem(item);
}

/** The search result Qt's list holds as current: the chosen one, else the selected session's. */
function currentResult(items: readonly SearchItem[]): number {
  const chosen = searchCursor();
  const at = chosen ? items.findIndex((item) => item.id === chosen) : -1;
  return at >= 0 ? at : items.findIndex((item) => item.session.key === selectedKey());
}

function stepResults(direction: Direction, needing: boolean) {
  const items = searchItems();
  const current = currentResult(items);
  if (!needing) {
    const next = current < 0 ? items[direction > 0 ? 0 : items.length - 1] : items[current + direction];
    if (next) chooseResult(next);
    return;
  }
  // A session can have several result rows: its other rows are no next session.
  const count = items.length;
  const currentKey = current >= 0 ? items[current].session.key : undefined;
  const start = current < 0 ? (direction > 0 ? -1 : count) : current;
  for (let step = 1; step <= count; step++) {
    const item = items[wrap(start + direction * step, count)];
    if ((step < count && item.session.key === currentKey) || !attention(catalog.rows[item.session.key] ?? item.session)) continue;
    chooseResult(item);
    return;
  }
  toast(tr("Sessions"), tr("No sessions need attention"));
}

/**
 * Next and Previous move in the shown list (search results while shown) and
 * stop at its ends. Attention stepping searches the same list in list order
 * and wraps; it opens a collapsed project to select its session.
 */
export function stepSession(direction: Direction, needing: boolean) {
  if (page() !== "sessions") setPage("sessions");
  if (search().trim()) { stepResults(direction, needing); return; }
  if (!needing) {
    const cards = listRows().filter((row) => row.type === "card");
    const at = cards.findIndex((row) => row.id === selectedKey());
    const next = at < 0 ? cards[direction > 0 ? 0 : cards.length - 1] : cards[at + direction];
    if (next) selectListed(next.id);
    return;
  }
  const { order, parents } = attentionOrder();
  const count = order.length;
  const at = order.findIndex((s) => s.key === selectedKey());
  const start = at < 0 ? (direction > 0 ? -1 : count) : at;
  for (let step = 1; step <= count; step++) {
    const s = order[wrap(start + direction * step, count)];
    if (!attention(s)) continue;
    openTeamsAbove(s.key, parents);
    openSession({ computer: s.computer, key: s.catalogKey, preserveFilters: true }, { keepFiltersIf: listedSessionIgnoringCollapse });
    return;
  }
  toast(tr("Sessions"), tr("No sessions need attention"));
}

/** F2: the header's Rename… for the selected session on the Sessions page. */
export function renameSelected() {
  const s = selected();
  if (!s || page() !== "sessions") return;
  const rename = actionEntries(s, detailsFor(s, !computerOffline(s.computer), followedFor(s.key))).rename;
  if (!rename.disabled) rename.action();
}

const shownElement = <T extends Element>(el: T | null | undefined): T | undefined =>
  el?.isConnected && !el.closest("[hidden]") ? el : undefined;

/** Ctrl+F: the session list's search field, selected; a collapsed list opens over the conversation. */
export function focusSessionSearch() {
  if (page() !== "sessions") setPage("sessions");
  if (stripMode() && !peek()) setPeek(true);
  const focus = () => {
    const field = shownElement(document.querySelector<HTMLInputElement>(".sp-panel .sp-search input"));
    if (!field) return false;
    field.focus();
    field.select();
    return true;
  };
  if (!focus()) requestAnimationFrame(() => { focus(); });
}

/** Ctrl+R and F5: the fleet's listings, the selected session's inspection and an open Machines page. */
export function refreshView() {
  void fleetRefresh().catch((error) => toast(tr("Connection"), String(error), "error", true));
  if (selected()) void composer.refresh();
  if (page() === "machines") machineEditor.choose(machineEditor.selected(), true);
}

const tabButtons = () => [...document.querySelectorAll<HTMLButtonElement>(".detail .detail-tabs [role=tab][data-tab]")]
  .filter((tab) => !tab.disabled && shownElement(tab));
const tabButton = (id: string) => tabButtons().find((tab) => tab.dataset.tab === id);

/** Where a detail view takes the focus (Qt's viewFocusTarget): Activity's text, Terminal, the process list. */
function viewTarget(): HTMLElement | undefined {
  const body = shownElement(document.querySelector<HTMLElement>(".detail .tab-body"));
  if (!body) return undefined;
  for (const selector of [".act-scroll", ".xterm-helper-textarea", ".proc-list", "textarea:not([hidden]):not(:disabled)"]) {
    const found = shownElement(body.querySelector<HTMLElement>(selector));
    if (found) return found;
  }
  return shownElement(body.querySelector<HTMLElement>("[tabindex]:not([tabindex='-1']), button:not(:disabled), input:not(:disabled)"));
}

const composerEditor = () => {
  const editor = shownElement(document.querySelector<HTMLTextAreaElement>(".detail .tab-body .cmp-editor"));
  return editor && !editor.disabled ? editor : undefined;
};

const onSessionView = () => page() === "sessions" && !!selected();

/** Ctrl+Tab and Ctrl+Shift+Tab: the next shown tab, wrapping, then its view's focus. */
export function stepTab(direction: Direction) {
  if (!onSessionView()) return;
  const tabs = tabButtons();
  if (tabs.length < 2) return;
  const at = tabs.findIndex((tab) => tab.getAttribute("aria-selected") === "true");
  tabs[wrap(at + direction, tabs.length)].click();
  viewTarget()?.focus();
}

/** Ctrl+`: Terminal, connected and focused; from Terminal back to Activity and its message field. */
export function toggleTerminal() {
  if (!onSessionView()) return;
  if (detailTab() !== "terminal") {
    const terminal = tabButton("terminal");
    if (!terminal) return;
    terminal.click();
    viewTarget()?.focus();
    return;
  }
  tabButton("activity")?.click();
  composerEditor()?.focus();
}

interface Area { area: Element; target: HTMLElement | undefined }

/**
 * The F6 areas of the Sessions page: the session list (or the search
 * results), the current view, the message field and the side panel. The
 * innermost area holding the focus is the current one, since the message
 * field lies inside Activity.
 */
function focusAreas(): Area[] {
  const areas: Area[] = [];
  const list = shownElement(document.querySelector<HTMLElement>(".sessions-pane .content-search-list"))
    ?? shownElement(document.querySelector<HTMLElement>(".sessions-pane .sp-scroll[role=tree]"));
  if (list) areas.push({ area: list, target: list });
  const body = selected() ? shownElement(document.querySelector<HTMLElement>(".detail .tab-body")) : undefined;
  if (body) areas.push({ area: body, target: viewTarget() });
  const editor = selected() ? composerEditor() : undefined;
  const field = editor?.closest(".composer") ?? editor;
  if (editor && field) areas.push({ area: field, target: editor });
  const panel = selected() ? shownElement(document.querySelector<HTMLElement>(".detail .inspector")) : undefined;
  if (panel) {
    const tab = panel.querySelector<HTMLElement>(".insp-tabs .tab[aria-selected=true]") ?? panel.querySelector<HTMLElement>("[tabindex='0'], button");
    areas.push({ area: panel, target: tab ?? undefined });
  }
  return areas;
}

/** F6 and Shift+F6: the next or previous area, wrapping. */
export function stepFocus(direction: Direction) {
  if (page() !== "sessions") return;
  const areas = focusAreas();
  if (!areas.length) return;
  const focus = document.activeElement;
  let current = -1;
  areas.forEach(({ area }, index) => { if (focus && (focus === area || area.contains(focus))) current = index; });
  const next = current < 0 ? (direction > 0 ? 0 : areas.length - 1) : wrap(current + direction, areas.length);
  areas[next].target?.focus();
}

/** Registers the window commands; returns the function that removes them. */
export function installWindowCommands(): () => void {
  const commands: [string, () => void][] = [
    ["session.next", () => stepSession(1, false)],
    ["session.previous", () => stepSession(-1, false)],
    ["session.nextAttention", () => stepSession(1, true)],
    ["session.previousAttention", () => stepSession(-1, true)],
    ["session.rename", renameSelected],
    ["session.search", focusSessionSearch],
    ["view.refresh", refreshView],
    ["tab.next", () => stepTab(1)],
    ["tab.previous", () => stepTab(-1)],
    ["tab.terminal", toggleTerminal],
    ["focus.next", () => stepFocus(1)],
    ["focus.previous", () => stepFocus(-1)],
  ];
  const off = commands.map(([id, run]) => onCommand(id, run));
  return () => off.forEach((undo) => undo());
}
