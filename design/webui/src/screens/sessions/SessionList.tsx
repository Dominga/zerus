import { For, Show, createMemo, createEffect, on } from "solid-js";
import { createVirtualizer } from "@tanstack/solid-virtual";
import { Icon, AgentGlyph } from "../../lib/icons";
import { listRows, rowHeight, shownSummary, type Row } from "./model";
import { SessionCard, ChildCard, CompactCard, AgentBadge, MachineBadge, StatusBadge, badgeKind, statusCaption } from "./SessionCard";
import { projects, machines, accounts, archived } from "../../mock/data";
import {
  selectedKey, setSelectedKey, setDetailTab, collapsed, setCollapsed, setExpandedChildren, expandedChildren,
  setCompactRows, compactRows, compactList, stateFilter, setStateFilter, counts, search, setSearch, stripMode, setStripMode,
  setMenu, setDialog, toast, demoState, machineFilter, setMachineFilter, projectFilter, setProjectFilter, accountFilter, setAccountFilter,
  setPage, multi, setMulti, peek, setPeek, catalog, setSubagentOpen,
  type StateFilter, type MenuEntry,
} from "../../state";
import type { Session } from "../../mock/types";

// Task cards dragged from the Tasks tab (MIME application/x-zerus-task) can be
// dropped on a live session row to retarget the task, or on a project header
// to set "Any session" (SessionList.cpp task drop rules).
const TASK_MIME = "application/x-zerus-task";
const acceptsTask = (e: DragEvent) => !!e.dataTransfer?.types.includes(TASK_MIME);
function taskDragOver(e: DragEvent) {
  if (!acceptsTask(e)) return;
  e.preventDefault();
  (e.currentTarget as HTMLElement).classList.add("task-drop");
}
const taskDragLeave = (e: DragEvent) => (e.currentTarget as HTMLElement).classList.remove("task-drop");
function taskDrop(e: DragEvent, target: string) {
  if (!acceptsTask(e)) return;
  e.preventDefault();
  (e.currentTarget as HTMLElement).classList.remove("task-drop");
  toast("Task retargeted", `${e.dataTransfer?.getData(TASK_MIME) || "Task"} → ${target}. Sends only when you press Send.`);
}

// Five state filters, in SessionsWindow order, then the scope filters.
const FILTERS: { id: StateFilter; icon: string; tip: string; count: () => string }[] = [
  { id: "all", icon: "rows-3", tip: "All sessions", count: () => String(counts().all) },
  { id: "attention", icon: "bell", tip: "Needs attention", count: () => String(counts().attention) },
  { id: "active", icon: "zap", tip: "Active: working, scheduled, waiting, needs attention or a draft", count: () => String(counts().active) },
  { id: "paused", icon: "pause", tip: "Saved sessions (paused or stopped)", count: () => String(counts().paused) },
  { id: "archived", icon: "archive", tip: "Archive", count: () => `${counts().archived}+` },
];

const openMenu = (e: MouseEvent, items: MenuEntry[]) => {
  e.preventDefault();
  e.stopPropagation();
  setMenu({ x: e.clientX, y: e.clientY, items });
};

