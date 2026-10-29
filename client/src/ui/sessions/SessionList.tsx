import { For, Show, Suspense, createMemo, createEffect, createSignal, lazy, on, onCleanup } from "solid-js";
import { createVirtualizer } from "@tanstack/solid-virtual";
import { Icon } from "../lib/icons";
import { listRows, rowHeight, shownSummary, machineChoices, projectChoices, type Row } from "./model";
import {
  SessionCard, ChildCard, CompactCard, AgentBadge, StatusBadge, badgeKind, statusCaption, stripCaption,
  emphasis, childStatus, gitTooltip, paletteIndex, folderOf,
} from "./SessionCard";
import { catalog, computerFresh, computerOffline } from "../../store/catalog";
import { visibleUnreadReply } from "../../store/attention";
import { runningCommandsEntry } from "../../store/processes";
import { flag } from "../../store/settings";
import { archiveCount, archiveTip } from "../../store/archive";
import { ArchiveBar, datePanelOpen } from "./ArchiveBar";
import { detailsFor, followedFor, notify, updating } from "../../store/actions";
import { STATE_CHANGED } from "./actions-ui";
import { running } from "../../store/batch";
import { grouping, headerOf, moveProject, setProjectCollapsed, setProjectToShow, sortProject } from "../../store/organization";
import { TOOL_WIDTH, clearArchiveMenuEntry, machineMenu, runSelection, selectedRows, selectionContextMenu, selectionCount } from "./selection-live";
import { barTools, splitTools, toolEntry } from "./selection-ui";
import { actionEntries, copyEntries, freshActionEntry, muteEntry } from "./ActionDialogs";
import { markAllEntry, markAllTip, markEntry, runMarkAll } from "./mark-entries";
import { moveEntry, openCreateProjectDialog, openDeleteProjectDialog, openRenameProjectDialog } from "./OrganizationDialogs";
import { backgroundEntries } from "./background-menu";
import { chipColor, headerEntries, projectColor, type FilterChip } from "./organization-ui";
import { newSessionForProject, newSessionFromPanel } from "./newsession-view";
import { CreationCard, CreationTile } from "./CreationCard";
import { creationCount, creationFooter, creationStatusMenu } from "../../store/creations";
import {
  selectedKey, setSelectedKey, setExpandedChildren, expandedChildren,
  setCompactRows, compactRows, stateFilter, setStateFilter, counts, search, setSearch, stripMode, setStripMode,
  setMenu, machineFilter, setMachineFilter, projectFilter, setProjectFilter, setPage,
  multi, setMulti, peek, setPeek, setSubagentOpen, sessionsWidth, setSessionsWidth, menu, subagentOpen,
  revealing, setRevealing, setDetailTab,
  type StateFilter, type MenuEntry,
} from "../../store/ui";
import type { Session } from "./types";
import { motionReduced } from "../motion";
import { composer } from "../../store/composer";
import { bulkSelect, clearMulti, multiCount, setAnchor, dragItem, setDragItem, drop, setDrop, endDrag, placement, applyDrop, SESSION_MIME, type DragItem } from "./organize";

const SavedDraftsDialog = lazy(() => import("./SavedDrafts").then((m) => ({ default: m.SavedDraftsDialog })));

/** Actions that go through guarded `hgs` calls arrive in phase 2; their menu entries show disabled. */
const LATER = true;

// Five state filters, in SessionsWindow order, then the scope filters.
const FILTERS: { id: StateFilter; icon: string; tip: string; count: () => string; tipText?: () => string }[] = [
  { id: "all", icon: "all", tip: "All sessions", count: () => String(counts().all) },
  { id: "attention", icon: "attention", tip: "Needs attention", count: () => String(counts().attention) },
  { id: "active", icon: "active", tip: "Active: working, scheduled, waiting, needs attention or a draft", count: () => String(counts().active) },
  { id: "paused", icon: "paused", tip: "Saved sessions (paused or stopped)", count: () => String(counts().paused) },
  { id: "archived", icon: "archived", tip: "Archive", count: archiveCount, tipText: archiveTip },
];

const filterTone = (id: StateFilter) => ({
  "t-attention": id === "attention" && counts().attention > 0,
  "t-active": id === "active" && counts().active > 0,
});

const openMenu = (e: MouseEvent, items: MenuEntry[]) => {
  e.preventDefault();
  e.stopPropagation();
  setMenu({ x: e.clientX, y: e.clientY, items });
};

// Card <-> compact row. The swap is instant in Qt; here it morphs in 220 ms so
// the title never jumps: the card's height eases to the new size, the title and
// badges glide from their old place (FLIP, transform only), the detail lines
// fade, and the rows below slide with the same timing.
const MORPH = { duration: 220, easing: "cubic-bezier(.33, 1, .68, 1)" } as const;
const FLIP = [".sc-title", ".sbadge", ".agent-badge", ".sc-mute"];
const DETAIL_LINES = ".sc-l2, .sc-l3, .sc-foot";
const [morphing, setMorphing] = createSignal(false);
let morphTimer: number | undefined;