function sessionMenu(s: Session): MenuEntry[] {
  const live = s.status !== "paused" && s.status !== "ended";
  return [
    { label: compactRows[s.key] ? "Expand session row" : "Compact session row", icon: compactRows[s.key] ? "maximize-2" : "rows-2", action: () => setCompactRows(s.key, !compactRows[s.key]) },
    { label: "Expand all collapsed", icon: "rows-3", disabled: !Object.values(compactRows).some(Boolean), action: () => Object.keys(compactRows).forEach((k) => setCompactRows(k, false)) },
    { separator: true },
    s.agent === "deepseek" ? { label: "Open native DeepSeek UI", icon: "external-link" } : { label: "Open terminal", icon: "square-terminal", action: () => setDetailTab("terminal") },
    ...(s.status === "background" ? [{ label: "Show running commands", icon: "activity", action: () => setDetailTab("processes") } as MenuEntry] : []),
    { label: "Open folder", icon: "folder-open", disabled: s.machine !== "workstation" },
    { label: "Open shell in session folder", icon: "terminal" },
    { label: "Copy connect command", icon: "copy", action: () => toast("Copied", "hgs @… attach command copied") },
    { separator: true },
    { label: s.muted ? "Unmute" : "Mute", icon: s.muted ? "bell" : "bell-off" },
    s.unread || s.status === "attention" ? { label: "Mark as read", icon: "check-check" } : { label: "Mark needs attention", icon: "bell" },
    { separator: true },
    { label: live ? "Pause session" : "Resume session", icon: live ? "pause" : "play" },
    { label: live ? "Clear session…" : "Start fresh…", icon: "rotate-ccw", action: () => setDialog("clear-session") },
    { label: "Rename…", icon: "pencil", kbd: "F2", action: () => setDialog("rename") },
    { label: "Fork session…", icon: "git-fork", action: () => setDialog("fork") },
    { label: "Move to archive", icon: "archive", disabled: live },
    { label: "Move to project…", icon: "folder", action: () => setDialog("move-project") },
    { separator: true },
    { label: "Copy session name", icon: "copy" },
    { label: "Copy session reference", icon: "link", disabled: s.status === "offline", action: () => toast("Session reference copied", "zerus://session?machine=…&name=…") },
    { label: "Copy folder path", icon: "copy" },
    { separator: true },
    live ? { label: "Terminate session…", icon: "circle-stop", danger: true, action: () => setDialog("terminate") } : { label: "Forget saved session…", icon: "trash", danger: true, action: () => setDialog("forget") },
  ];
}

function headerMenu(project: string): MenuEntry[] {
  return [
    { label: "New session…", icon: "plus", action: () => setDialog("new-session") },
    { separator: true },
    { label: "Filter this project", icon: "funnel", action: () => setProjectFilter([project]) },
    { separator: true },
    { label: collapsed[project] ? "Expand project" : "Collapse project", icon: "list-tree", action: () => setCollapsed(project, !collapsed[project]) },
    { label: "Rename project…", icon: "pencil" },
    { label: "Project settings…", icon: "settings", action: () => setPage("projects") },
    { separator: true },
    { label: "Move project up", icon: "arrow-up" },
    { label: "Move project down", icon: "arrow-down" },
    { label: "Sort sessions by name", icon: "list-filter" },
    { separator: true },
    { label: "Delete project…", icon: "trash", danger: true, action: () => setPage("projects") },
  ];
}

function batchMenu(): MenuEntry[] {
  return [
    ...machines.filter((m) => m.connection !== "offline").map((m) => ({ heading: m.thisComputer ? `${m.alias} (this machine)` : m.alias } as MenuEntry)).flatMap((h) => [
      h,
      { label: "Pause all (3)", icon: "pause", disabled: true } as MenuEntry,
      { label: "Resume all in background (1)", icon: "play" } as MenuEntry,
    ]).slice(0, 6),
    { separator: true },
    { label: "Expand all collapsed", icon: "rows-3", action: () => Object.keys(compactRows).forEach((k) => setCompactRows(k, false)) },
    { label: `Mark all as read (${catalog.rows.filter((s) => s.unread).length})`, icon: "check-check" },
    { label: "Clear archive…", icon: "trash", danger: true },
  ];
}

function Header(props: { row: Extract<Row, { type: "header" }> }) {
  const p = () => projects.find((x) => x.id === props.row.project)!;
  return (
    <button class="group-band entity" style={{ "--entity": `var(--z-entity-palette-${p().colorIndex})` }}
      onDragOver={taskDragOver} onDragLeave={taskDragLeave} onDrop={(e) => taskDrop(e, "Any session")}
      aria-expanded={!collapsed[props.row.project]} onClick={() => setCollapsed(props.row.project, !collapsed[props.row.project])}
      onContextMenu={(e) => openMenu(e, headerMenu(props.row.project))}
      title={`${p().name}: ${props.row.total} shown, ${props.row.attention} need a response, ${props.row.working} working, ${props.row.unread} unread replies. Click to collapse or expand; drag to reorder.`}>
      <Icon name="chevron-down" size={13} class="gb-chev" />
      <Icon name="folder" size={15} stroke={1.7} />
      <span class="gb-name ell">{p().name}</span>
      <Show when={props.row.working}><span class="gb-count working">● {props.row.working}</span></Show>
      <Show when={props.row.unread}><span class="gb-count hot">● {props.row.unread}</span></Show>
      <Show when={props.row.attention}><span class="gb-count hot">! {props.row.attention}</span></Show>
      <span class="gb-count total">{props.row.total}</span>
    </button>
  );
}

function selectSession(key: string, e?: MouseEvent) {
  if (e && (e.ctrlKey || e.metaKey)) {
    setMulti(key, !multi[key]);
    return;
  }
  if (e?.shiftKey) {
    setMulti(key, true);
    return;
  }
  Object.keys(multi).forEach((k) => setMulti(k, false));
  setSelectedKey(key);
  setSubagentOpen(undefined);
  setDetailTab("activity");
  if (peek()) setPeek(false);
}

const multiCount = () => Object.values(multi).filter(Boolean).length;

function ScopeButton(props: { icon: string; label: string; selected: string[]; set: (v: string[]) => void; choices: { id: string; label: string; count: number; entity?: number }[] }) {
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
  const chips = () => [
    ...machineFilter().map((m) => ({ id: m, label: m, kind: "m" as const, idx: machines.find((x) => x.alias === m)?.colorIndex ?? 0 })),
    ...projectFilter().map((p) => ({ id: p, label: projects.find((x) => x.id === p)?.name ?? "Unavailable project", kind: "p" as const, idx: projects.find((x) => x.id === p)?.colorIndex ?? 0 })),
    ...accountFilter().map((a) => ({ id: a, label: accounts.find((x) => x.id === a)?.label ?? "Unavailable account", kind: "a" as const, idx: 6 })),
  ];
  const remove = (c: { id: string; kind: "m" | "p" | "a" }) =>
    c.kind === "m" ? setMachineFilter(machineFilter().filter((x) => x !== c.id)) : c.kind === "p" ? setProjectFilter(projectFilter().filter((x) => x !== c.id)) : setAccountFilter(accountFilter().filter((x) => x !== c.id));
  return (
    <Show when={chips().length}>
      <div class="filter-chips">
        <For each={chips()}>{(c) => (
          <button class="fchip-sel entity" style={{ "--entity": `var(--z-entity-palette-${c.idx})` }} title="Remove filter" onClick={() => remove(c)}>
            <span class="ell">{c.label}</span><Icon name="x" size={11} />
          </button>
        )}</For>
      </div>
    </Show>
  );
}

/** Content search results page: metadata matches at once, message hits per host. */
function SearchResults() {
  const q = () => search().trim();
  const hits = createMemo(() => {
    const needle = q().toLowerCase();
    const all = [...catalog.rows.slice(0, 200), ...archived];
    return all.filter((s) => `${s.name} ${s.summary} ${s.machine} ${s.model} ${s.branch ?? ""}`.toLowerCase().includes(needle)).slice(0, 30);
  });
  const mark = (text: string) => {
    const i = text.toLowerCase().indexOf(q().toLowerCase());
    if (i < 0 || !q()) return text;
    return <>{text.slice(0, i)}<mark>{text.slice(i, i + q().length)}</mark>{text.slice(i + q().length)}</>;
  };
  return (
    <div class="sp-scroll search-results">
      <div class="search-status">
        <Show when={q().length >= 2} fallback={<span>Type 2+ characters to search message contents</span>}>
          <span>{hits().length} {hits().length === 1 ? "message" : "messages"}, {Math.min(hits().length, 9)} {Math.min(hits().length, 9) === 1 ? "session" : "sessions"} — build-02: machine offline; contents unavailable</span>
        </Show>
        <span class="grow" />
        <button class="icon-btn small" title="Search again"><Icon name="refresh-cw" size={12} /></button>
      </div>
      <For each={hits()} fallback={<div class="empty"><Icon name="search" size={26} /><h3>No matching sessions</h3><p>Nothing matches “{q()}” in names, IDs or messages with the current filters.</p></div>}>
        {(s) => (
          <button class="search-hit" classList={{ selected: selectedKey() === s.key }} onClick={() => selectSession(s.key)}>
            <div class="row"><b class="ell">{mark(s.name)}</b><span class="grow" /><AgentBadge agent={s.agent} narrow /><MachineBadge alias={s.machine} /></div>
            <div class="sh-meta">{s.status === "ended" ? "Archive / Agent / 24 Oct 17:12" : `Agent / 27 Oct ${10 + (s.name.length % 3)}:${String(s.name.length * 7 % 60).padStart(2, "0")}`}</div>
            <div class="sh-snippet">{mark(s.summary)}</div>
          </button>
        )}
      </For>
    </div>
  );
}