function setRowCompact(key: string, compact: boolean) {
  if (!!compactRows[key] === compact) return;
  const row = document.querySelector<HTMLElement>(`.sp-scroll .vrow[data-key="${CSS.escape(key)}"]`);
  const card = row?.querySelector<HTMLElement>(".scard");
  if (!row || !card || stripMode() || motionReduced()) { setCompactRows(key, compact); return; }
  const before = FLIP.map((sel) => row.querySelector(sel)?.getBoundingClientRect());
  const from = card.getBoundingClientRect().height;
  const ghosts = compact ? [...card.querySelectorAll<HTMLElement>(DETAIL_LINES)].map((n) => n.cloneNode(true) as HTMLElement) : [];
  clearTimeout(morphTimer);
  setMorphing(true);
  setCompactRows(key, compact); // Solid updates the row synchronously.
  const next = row.querySelector<HTMLElement>(".scard");
  if (next) {
    const to = next.getBoundingClientRect().height;
    next.classList.add("morph");
    next.animate([{ height: `${from}px` }, { height: `${to}px` }], MORPH).finished.catch(() => {}).finally(() => next.classList.remove("morph"));
    FLIP.forEach((sel, i) => {
      const el = row.querySelector<HTMLElement>(sel), a = before[i];
      if (!el || !a) return;
      const b = el.getBoundingClientRect();
      if (a.left !== b.left || a.top !== b.top) el.animate([{ transform: `translate(${a.left - b.left}px, ${a.top - b.top}px)` }, { transform: "none" }], MORPH);
    });
    if (compact) {
      for (const g of ghosts) {
        g.setAttribute("aria-hidden", "true");
        g.style.pointerEvents = "none";
        next.append(g);
        g.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 120, easing: "ease-out", fill: "forwards" }).finished.catch(() => {}).finally(() => g.remove());
      }
    } else {
      next.querySelectorAll<HTMLElement>(DETAIL_LINES).forEach((n) => n.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 160, delay: 60, easing: "ease-out", fill: "backwards" }));
    }
  }
  morphTimer = window.setTimeout(() => setMorphing(false), MORPH.duration + 40);
}

/**
 * The menu's Clear session (Qt's SessionMenuAction::Clear, SessionsWindow.cpp
 * 5339-5340 and 5485-5490): only for the selected row, with the header's
 * state and reason, and through the composer's Clear dialog. A row whose
 * state changed since the menu opened asks to open its actions again.
 */
function clearEntry(s: Session): MenuEntry {
  const control = s.key === selectedKey() && !subagentOpen() ? composer.view()?.clear : undefined;
  return {
    label: "Clear session…", icon: "clear", disabled: !control?.enabled, hint: control?.reason || undefined,
    action: () => {
      if (catalog.rows[s.key]?.state !== s.state) { notify("error", STATE_CHANGED, true); return; }
      composer.openClear();
    },
  };
}

export function sessionMenu(s: Session): MenuEntry[] {
  // The selected row's menu reads the same details as the header (Qt's m_details).
  const details = s.key === selectedKey() && !subagentOpen() ? detailsFor(s, !computerOffline(s.computer), followedFor(s.key)) : undefined;
  const act = actionEntries(s, undefined, { context: true });
  const cp = copyEntries(s, details);
  const mute = muteEntry(s);
  const mark = markEntry(s);
  const fresh = freshActionEntry(s);
  const archived = s.state === "archived";
  const commands = runningCommandsEntry(s, () => computerFresh(s.computer), () => flag("processes/enabled"), () => { setSelectedKey(s.key); setDetailTab("processes"); });
  return [
    { label: compactRows[s.key] ? "Expand session row" : "Compact session row", icon: compactRows[s.key] ? "expand-row" : "compact-row", action: () => setRowCompact(s.key, !compactRows[s.key]) },
    { label: "Expand all collapsed", icon: "expand-row", disabled: !Object.values(compactRows).some(Boolean), action: () => Object.keys(compactRows).forEach((k) => setCompactRows(k, false)) },
    { separator: true },
    // An archived row has no terminal, and no Mute or Mark group (Qt's `if (!archived)`).
    ...(archived ? [] : [s.agent === "deepseek" ? { label: "Open native DeepSeek UI", icon: "external-link", disabled: LATER } : { label: "Open terminal", icon: "square-terminal", disabled: LATER }]),
    ...(commands ? [commands] : []),
    { label: "Open folder", icon: "folder-open", disabled: LATER },
    { label: "Open shell in session folder", icon: "terminal", disabled: LATER },
    cp.command,
    ...(archived ? [] : [{ separator: true } as const, ...(mute ? [mute] : []), ...(mark ? [mark] : [])]),
    { separator: true },
    act.change,
    ...(fresh ? [fresh] : archived ? [] : [clearEntry(s)]),
    act.rename,
    { label: "Fork session…", icon: "git-fork", disabled: LATER },
    ...(act.archive ? [act.archive] : []),
    moveEntry(s),
    { separator: true },
    cp.name,
    cp.reference,
    cp.folder,
    { separator: true },
    act.end,
  ];
}

/** The project header's menu (SessionsWindow.cpp:2568-2616); its rules are organization-ui.ts's. */
function headerMenu(project: string): MenuEntry[] {
  return headerEntries(grouping(), project, {
    newSession: () => newSessionForProject(project),
    filter: () => setProjectFilter([project]),
    collapse: (collapsed) => setProjectCollapsed(project, collapsed),
    rename: () => openRenameProjectDialog(project),
    settings: () => { setProjectToShow(project); setPage("projects"); },
    move: (before) => { void moveProject(project, before); },
    sort: () => { void sortProject(project); },
    remove: () => openDeleteProjectDialog(project),
  });
}

function batchMenu(): MenuEntry[] {
  return [
    ...machineMenu(),
    { label: "Expand all collapsed", icon: "expand-row", action: () => Object.keys(compactRows).forEach((k) => setCompactRows(k, false)) },
    markAllEntry(),
    clearArchiveMenuEntry(),
  ];
}