function SelectionBar() {
  return (
    <Show when={multiCount() > 1}>
      <div class="selection-bar" role="toolbar" aria-label="Selected sessions">
        <div class="row"><b>{multiCount()} selected</b><span class="grow" /><button class="icon-btn small" title="Deselect all (Esc)" onClick={() => Object.keys(multi).forEach((k) => setMulti(k, false))}><Icon name="x" size={14} /></button></div>
        <div class="row sel-actions">
          <button class="icon-btn" title={`Mark as read (${multiCount()})`}><Icon name="check-check" size={18} /></button>
          <button class="icon-btn" title="Pause (1)"><Icon name="pause" size={18} /></button>
          <button class="icon-btn" title="Compact session rows"><Icon name="rows-2" size={18} /></button>
          <button class="icon-btn" title="Move to project" onClick={() => setDialog("move-project")}><Icon name="folder" size={18} /></button>
          <button class="icon-btn" title="Move to archive"><Icon name="archive" size={18} /></button>
          <button class="icon-btn danger" title={`Terminate (${multiCount()})`} onClick={() => setDialog("terminate")}><Icon name="circle-stop" size={18} /></button>
          <button class="icon-btn" title="More actions for selected sessions"><Icon name="chevron-down" size={18} /></button>
        </div>
      </div>
    </Show>
  );
}