/** Project header: 40 px row, 32 px band (radius 6), Qt chevron, icon and counters. */
function Header(props: { row: Extract<Row, { type: "header" }> }) {
  const name = () => props.row.name;
  const open = () => !props.row.collapsed;
  return (
    <button class="group-band entity" style={{ "--entity": projectColor(props.row) }} draggable="true" onDragStart={(e) => startDrag(e, { kind: "group", id: props.row.project })}
      aria-expanded={open()} onClick={() => setProjectCollapsed(props.row.project, !props.row.collapsed)}
      onContextMenu={(e) => openMenu(e, headerMenu(props.row.project))}
      title={`${name()}: ${props.row.total} shown, ${props.row.attention} need a response, ${props.row.working} working, ${props.row.unread} unread replies. Click to collapse or expand; drag to reorder.`}>
      <svg class="gb-chev" width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d={open() ? "M1 3L5 7L9 3" : "M3 1L7 5L3 9"} />
      </svg>
      <Icon name="projects" size={16} class="gb-icon" />
      <span class="gb-name ell">{name()}</span>
      <Show when={props.row.working}><span class="gb-count working">● {props.row.working}</span></Show>
      <Show when={props.row.unread}><span class="gb-count hot">● {props.row.unread}</span></Show>
      <Show when={props.row.attention}><span class="gb-count hot">! {props.row.attention}</span></Show>
      <span class="gb-count total">{props.row.total}</span>
    </button>
  );
}

function selectSession(key: string, e?: MouseEvent) {
  const additive = !!e && (e.ctrlKey || e.metaKey);
  if (e && (additive || e.shiftKey)) {
    bulkSelect(key, additive, e.shiftKey);
    return;
  }
  clearMulti();
  setAnchor(key);
  setSelectedKey(key);
  setSubagentOpen(undefined);
  if (peek()) setPeek(false);
}


function ScopeButton(props: { icon: string; label: string; selected: string[]; set: (v: string[]) => void; choices: { id: string; label: string; count: number }[] }) {
  return (
    <button class="scope-btn" classList={{ on: props.selected.length > 0 }} data-tip={props.selected.length ? `${props.selected.length} ${props.label} selected; click to change` : `All ${props.label}: choose one or more`}
      onClick={(e) => openMenu(e, [
        { heading: `Choose ${props.label}` },
        { label: `All ${props.label} (${props.choices.reduce((a, c) => a + c.count, 0)})`, action: () => props.set([]) },
        { separator: true },
        ...props.choices.map((c) => ({ label: `${c.label} (${c.count})`, checked: props.selected.includes(c.id),
          action: () => props.set(props.selected.includes(c.id) ? props.selected.filter((x) => x !== c.id) : [...props.selected, c.id]) })),
      ])}>
      <Icon name={props.icon} size={14} />
      <Show when={props.selected.length}><span class="tabular">{props.selected.length}</span></Show>
    </button>
  );
}

function FilterChips() {
  const chips = (): FilterChip[] => [
    ...machineFilter().map((m) => ({ id: m, label: m, kind: "m" as const })),
    ...projectFilter().map((p) => ({ id: p, label: headerOf(p)?.name ?? p, kind: "p" as const })),
  ];
  const remove = (c: FilterChip) =>
    c.kind === "m" ? setMachineFilter(machineFilter().filter((x) => x !== c.id)) : setProjectFilter(projectFilter().filter((x) => x !== c.id));
  return (
    <Show when={chips().length}>
      <div class="filter-chips">
        <For each={chips()}>{(c) => (
          <button class="fchip-sel entity" style={{ "--entity": chipColor(c, headerOf, paletteIndex) }} title="Remove filter" onClick={() => remove(c)}>
            <span class="ell">{c.label}</span><Icon name="x" size={11} />
          </button>
        )}</For>
      </div>
    </Show>
  );
}

function SelectionBar() {
  const [width, setWidth] = createSignal(0);
  const observe = (el: HTMLElement) => {
    const ro = new ResizeObserver(() => setWidth(el.clientWidth));
    ro.observe(el);
    onCleanup(() => ro.disconnect());
  };
  const rows = createMemo(() => selectedRows(listRows()));
  const tools = createMemo(() => barTools(selectionCount(rows()), rows().length, running()));
  const split = createMemo(() => splitTools(tools(), Math.max(1, Math.floor(width() / TOOL_WIDTH))));
  return (
    <Show when={rows().length > 1}>
      <div class="selection-bar" role="toolbar" aria-label="Selected sessions">
        <div class="row"><b>{rows().length} selected</b><span class="grow" /><button class="icon-btn small" title="Deselect all" aria-label="Deselect all" onClick={clearMulti}><Icon name="x" size={14} /></button></div>
        <div class="row sel-actions" ref={observe}>
          <For each={split().shown}>
            {(t) => (
              <button class="icon-btn" classList={{ danger: t.danger }} title={t.hint} aria-label={t.hint} disabled={!t.enabled} data-text={t.text} onClick={() => runSelection(rows(), t.action)}><Icon name={t.icon} size={18} /></button>
            )}
          </For>
          <Show when={split().more.length}>
            <button class="icon-btn" title="More actions for selected sessions" aria-label="More actions for selected sessions" aria-haspopup="menu"
              onClick={(e) => openMenu(e, split().more.map((t) => toolEntry(t, (a) => runSelection(rows(), a))))}><Icon name="more" size={18} /></button>
          </Show>
        </div>
      </div>
    </Show>
  );
}