export function SessionList() {
  let scroller!: HTMLDivElement;
  const rows = listRows;
  const virtualizer = createVirtualizer({
    get count() { return rows().length; },
    getScrollElement: () => scroller,
    estimateSize: (i) => rowHeight(rows()[i], compactList()),
    getItemKey: (i) => rows()[i].id,
    overscan: 6,
  });
  createEffect(on([rows, compactList], () => virtualizer.measure(), { defer: true }));
  const items = createMemo(() => virtualizer.getVirtualItems());
  const searching = () => search().trim().length > 0 && stateFilter() !== "archived";
  const expanded = () => !stripMode() || peek();

  const onKey = (e: KeyboardEvent) => {
    const list = rows().filter((r) => r.type !== "header");
    const i = list.findIndex((r) => r.id === selectedKey());
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const next = list[Math.max(0, Math.min(list.length - 1, i + (e.key === "ArrowDown" ? 1 : -1)))];
      if (next) {
        setSelectedKey(next.id);
        virtualizer.scrollToIndex(rows().indexOf(next), { align: "auto" });
      }
    } else if ((e.key === "ArrowRight" || e.key === "ArrowLeft") && i >= 0) {
      const r = list[i];
      if (r.type === "card" && r.compactRow && e.key === "ArrowRight") setCompactRows(r.session.key, false);
      else if (r.type === "card" && r.expandable) setExpandedChildren(r.session.key, e.key === "ArrowRight");
      else if (r.type === "child" && e.key === "ArrowLeft") setSelectedKey(r.parent.key);
    } else if (e.key === "Escape") {
      Object.keys(multi).forEach((k) => setMulti(k, false));
    }
  };

  return (
    <section class="sessions-pane" classList={{ strip: stripMode(), peek: stripMode() && peek() }} aria-label="Sessions"
      onMouseLeave={() => stripMode() && peek() && setTimeout(() => setPeek(false), 350)}>
      <Show when={stripMode()}><Strip /></Show>
      <div class="sp-panel" classList={{ overlay: stripMode() }} hidden={!expanded()}>
        <header class="sp-head">
          <h1>{stateFilter() === "archived" ? "Archive" : "Sessions"}</h1>
          <span class="grow" />
          <button class="icon-btn" data-tip="New session (Ctrl+N)" onClick={() => setDialog("new-session")}><Icon name="plus" size={16} /></button>
          <button class="icon-btn" data-tip={stripMode() ? "Keep session list open" : "Collapse session list"} onClick={() => { setPeek(false); setStripMode(!stripMode()); }}>
            <Icon name={stripMode() ? "pin" : "panel-right-close"} size={15} style={stripMode() ? undefined : { transform: "scaleX(-1)" }} />
          </button>
          <button class="icon-btn" data-tip="Session and machine actions" onClick={(e) => openMenu(e, batchMenu())}><Icon name="ellipsis" size={16} /></button>
        </header>
        <div class="sp-search">
          <Icon name="search" size={14} class="sp-search-ico" />
          <input class="field" type="search" placeholder={stateFilter() === "archived" ? "Search archive names and IDs…" : "Search sessions, IDs and messages…"} value={search()} onInput={(e) => setSearch(e.currentTarget.value)}
            onKeyDown={(e) => e.key === "Escape" && setSearch("")} />
        </div>
        <div class="sp-filters" role="toolbar" aria-label="State filter">
          <For each={FILTERS}>
            {(f) => (
              <button class="fchip" classList={{ on: stateFilter() === f.id, hot: f.id === "attention" && counts().attention > 0 }} data-tip={`${f.tip} / ${f.count()}`}
                aria-pressed={stateFilter() === f.id} onClick={() => setStateFilter(f.id)}>
                <Icon name={f.icon} size={13} />
                <span class="tabular">{f.count()}</span>
              </button>
            )}
          </For>
          <span class="grow" />
          <ScopeButton icon="monitor" label="machines" selected={machineFilter()} set={setMachineFilter}
            choices={machines.map((m) => ({ id: m.alias, label: `${m.alias}${m.thisComputer ? " (this machine)" : m.connection === "offline" ? " (offline)" : ""}`, count: catalog.rows.filter((s) => s.machine === m.alias).length }))} />
          <ScopeButton icon="folder" label="projects" selected={projectFilter()} set={setProjectFilter}
            choices={projects.map((p) => ({ id: p.id, label: p.name, count: catalog.rows.filter((s) => s.project === p.id).length }))} />
          <ScopeButton icon="user-round-cog" label="accounts" selected={accountFilter()} set={setAccountFilter}
            choices={accounts.map((a) => ({ id: a.id, label: `${a.label} · ${a.agent}`, count: a.sessions }))} />
        </div>
        <FilterChips />

        <Show when={stateFilter() !== "archived"} fallback={<ArchiveList />}>
          <Show when={!searching()} fallback={<SearchResults />}>
            <Show when={demoState() === "normal" || demoState() === "error"} fallback={<ListPlaceholder />}>
              <div class="sp-scroll" ref={scroller} tabindex="0" onKeyDown={onKey} role="tree"
                aria-label="Agent sessions; Ctrl-click to select several; drag to organize, left/right to collapse or expand a group">
                <Show when={rows().length} fallback={<NoMatches />}>
                  <div style={{ height: `${virtualizer.getTotalSize()}px`, position: "relative" }}>
                    <For each={items()}>
                      {(item) => {
                        const row = () => rows()[item.index];
                        return (
                          <div class="vrow" data-type={row()?.type} style={{ transform: `translateY(${item.start}px)`, height: `${item.size}px` }}
                            role="treeitem" aria-selected={row()?.id === selectedKey()}>
                            <Show when={row()}>
                              {(r) => {
                                const v = r();
                                if (v.type === "header") return <Header row={v} />;
                                if (v.type === "child")
                                  return (
                                    <div class="rowhit" onClick={() => { setSelectedKey(v.parent.key); setSubagentOpen(v.child.id); setDetailTab("activity"); }}>
                                      <ChildCard parent={v.parent} child={v.child} last={v.last} selected={false} />
                                    </div>
                                  );
                                return (
                                  <div class="rowhit" classList={{ "has-kids-open": v.expanded, multi: !!multi[v.session.key] }} onClick={(e) => selectSession(v.session.key, e)}
                                    onDragOver={taskDragOver} onDragLeave={taskDragLeave} onDrop={(e) => taskDrop(e, v.session.name)}
                                    onContextMenu={(e) => { if (!multi[v.session.key]) selectSession(v.session.key); openMenu(e, sessionMenu(v.session)); }}
                                    onDblClick={() => setCompactRows(v.session.key, !compactRows[v.session.key])}>
                                    <Show when={!v.compactRow} fallback={<CompactCard session={v.session} selected={selectedKey() === v.session.key || !!multi[v.session.key]} onExpand={() => setCompactRows(v.session.key, false)} />}>
                                      <SessionCard session={v.session} selected={selectedKey() === v.session.key || !!multi[v.session.key]} expandable={v.expandable}
                                        expanded={!!expandedChildren[v.session.key]} onToggleChildren={() => setExpandedChildren(v.session.key, !expandedChildren[v.session.key])} />
                                    </Show>
                                  </div>
                                );
                              }}
                            </Show>
                          </div>
                        );
                      }}
                    </For>
                  </div>
                </Show>
              </div>
            </Show>
          </Show>
        </Show>
        <SelectionBar />
        <footer class="sp-foot">
          <span class="tabular ell">{searching() ? "" : shownSummary().shown ? `${shownSummary().shown} shown, ${shownSummary().working} working, ${shownSummary().attention} attention, 1 starting` : "No matching sessions"}</span>
          <span class="grow" />
          <button class="icon-btn small" data-tip="Session creation status" onClick={(e) => openMenu(e, [{ heading: "Session creations" }, { label: "orders-v3 · Preparing worktree · 0:41", icon: "loader-circle" }, { label: "docs-index · Waiting for session", icon: "clock" }])}><Icon name="clock" size={13} /></button>
          <button class="icon-btn small" data-tip="Mark all as read: current replies on every machine" onClick={() => toast("Marked as read", `${catalog.rows.filter((s) => s.unread).length} replies marked as read.`)}><Icon name="check-check" size={14} /></button>
          <button class="icon-btn small" data-tip="Saved drafts: recover local drafts, including those from ended sessions" onClick={() => setDialog("drafts")}><Icon name="pencil" size={13} /></button>
        </footer>
      </div>
    </section>
  );
}