// Strip geometry (SessionCardDelegate.h SessionStrip): tile side 84, strip 98.
const STRIP_WIDTH = 98;
const OPEN_MS = 220; // SessionPanelDock: 220 ms, OutCubic
const OUT_CUBIC = "cubic-bezier(.33, 1, .68, 1)";

/**
 * Keeps one DOM node per row id. The virtualizer's items are new objects on
 * every measure, so the list iterates stable string keys instead: a catalog
 * delta updates bound text in place and never rebuilds a row (no flicker, no
 * restarted animations, hover kept).
 */
function useRows(strip: boolean) {
  // A plain element, not a signal: a reactive getter makes the virtualizer
  // observe the box during render, before it is attached and sized.
  let scrollEl: HTMLElement | undefined;
  const rows = listRows;
  const heights = createMemo(() => rows().map((r) => rowHeight(r, strip)));
  const virtualizer = createVirtualizer({
    get count() { return rows().length; },
    // Only once it is in the page: a box observed while Solid is still creating
    // it reports no size, and the list would stay empty (the scroller appears
    // with the first rows, after the placeholder).
    getScrollElement: () => (scrollEl?.isConnected ? scrollEl : null),
    estimateSize: (i) => heights()[i] ?? 94,
    getItemKey: (i) => rows()[i]?.id ?? i,
    overscan: 6,
  });
  // Re-measure only when ids or heights change, not on every row update.
  const shape = createMemo(() => rows().map((r, i) => `${r.id}:${heights()[i]}`).join("|"));
  createEffect(on(shape, () => virtualizer.measure(), { defer: true }));
  const byId = createMemo(() => new Map(rows().map((r) => [r.id, r])));
  const items = createMemo(() => virtualizer.getVirtualItems());
  // getVirtualItems() is one store array reconciled by index; reading its keys
  // here tracks it, and an unchanged key list keeps every row's DOM node.
  const keys = createMemo<string[]>(() => items().map((i) => String(i.key)), [], { equals: (a, b) => a.length === b.length && a.every((k, i) => k === b[i]) });
  const startOf = createMemo(() => new Map(items().map((i) => [String(i.key), i.start])));
  // The scroller can be replaced (placeholder, then the list); observe the new box once it is in the page.
  const ref = (el: HTMLElement) => { scrollEl = el; requestAnimationFrame(() => virtualizer._willUpdate()); };
  // Fixed heights per row type, so every row's offset is known without the DOM.
  const offsets = createMemo(() => { let y = 0; return heights().map((h) => { const t = y; y += h; return t; }); });
  return { ref, rows, virtualizer, byId, keys, startOf, offsets, heights, heightOf: (id: string) => { const i = rows().findIndex((r) => r.id === id); return heights()[i] ?? 94; } };
}

/** Slides the full panel out from under the strip (transform only). */
function slide(el: HTMLElement | undefined, width: number, opening: boolean, done?: () => void) {
  if (!el || motionReduced()) { done?.(); return; }
  const hidden = `translateX(${-(width - STRIP_WIDTH)}px)`;
  const a = el.animate(
    opening ? [{ transform: hidden, opacity: 0.6 }, { transform: "translateX(0)", opacity: 1 }] : [{ transform: "translateX(0)", opacity: 1 }, { transform: hidden, opacity: 0 }],
    { duration: OPEN_MS, easing: OUT_CUBIC },
  );
  a.onfinish = () => done?.();
}

export function SessionList() {
  const list = useRows(false);
  // A row the menu bar or a notification opened (Qt's revealRow): scrolled to
  // once the list has it; another selection drops the request.
  createEffect(on([revealing, list.rows, selectedKey], ([id, rows, selected]) => {
    if (!id) return;
    if (selected !== id) { setRevealing(undefined); return; }
    const index = rows.findIndex((r) => r.id === id);
    if (index < 0) return;
    setRevealing(undefined);
    requestAnimationFrame(() => list.virtualizer.scrollToIndex(index, { align: "center" }));
  }));
  let panel: HTMLDivElement | undefined;
  let section: HTMLElement | undefined;
  const [panelVisible, setPanelVisible] = createSignal(!stripMode());
  const [searchFocused, setSearchFocused] = createSignal(false);
  const [savedOpen, setSavedOpen] = createSignal(false);
  const showSavedDrafts = async (from: HTMLElement) => {
    const handle = subagentOpen() ? undefined : composer.view()?.handle;
    setSavedOpen(true);
    const { openSavedDrafts } = await import("./SavedDrafts");
    openSavedDrafts(from, handle);
  };

  // Docked <-> strip: the conversation resizes once; the panel animates above it.
  const toggleDock = () => {
    Object.keys(multi).forEach((k) => setMulti(k, false));
    if (!stripMode()) {
      setStripMode(true);
      setPanelVisible(true);
      slide(panel, sessionsWidth(), false, () => { if (stripMode() && !peek()) setPanelVisible(false); });
    } else {
      setPeek(false);
      setStripMode(false);
      setPanelVisible(true);
      requestAnimationFrame(() => slide(panel, sessionsWidth(), true));
    }
  };
  // Peek: the full panel lies over the conversation; the strip stays put.
  createEffect(on(peek, (open, was) => {
    if (!stripMode() || open === was) return;
    if (open) { setPanelVisible(true); requestAnimationFrame(() => slide(panel, sessionsWidth(), true)); }
    else slide(panel, sessionsWidth(), false, () => { if (!peek() && stripMode()) setPanelVisible(false); });
  }, { defer: true }));

  // Hover expand: opens after 150 ms of rest on the strip; closes once the
  // pointer has been outside for 350 ms. Menus and an active search keep it open.
  let enterTimer: number | undefined, leaveTimer: number | undefined;
  let hoverOpened = false;
  const keepOpen = () => !!menu() || searchFocused() || search().length > 0 || datePanelOpen();
  const onStripEnter = () => {
    clearTimeout(leaveTimer);
    if (!stripMode() || peek()) return;
    enterTimer = setTimeout(() => { if (stripMode() && !peek()) { hoverOpened = true; setPeek(true); } }, 150);
  };
  const onSectionEnter = () => clearTimeout(leaveTimer);
  const onSectionLeave = () => {
    clearTimeout(enterTimer);
    if (!peek() || !hoverOpened) return;
    const check = () => { if (keepOpen()) leaveTimer = setTimeout(check, 100); else { hoverOpened = false; setPeek(false); } };
    leaveTimer = setTimeout(check, 350);
  };
  const onKeyEsc = (e: KeyboardEvent) => {
    if (e.key !== "Escape") return;
    if (peek()) { hoverOpened = false; setPeek(false); }
    // Esc ends a bulk selection, unless a menu or dialog takes it first.
    else if (multiCount() && !menu() && !document.querySelector("dialog[open]")) clearMulti();
  };
  addEventListener("keydown", onKeyEsc);
  const outside = (e: PointerEvent) => { if (peek() && section && !section.contains(e.target as Node) && !(e.target as Element).closest?.(".menu")) { hoverOpened = false; setPeek(false); } };
  addEventListener("pointerdown", outside);
  onCleanup(() => { removeEventListener("keydown", onKeyEsc); removeEventListener("pointerdown", outside); clearTimeout(enterTimer); clearTimeout(leaveTimer); });

  const onKey = (e: KeyboardEvent) => {
    const nav = list.rows().filter((r) => r.type !== "header");
    const i = nav.findIndex((r) => r.id === selectedKey());
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const next = nav[Math.max(0, Math.min(nav.length - 1, i + (e.key === "ArrowDown" ? 1 : -1)))];
      if (next) {
        setSelectedKey(next.type === "child" ? next.parent.key : next.id);
        list.virtualizer.scrollToIndex(list.rows().indexOf(next), { align: "auto" });
      }
    } else if ((e.key === "ArrowRight" || e.key === "ArrowLeft") && i >= 0) {
      const r = nav[i];
      if (r.type === "card" && r.compactRow && e.key === "ArrowRight") setRowCompact(r.session.key, false);
      else if (r.type === "card" && r.expandable) setExpandedChildren(r.session.key, e.key === "ArrowRight");
      else if (r.type === "child" && e.key === "ArrowLeft") setSelectedKey(r.parent.key);
    } else if (e.key === "Escape") {
      Object.keys(multi).forEach((k) => setMulti(k, false));
    }
  };

  const toggleGlyph = () => (!stripMode() ? "collapse-sessions" : peek() ? "pin" : "expand-sessions");
  const toggleCaption = () => (!stripMode() ? "Collapse session list" : peek() ? "Keep session list open" : "Expand session list");

  return (
    <section class="sessions-pane" ref={section} classList={{ strip: stripMode(), peek: stripMode() && peek() }} aria-label="Sessions"
      style={{ "--pane-w": stripMode() ? `${STRIP_WIDTH}px` : `${sessionsWidth()}px`, "--panel-w": `${sessionsWidth()}px` }}
      onPointerEnter={onSectionEnter} onPointerLeave={onSectionLeave}>
      <Show when={stripMode()}><Strip onEnter={onStripEnter} onToggle={toggleDock} /></Show>
      <div class="sp-panel" ref={panel} classList={{ overlay: stripMode() }} hidden={!panelVisible()}
        onContextMenu={(e) => { if ((e.target as Element).closest("input,textarea")) return; openMenu(e, backgroundEntries({ newProject: openCreateProjectDialog })); }}>
        <header class="sp-head">
          <h1>{stateFilter() === "archived" ? "Archive" : "Sessions"}</h1>
          <span class="grow" />
          <button class="icon-btn hd" data-tip="New session" aria-label="New session" onClick={newSessionFromPanel}><Icon name="add" size={18} /></button>
          <button class="icon-btn hd" data-tip={toggleCaption()} onClick={toggleDock}><Icon name={toggleGlyph()} size={18} /></button>
          <button class="icon-btn hd" data-tip="Session and machine actions" onClick={(e) => openMenu(e, batchMenu())}><Icon name="more" size={18} /></button>
        </header>
        <div class="sp-search">
          <input class="field" type="search" placeholder={stateFilter() === "archived" ? "Search archive names and IDs…" : "Search sessions and IDs…"} value={search()}
            onInput={(e) => setSearch(e.currentTarget.value)} onFocus={() => setSearchFocused(true)} onBlur={() => setSearchFocused(false)}
            onKeyDown={(e) => e.key === "Escape" && setSearch("")} aria-label="Search sessions" />
        </div>
        <div class="sp-filters" role="toolbar" aria-label="State filter">
          <For each={FILTERS}>
            {(f) => (
              <button class="fchip" classList={{ on: stateFilter() === f.id, ...filterTone(f.id) }} data-tip={f.tipText?.() ?? `${f.tip} / ${f.count()}`}
                aria-pressed={stateFilter() === f.id} onClick={() => setStateFilter(f.id)}>
                <Icon name={f.icon} size={16} />
                <span class="tabular">{f.count()}</span>
              </button>
            )}
          </For>
          <ScopeButton icon="machines" label="machines" selected={machineFilter()} set={setMachineFilter} choices={machineChoices()} />
          <ScopeButton icon="projects" label="projects" selected={projectFilter()} set={setProjectFilter} choices={projectChoices()} />
        </div>
        <FilterChips />

        <Show when={stateFilter() === "archived"}><ArchiveBar /></Show>
          <Show when={catalog.order.length > 0 || creationCount() > 0 || stateFilter() === "archived"} fallback={<ListPlaceholder />}>
            <div class="sp-scroll" classList={{ morphing: morphing(), organizing: !!dragItem() }} ref={list.ref} tabindex="0" onKeyDown={onKey} role="tree"
              onDragOver={(e) => organizeOver(e, list)} onDrop={(e) => organizeDrop(e)} onDragLeave={(e) => { if (!(e.currentTarget as Element).contains(e.relatedTarget as Node)) setDrop(undefined); }}
              aria-label="Agent sessions; Ctrl-click to select several; drag to organize, left/right to collapse or expand a group">
              <Show when={list.rows().length} fallback={<NoMatches />}>
                <div style={{ height: `${list.virtualizer.getTotalSize()}px`, position: "relative" }}>
                  <DropIndicator />
                  <For each={list.keys()}>{(id) => <ListRow id={id} list={list} />}</For>
                </div>
              </Show>
            </div>
          </Show>
        <SelectionBar />
        <footer class="sp-foot">
          <span class="tabular ell">{shownSummary().shown ? `${shownSummary().shown} shown, ${shownSummary().working} working, ${shownSummary().attention} attention${creationFooter(true)}` : creationFooter(false) || (catalog.order.length ? "No matching sessions" : "")}</span>
          <span class="grow" />
          <button class="icon-btn small" data-tip="Session creation status" onClick={(e) => openMenu(e, creationStatusMenu())}><Icon name="clock" size={16} /></button>
          <button class="icon-btn small" title={markAllTip(markAllEntry())} disabled={markAllEntry().disabled} onClick={() => runMarkAll()}><Icon name="read-all" size={16} /></button>
          {/* The open composer is the dialog's target, none while a subagent shows (SessionsWindow.cpp:485-489). */}
          <button class="icon-btn small" data-tip="Saved drafts: recover local drafts, including those from ended sessions" aria-label="Saved drafts"
            onClick={(e) => { void showSavedDrafts(e.currentTarget); }}><Icon name="drafts" size={16} /></button>
        </footer>
      </div>
      <Show when={!stripMode()}><ResizeHandle /></Show>
      <Show when={savedOpen()}><Suspense><SavedDraftsDialog /></Suspense></Show>
    </section>
  );
}