function NoMatches() {
  return (
    <div class="empty">
      <Icon name="funnel-x" size={28} />
      <h3>No matching sessions</h3>
      <p>The current filters hide every session. Filters change only this list, never the open conversation.</p>
      <button class="btn small" onClick={() => { setSearch(""); setStateFilter("all"); setMachineFilter([]); setProjectFilter([]); setAccountFilter([]); }}>Show all sessions</button>
    </div>
  );
}

function ListPlaceholder() {
  return (
    <Show when={demoState() === "loading"} fallback={
      <div class="empty sp-empty">
        <Icon name="sparkles" size={30} />
        <h3>No sessions yet</h3>
        <p>Start an agent in a project folder on this computer or on a connected machine.</p>
        <button class="btn primary" onClick={() => setDialog("new-session")}><Icon name="plus" size={14} />New session</button>
        <button class="btn ghost small" onClick={() => setPage("machines")}>Connect a machine…</button>
      </div>
    }>
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
    </Show>
  );
}

function ArchiveList() {
  return (
    <div class="sp-scroll archive">
      <div class="archive-bar">
        <button class="btn small" onClick={(e) => openMenu(e, [{ heading: "Archived date" }, { label: "All dates", checked: true }, { label: "Today", checked: false }, { label: "Last 7 days", checked: false }, { label: "Last 30 days", checked: false }, { separator: true }, { label: "Custom range…", icon: "calendar-days" }])}>
          <Icon name="calendar-days" size={13} />All dates
        </button>
        <button class="icon-btn small" title="Refresh"><Icon name="refresh-cw" size={12} /></button>
        <span class="hint grow">1–4 of 4</span>
        <button class="icon-btn small" title="Newer" disabled><Icon name="chevron-left" size={13} /></button>
        <button class="icon-btn small" title="Older"><Icon name="chevron-right" size={13} /></button>
      </div>
      <For each={archived}>
        {(s) => (
          <div class="rowhit archived-row" onClick={() => selectSession(s.key)}>
            <div class="scard compact archived" classList={{ selected: selectedKey() === s.key }}>
              <span class="sc-title ell">{s.name}</span>
              <span class="hint ell grow">{s.summary} · {s.archivedAt}</span>
              <AgentBadge agent={s.agent} narrow />
              <button class="btn small" onClick={(e) => { e.stopPropagation(); toast("Restore", `${s.name} returns to its project`); }}>Restore</button>
            </div>
          </div>
        )}
      </For>
      <div class="hint" style={{ padding: "10px 6px" }}>One page of 50 per machine. build-02: machine offline; contents unavailable.</div>
    </div>
  );
}