/** QSplitter handle: 8 px, transparent; drag sets the docked width (min 270). */
function ResizeHandle() {
  const [dragging, setDragging] = createSignal(false);
  const down = (e: PointerEvent) => {
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const startX = e.clientX, startW = sessionsWidth();
    const right = !!el.closest(".layout-right");
    setDragging(true);
    const move = (m: PointerEvent) => {
      const dx = (m.clientX - startX) * (right ? -1 : 1);
      setSessionsWidth(Math.round(Math.max(270, Math.min(innerWidth - 56 - 520, startW + dx))));
    };
    const up = () => { setDragging(false); el.removeEventListener("pointermove", move); el.removeEventListener("pointerup", up); };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
  };
  return <div class="sp-resize" classList={{ dragging: dragging() }} tabindex="-1" role="separator" aria-orientation="vertical" aria-label="Session list width" onPointerDown={down}><i /></div>;
}


// --- Organising by drag (SessionList.cpp placement/dropEvent) ---------------

const IS_MAC = /Mac|iPhone|iPad/.test(navigator.platform);

function startDrag(e: DragEvent, item: DragItem) {
  if (stripMode() || !e.dataTransfer) return;
  e.dataTransfer.setData(SESSION_MIME, JSON.stringify(item));
  e.dataTransfer.effectAllowed = "move";
  setDragItem(item);
  addEventListener("dragend", endDrag, { once: true });
}

function organizeOver(e: DragEvent, list: { rows: () => Row[]; offsets: () => number[]; heights: () => number[] }) {
  const item = dragItem();
  if (!item) return;
  e.preventDefault();
  if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
  const scroller = e.currentTarget as HTMLElement;
  const y = e.clientY - scroller.getBoundingClientRect().top + scroller.scrollTop;
  const over = (e.target as Element).closest<HTMLElement>(".vrow")?.dataset.key;
  setDrop(placement(list.rows(), list.offsets(), list.heights(), over, y, item));
}

function organizeDrop(e: DragEvent) {
  const item = dragItem(), target = drop();
  if (!item || !target) return;
  e.preventDefault();
  applyDrop(item, target);
  endDrag();
}

/** Qt's drop indicator: a 2 px accent line between rows, or a frame around a project header. */
function DropIndicator() {
  return (
    <Show when={drop()}>{(d) => (
      <Show when={d().box} fallback={<i class="drop-line" style={{ transform: `translateY(${(d().line ?? 0) - 1}px)`, left: `${d().inset ?? 12}px`, right: `${d().inset ?? 12}px` }} />}>
        {(b) => <i class="drop-box" style={{ transform: `translateY(${b().top}px)`, height: `${b().height}px` }} />}
      </Show>
    )}</Show>
  );
}

type RowSource = ReturnType<typeof useRows>;