/** Collapsed list: square tiles (84 or 94 px) in a 98/108 px strip. */
function Strip() {
  return (
    <div class="strip-col" onMouseEnter={() => setTimeout(() => stripMode() && setPeek(true), 150)}>
      <button class="strip-search" data-tip="Search sessions" data-tip-pos="right" onClick={() => setPeek(true)}><Icon name="search" size={15} /></button>
      <div class="strip-filters">
        <button class="fchip on"><Icon name="rows-3" size={12} /><span>{counts().all}</span></button>
        <Show when={counts().attention}><button class="fchip hot" onClick={() => setStateFilter("attention")}><Icon name="bell" size={12} /><span>{counts().attention}</span></button></Show>
      </div>
      <div class="strip-scroll">
        <For each={listRows()}>
          {(r) => r.type === "header" ? (
            <div class="strip-band entity" style={{ "--entity": `var(--z-entity-palette-${projects.find((p) => p.id === r.project)?.colorIndex ?? 0})` }}
              title={projects.find((p) => p.id === r.project)?.name}>
              <span class="ell">{projects.find((p) => p.id === r.project)?.name}</span>
              <span class="gb-count" classList={{ hot: !!(r.attention || r.unread), working: !r.attention && !r.unread && !!r.working, total: !r.attention && !r.unread && !r.working }}>
                {r.attention ? `! ${r.attention}` : r.unread ? `● ${r.unread}` : r.working ? `● ${r.working}` : r.total}
              </span>
            </div>
          ) : r.type === "card" ? (
            <button class="strip-tile" classList={{ selected: selectedKey() === r.session.key }} data-state={badgeKind(r.session)}
              title={`${r.session.name} · ${statusCaption(r.session)}\n${r.session.summary}\n${r.session.machine}`} onClick={() => selectSession(r.session.key)}>
              <span class="st-top">
                <StatusBadge kind={badgeKind(r.session)} caption="" />
                <span class="st-agent"><AgentGlyph agent={r.session.agent} size={12} /></span>
              </span>
              <span class="st-name">{r.session.name}</span>
              <span class="st-state">{r.session.status === "working" ? `${r.session.turnStartedMin}m` : statusCaption(r.session)}</span>
              <Show when={r.session.unread}><i class="st-unread" /></Show>
            </button>
          ) : null}
        </For>
      </div>
      <div class="strip-count tabular">{shownSummary().shown}</div>
    </div>
  );
}