/** One virtual row, bound to its id: content updates in place while the row exists. */
function ListRow(props: { id: string; list: RowSource }) {
  const row = () => props.list.byId().get(props.id);
  const top = () => props.list.startOf().get(props.id) ?? 0;
  return (
    <div class="vrow" data-type={row()?.type} data-key={props.id} style={{ transform: `translateY(${top()}px)`, height: `${props.list.heightOf(props.id)}px` }}
      role="treeitem" aria-selected={props.id === selectedKey()}>
      <Show when={row()?.type === "header" ? (row() as Extract<Row, { type: "header" }>) : undefined}>{(h) => <Header row={h()} />}</Show>
      <Show when={row()?.type === "child" ? (row() as Extract<Row, { type: "child" }>) : undefined}>
        {(c) => (
          <div class="rowhit" onClick={() => { setSelectedKey(c().parent.key); setSubagentOpen(c().child.id); }}>
            <ChildCard parent={c().parent} child={c().child} first={c().first} last={c().last} selected={selectedKey() === c().parent.key && subagentOpen() === c().child.id} />
          </div>
        )}
      </Show>
      <Show when={row()?.type === "creation" ? (row() as Extract<Row, { type: "creation" }>) : undefined}>{(v) => <CreationCard card={v().creation} />}</Show>
      <Show when={row()?.type === "card" ? (row() as Extract<Row, { type: "card" }>) : undefined}>
        {(v) => (
          <div class="rowhit" classList={{ "has-kids-open": v().expanded && !v().compactRow, multi: !!multi[v().session.key], dragged: dragItem()?.id === v().session.key }}
            draggable="true" onDragStart={(e) => startDrag(e, { kind: "session", id: v().session.key })}
            onMouseDown={(e) => { if (e.shiftKey) e.preventDefault(); }} onClick={(e) => selectSession(v().session.key, e)}
            onContextMenu={(e) => {
              // macOS turns Control-click into a context click; Control keeps its selection meaning here.
              if (e.ctrlKey && e.button === 0 && IS_MAC) { e.preventDefault(); selectSession(v().session.key, e); return; }
              if (multi[v().session.key] && selectedRows(listRows()).length > 1) { openMenu(e, selectionContextMenu(listRows())); return; }
              if (!multi[v().session.key]) selectSession(v().session.key); openMenu(e, sessionMenu(v().session));
            }}
            onDblClick={(e) => { if (!e.ctrlKey && !e.metaKey && !e.shiftKey) setRowCompact(v().session.key, !compactRows[v().session.key]); }}>
            <Show when={!v().compactRow} fallback={<CompactCard session={v().session} selected={selectedKey() === v().session.key || !!multi[v().session.key]} onExpand={() => setRowCompact(v().session.key, false)} />}>
              <SessionCard session={v().session} selected={(selectedKey() === v().session.key && !subagentOpen()) || !!multi[v().session.key]} expandable={v().expandable}
                expanded={!!expandedChildren[v().session.key]} onToggleChildren={() => setExpandedChildren(v().session.key, !expandedChildren[v().session.key])} />
            </Show>
          </div>
        )}
      </Show>
    </div>
  );
}

function NoMatches() {
  return (
    <div class="empty">
      <Icon name="funnel-x" size={28} />
      <h3>No matching sessions</h3>
      <p>The current filters hide every session. Filters change only this list, never the open conversation.</p>
      <button class="btn small" onClick={() => { setSearch(""); setStateFilter("all"); setMachineFilter([]); setProjectFilter([]); }}>Show all sessions</button>
    </div>
  );
}

/** Before the first rows: skeleton cards while a computer is still starting, then the empty states. */
function ListPlaceholder() {
  const computers = () => Object.values(catalog.computers);
  const waiting = () => !computers().length || computers().some((c) => c.status === "probing");
  const unsupported = () => computers().length > 0 && computers().every((c) => c.status === "unsupported");
  return (
    <Show when={!waiting()} fallback={
      <div class="sp-scroll">
        <For each={[0, 1, 2, 3, 4]}>
          {() => (
            <div class="skel-card">
              <div class="skeleton" style={{ width: "55%", height: "14px" }} />
              <div class="skeleton" style={{ width: "80%", height: "10px" }} />
              <div class="skeleton" style={{ width: "70%", height: "11px" }} />
              <div class="skeleton" style={{ width: "35%", height: "9px" }} />
            </div>
          )}
        </For>
      </div>
    }>
      <div class="empty sp-empty">
        <Icon name="sparkles" size={30} />
        <Show when={unsupported()} fallback={<>
          <h3>No sessions yet</h3>
          <p>Start an agent in a project folder on this computer or on a connected machine.</p>
        </>}>
          <h3>No event stream</h3>
          <p>This computer's hgs has no event stream (Event stream ABI: watch-v1). Update hgs to see its sessions here.</p>
        </Show>
      </div>
    </Show>
  );
}

/**
 * Collapsed list (SessionPanelDock Collapsed): one toggle, a search button as
 * wide as a tile, the active and most urgent filters meeting in the middle,
 * then the same rows at their heights: bands, 84 px square tiles, child rows.
 */
function Strip(props: { onEnter: () => void; onToggle: () => void }) {
  const list = useRows(true);
  // The active filter plus the first of attention, active, all that is not checked and has sessions.
  const shown = createMemo(() => {
    const urgent = (["attention", "active", "all"] as StateFilter[]).find((id) => id !== stateFilter() && (id === "all" || Number(FILTERS.find((f) => f.id === id)!.count()) > 0));
    return FILTERS.filter((f) => f.id === stateFilter() || f.id === urgent);
  });
  return (
    <div class="strip-col" onPointerEnter={props.onEnter}>
      <button class="icon-btn hd strip-toggle" data-tip="Expand session list" data-tip-pos="right" onClick={props.onToggle}><Icon name="expand-sessions" size={18} /></button>
      <button class="strip-search" data-tip="Search sessions" data-tip-pos="right" onClick={() => { setPeek(true); requestAnimationFrame(() => document.querySelector<HTMLInputElement>(".sp-panel.overlay .sp-search input")?.focus()); }}>
        <Icon name="search" size={16} />
      </button>
      <div class="strip-filters" classList={{ single: shown().length < 2 }}>
        <For each={shown()}>{(f) => (
          <button class="fchip" classList={{ on: stateFilter() === f.id, ...filterTone(f.id) }} aria-pressed={stateFilter() === f.id} data-tip={f.tipText?.() ?? `${f.tip} / ${f.count()}`} data-tip-pos="right" onClick={() => setStateFilter(f.id)}>
            <Icon name={f.icon} size={16} /><span class="tabular">{f.count()}</span>
          </button>
        )}</For>
      </div>
      <div class="strip-scroll" ref={list.ref}>
        <div style={{ height: `${list.virtualizer.getTotalSize()}px`, position: "relative" }}>
          <For each={list.keys()}>{(id) => <StripRow id={id} list={list} />}</For>
        </div>
      </div>
      <div class="strip-count tabular">{shownSummary().shown + creationCount()}</div>
    </div>
  );
}

function StripRow(props: { id: string; list: RowSource }) {
  const row = () => props.list.byId().get(props.id);
  const top = () => props.list.startOf().get(props.id) ?? 0;
  return (
    <div class="vrow" style={{ transform: `translateY(${top()}px)`, height: `${props.list.heightOf(props.id)}px` }}>
      <Show when={row()?.type === "header" ? (row() as Extract<Row, { type: "header" }>) : undefined}>{(h) => <StripBand row={h()} />}</Show>
      <Show when={row()?.type === "child" ? (row() as Extract<Row, { type: "child" }>) : undefined}>
        {(c) => (
          <button class="strip-child" title={`${c().child.name}\n${childStatus(c().child).caption}`}
            onClick={() => { setSelectedKey(c().parent.key); setSubagentOpen(c().child.id); }}>
            <StatusBadge kind={childStatus(c().child).kind} caption="" iconOnly />
            <span class="ell">{c().child.name}</span>
          </button>
        )}
      </Show>
      <Show when={row()?.type === "card" ? (row() as Extract<Row, { type: "card" }>) : undefined}>{(v) => <StripTile row={v()} />}</Show>
      <Show when={row()?.type === "creation" ? (row() as Extract<Row, { type: "creation" }>) : undefined}>{(v) => <CreationTile card={v().creation} />}</Show>
    </div>
  );
}

/** Header band in the strip: one counter (attention > unread > working > total), name in the rest. */
function StripBand(props: { row: Extract<Row, { type: "header" }> }) {
  const name = () => props.row.name;
  const counter = () => props.row.attention ? { text: `! ${props.row.attention}`, cls: "hot" } : props.row.unread ? { text: `● ${props.row.unread}`, cls: "hot" }
    : props.row.working ? { text: `● ${props.row.working}`, cls: "working" } : { text: String(props.row.total), cls: "total" };
  return (
    <button class="group-band strip-band entity" style={{ "--entity": projectColor(props.row) }} title={name()}
      onClick={() => setProjectCollapsed(props.row.project, !props.row.collapsed)}>
      <span class="ell">{name()}</span>
      <span class={`gb-count ${counter().cls}`}>{counter().text}</span>
    </button>
  );
}

/** A square tile: status and provider marks, name on up to two lines, state; the frame carries the state colour. */
function StripTile(props: { row: Extract<Row, { type: "card" }> }) {
  const s = () => props.row.session;
  const kind = () => badgeKind(s());
  const frame = () => (emphasis(s()) === "draft" ? "draft" : kind() === "muted-attention" || kind() === "muted-unread" ? "muted" : kind() === "unread" ? "attention" : kind());
  const tip = () => [`${s().name}: ${statusCaption(s())}`, s().summary, `${folderOf(s())}${s().branch ? ` / ${s().branch}` : ""} on ${s().machine}`, s().git ? gitTooltip(s()) : "", s().optionalQuestions ? `${s().optionalQuestions} optional question(s) waiting` : ""].filter(Boolean).join("\n");
  return (
    <button class="strip-tile" classList={{ selected: selectedKey() === s().key }} data-frame={frame()} title={tip()} onClick={() => selectSession(s().key)}
      onContextMenu={(e) => { selectSession(s().key); openMenu(e, sessionMenu(s())); }}>
      <span class="st-block">
        <span class="st-top">
          <span class="st-status">
            <StatusBadge kind={kind()} caption="" iconOnly />
            <Show when={visibleUnreadReply(s(), computerFresh(s().computer)) && (s().status === "working" || s().status === "attention" || s().draft)}><i class="st-unread" classList={{ muted: !!s().muted }} /></Show>
          </span>
          <Show when={s().muted}><Icon name="mute" size={14} class="st-mute" /></Show>
          <span class="st-provider">
            <AgentBadge agent={s().agent} narrow />
            <Show when={s().optionalQuestions}><i class="st-question" classList={{ muted: !!s().muted }}>?</i></Show>
          </span>
        </span>
        <span class="st-name">{s().title}</span>
        <span class="st-state" data-tone={kind()}>{stripCaption(s())}</span>
      </span>
    </button>
  );
}
