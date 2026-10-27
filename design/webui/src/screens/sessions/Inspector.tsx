import { For, Show, Switch, Match, createMemo, createSignal, type JSX } from "solid-js";
import { createStore } from "solid-js/store";
import { Icon } from "../../lib/icons";
import { selected, inspectorTab, setInspectorTab, setInspectorOpen, setSelectedKey, setDetailTab, setMenu, toast, setDialog, inspectorWidth, setInspectorWidth, INSPECTOR_WIDTH, type InspectorTab } from "../../state";
import { accounts, projects } from "../../mock/data";
import { catalogFor, type Checkout } from "../../mock/setup-extra";
import { statusLine } from "../setup/WorktreePanel";
import { openSetup } from "../setup/ui";
import { setCleanupFocus, setLaunchPreset } from "../setup/store";
import { TasksPanel, TaskTabBadge } from "../tasks/TasksPanel";

const TABS: { id: InspectorTab; label: string }[] = [
  { id: "plan", label: "Plan & agents" },
  { id: "details", label: "Details" },
  { id: "worktrees", label: "Worktrees" },
  { id: "tasks", label: "Tasks" },
];

// ---------------------------------------------------------------------------
// Grouped blocks: a raised rounded card with a bold title and a chevron that
// collapses it. Collapsed blocks are remembered while the window lives.

const [closed, setClosed] = createStore<Record<string, boolean>>({});

function Card(props: { id: string; title: JSX.Element; aside?: JSX.Element; children: JSX.Element; class?: string; classList?: Record<string, boolean>; onContextMenu?: (e: MouseEvent) => void }) {
  const open = () => !closed[props.id];
  const body = `icard-${props.id}`;
  return (
    <section class={`icard ${props.class ?? ""}`} classList={{ closed: !open(), ...props.classList }} onContextMenu={props.onContextMenu}>
      <header class="icard-head">
        <button class="icard-toggle" aria-expanded={open()} aria-controls={body} onClick={() => setClosed(props.id, open())}>
          <span class="icard-title">{props.title}</span>
          <Icon name="chevron-down" size={12} class="icard-chev" />
        </button>
        <Show when={props.aside}><span class="icard-aside">{props.aside}</span></Show>
      </header>
      <Show when={open()}><div class="icard-body" id={body}>{props.children}</div></Show>
    </section>
  );
}

/** One line: a 16 px muted icon, the value and a right-aligned muted label. */
function Row(props: { icon?: string; lead?: JSX.Element; label?: string; title?: string; mono?: boolean; wrap?: boolean; tail?: JSX.Element; children: JSX.Element; class?: string }) {
  return (
    <div class={`irow ${props.class ?? ""}`} title={props.title}>
      <span class="irow-ico">{props.lead ?? <Icon name={props.icon ?? "circle"} size={16} stroke={1.75} />}</span>
      <span class="irow-val" classList={{ mono: props.mono, ell: !props.wrap }}>{props.children}</span>
      {props.tail}
      <Show when={props.label}><span class="irow-label">{props.label}</span></Show>
    </div>
  );
}

// ---------------------------------------------------------------------------

function PlanAgents() {
  const s = () => selected();
  const kids = () => s()?.subagents ?? [];
  const status = () => (s()?.status === "working" ? "Using a tool" : "Ready");
  return (
    <div class="insp-body">
      <Card id="plan-main" title="Main agent" aside={<span class={s()?.status === "working" ? "accent" : "muted"}>{status()}</span>}>
        <Row icon="activity" label="status">{status()}</Row>
        <Row icon="terminal" label="action" mono title={s()?.summary}>{s()?.summary}</Row>
      </Card>
      <Show when={s()?.goal}>
        <Card id="plan-goal" title="Goal" class="goal" aside={<span class="sbadge k-scheduled"><i class="sb-ico" /><span class="sb-label">Pursuing goal</span></span>}>
          <p class="icard-text">{s()?.goal}</p>
          <Row icon="clock" label="recorded">1 h 12 min · 38 turns</Row>
          <div class="hint">Reported by the agent. Completion: all v1 contract tests pass.</div>
        </Card>
      </Show>
      <Card id="plan-tasks" title="Tasks" aside={<span class="dim">2 of 3 completed (last recorded)</span>}>
        <Row class="done" icon="check" label="Done">Inspect routes and existing clients</Row>
        <Row class="done" icon="check" label="Done">Refactor handlers and add tests</Row>
        <Row class="doing" icon="circle-dot" label="In progress">Review macOS and server behavior</Row>
      </Card>
      <Card id="plan-agents" title="Subagents" aside={<span class="dim">{kids().filter((k) => k.state === "working").length} active / {kids().length} total</span>}>
        <Show when={kids().length} fallback={<div class="hint">No subagent activity has been reported yet.</div>}>
          <For each={kids()}>{(k) => (
            <Row lead={<span class={`state-dot ${k.state}`} />} label={k.state === "working" ? "Working" : k.state === "ready" ? "Ready" : "Needs input"}
              class={`agent ${k.state}`} title={k.summary}>
              <button class="link" onClick={() => setSelectedKey(`${s()!.key}/${k.id}`)}>{k.name} →</button>
              <span class="dim"> {k.summary}</span>
            </Row>
          )}</For>
        </Show>
      </Card>
    </div>
  );
}

const USAGE = [["Total tokens", "1.84M"], ["Input (including cache)", "1.71M"], ["Uncached input", "142k"], ["Cache read", "1.52M"], ["Cache write", "48k"], ["Output", "126k"], ["Reasoning (in output)", "61k"], ["Recorded requests", "212"], ["Native cost estimate", "$18.40"], ["Tool time", "41 min"], ["Lines added / removed", "+1 204 / −388"], ["Latest context", "72k of 200k"]];

function Details() {
  const s = () => selected();
  const acc = () => accounts.find((a) => a.id === s()?.account);
  const v = (value: string | undefined) => value ?? "Not reported";
  return (
    <div class="insp-body">
      <Card id="details-id" title="Session ID">
        <Row icon="key-round" mono title="Session ID"
          tail={<button class="icon-btn small" data-tip="Copy session ID" data-tip-pos="left" aria-label="Copy session ID" onClick={() => toast("Copied", "Session ID copied")}><Icon name="copy" size={13} /></button>}>
          <span class="selectable">5c1e9b2a-7d40-4e6b-9a1f-2b8c0d3e4f51</span>
        </Row>
      </Card>
      <Card id="details-session" title="Session">
        <Row icon="message-square" label="name" mono title={`${s()?.agent}/${s()?.project}/${s()?.name}`}>{s()?.agent}/{s()?.project}/{s()?.name}</Row>
        <Row icon="monitor" label="machine">{v(s()?.machine)}</Row>
        <Row icon="folder" label="directory" mono title={s()?.cwd}>{v(s()?.cwd)}</Row>
        <Row icon="git-branch" label="branch">{v(s()?.branch)}</Row>
        <Row icon="layers" label="project">{v(projects.find((p) => p.id === s()?.project)?.name)}</Row>
        <Row icon="clock" label="last event" title="Last recorded event">Tue, 27 Oct 11:43:08</Row>
        <div class="hint">Activity stays on {s()?.machine}. Closing this window does not stop your agent.</div>
      </Card>
      <Card id="details-account" title="Account">
        <Row icon="user-round-cog" label="account">{acc() ? `${acc()!.label} · ${acc()!.plan}` : "Not reported"}</Row>
        <Row icon="hard-drive" label="agent home" mono>~/.config/hgs/accounts/claude-a1</Row>
        <Row icon="sparkles" label="model">{s()?.model} {s()?.effort ?? ""}</Row>
      </Card>
      <Card id="details-usage" title="Session usage">
        <table class="usage">
          <tbody>
            <For each={USAGE}>{([k, val]) => <tr><td>{k}</td><td class="tabular">{val}</td></tr>}</For>
          </tbody>
        </table>
      </Card>
      <div class="insp-foot"><span class="muted">Context: 36%</span><span class="grow" /><button class="btn small" onClick={() => setDialog("clear-session")}>Clear session…</button></div>
    </div>
  );
}

const KIND_LABEL: Record<Checkout["kind"], string> = { main: "Main checkout", linked: "Linked worktree", bare: "Bare repository", folder: "Folder" };
const KIND_ICON: Record<Checkout["kind"], string> = { main: "folder-git-2", linked: "git-branch", bare: "box", folder: "folder" };

function Worktrees() {
  const s = () => selected();
  const catalog = createMemo(() => (s() ? catalogFor(s()!.machine, s()!.cwd) : undefined));
  const [picked, setPicked] = createSignal<string>();
  const [reading, setReading] = createSignal(false);
  // The selected root: the user's pick, else the checkout this session uses.
  const current = () => picked() ?? catalog()?.checkouts.find((c) => c.sessions.some((x) => x.key === s()?.key))?.path ?? catalog()?.checkouts[0]?.path;
  const status = () => (reading() ? { text: "Reading folder…" } : catalog() ? statusLine(catalog()!, s()!.machine, false) : { text: "Select a folder" });
  const select = (c: Checkout) => { setPicked(c.path); toast("Folder filter", `The list shows the sessions in ${c.path}`); };
  const menu = (e: MouseEvent, c: Checkout) => {
    e.preventDefault();
    setPicked(c.path);
    setMenu({
      x: e.clientX, y: e.clientY, items: [
        ...(c.kind === "linked" ? [{
          label: c.unavailable ? "Forget missing worktree…" : "Remove worktree…", icon: "trash", danger: true,
          action: () => { setCleanupFocus(c.path); openSetup("worktree-cleanup"); },
        }, { separator: true as const }] : []),
        { label: "Copy path", icon: "copy", action: () => toast("Copied", c.path) },
      ],
    });
  };
  return (
    <div class="insp-body">
      <div class="insp-h">
        <b>Folder and worktrees</b><span class="grow" />
        <button class="icon-btn small" data-tip="Refresh" data-tip-pos="left" aria-label="Refresh" onClick={() => { setReading(true); setTimeout(() => setReading(false), 700); }}><Icon name="refresh-cw" size={13} /></button>
      </div>
      <div class="hint" classList={{ warn: status().tone === "warn", danger: status().tone === "danger" }}>{status().text}</div>
      <For each={catalog()?.checkouts ?? []}>{(c) => (
        <Card id={`wt-${c.path}`} class="wt" classList={{ on: current() === c.path, gone: !!c.unavailable }} onContextMenu={(e) => menu(e, c)}
          title={<><b>{KIND_LABEL[c.kind]}</b><Show when={c.branch}><span class="wt-branch"> / {c.branch}</span></Show><Show when={c.detached}><span class="wt-branch"> / detached {c.detached}</span></Show></>}
          aside={<>
            <Show when={c.locked}><span class="dim">(locked)</span></Show>
            <Show when={c.prunable}><span class="warn">(prunable)</span></Show>
            <Show when={c.unavailable}><span class="danger">(unavailable)</span></Show>
            <Show when={c.sessions.length}><span class="dim">({c.sessions.length} {c.sessions.length === 1 ? "session" : "sessions"})</span></Show>
          </>}>
          <button class="irow wt-path" title="Show the sessions in this folder" onClick={() => select(c)}>
            <span class="irow-ico"><Icon name={KIND_ICON[c.kind]} size={16} stroke={1.75} /></span>
            <span class="irow-val ell mono">{c.path}</span>
            <span class="irow-label">folder</span>
          </button>
          <For each={c.sessions}>{(x) => (
            <Row icon="corner-down-right" label={x.state}>
              <button class="link" disabled={!x.key} onClick={() => { if (x.key) { setSelectedKey(x.key); setDetailTab("activity"); } }}>{x.name}</button>
            </Row>
          )}</For>
        </Card>
      )}</For>
      <div class="insp-foot">
        <button class="btn small" onClick={() => { setPicked(undefined); toast("All sessions", "The folder filter was cleared"); }}>All sessions</button>
        <button class="btn small" disabled={catalog()?.state !== "ok"} data-tip="Review this repository's worktrees on its computer before anything is removed"
          onClick={() => { setCleanupFocus(undefined); openSetup("worktree-cleanup"); }}>{catalog()?.cleanable ? `Clean up (${catalog()!.cleanable})…` : "Clean up…"}</button>
        <span class="grow" />
        <button class="btn small" disabled={!current()} onClick={() => { setLaunchPreset({ project: s()?.project, machine: s()?.machine, path: current() }); openSetup("new-session"); }}><Icon name="plus" size={12} />New session…</button>
      </div>
    </div>
  );
}

/** Splitter handle on the panel's left edge: drag to resize, double-click to
 *  restore 400 px. Activity keeps at least 360 px beside it. */
function WidthHandle() {
  const [dragging, setDragging] = createSignal(false);
  const down = (e: PointerEvent) => {
    if (e.button !== 0) return;
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const work = el.closest(".work")?.getBoundingClientRect().width ?? innerWidth;
    const max = Math.max(INSPECTOR_WIDTH.min, Math.min(INSPECTOR_WIDTH.max, work - 16 - 360));
    const startX = e.clientX, start = Math.min(inspectorWidth(), max);
    setDragging(true);
    const move = (m: PointerEvent) => setInspectorWidth(Math.round(Math.max(INSPECTOR_WIDTH.min, Math.min(max, start + startX - m.clientX))));
    const up = () => { setDragging(false); el.removeEventListener("pointermove", move); el.removeEventListener("pointerup", up); el.removeEventListener("pointercancel", up); };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
    el.addEventListener("pointercancel", up);
  };
  const key = (e: KeyboardEvent) => {
    if (e.key === "ArrowLeft") setInspectorWidth(Math.min(INSPECTOR_WIDTH.max, inspectorWidth() + 20));
    else if (e.key === "ArrowRight") setInspectorWidth(Math.max(INSPECTOR_WIDTH.min, inspectorWidth() - 20));
    else if (e.key === "Home") setInspectorWidth(INSPECTOR_WIDTH.initial);
    else return;
    e.preventDefault();
  };
  return (
    <div class="insp-resize" classList={{ dragging: dragging() }} tabindex="0" role="separator" aria-orientation="vertical" aria-label="Side panel width"
      aria-valuemin={INSPECTOR_WIDTH.min} aria-valuemax={INSPECTOR_WIDTH.max} aria-valuenow={inspectorWidth()}
      data-tip="Drag to resize the side panel. Double-click to restore the default width." data-tip-pos="left"
      onPointerDown={down} onDblClick={() => setInspectorWidth(INSPECTOR_WIDTH.initial)} onKeyDown={key}><i /></div>
  );
}

export function Inspector(props: { leaving?: boolean; onLeft?: () => void }) {
  return (
    <aside class="inspector" classList={{ leaving: props.leaving }} style={{ width: `${inspectorWidth()}px` }} aria-label="Side panel" aria-hidden={props.leaving}
      onAnimationEnd={(e) => { if (e.target === e.currentTarget && props.leaving) props.onLeft?.(); }}>
      <WidthHandle />
      <div class="insp-tabs tabs" role="tablist">
        <For each={TABS}>{(t) => (
          <button class="tab" role="tab" classList={{ on: inspectorTab() === t.id }} aria-selected={inspectorTab() === t.id} onClick={() => setInspectorTab(t.id)}>{t.label}{t.id === "tasks" ? <TaskTabBadge /> : null}</button>
        )}</For>
        <span class="grow" />
        <button class="icon-btn small" data-tip="Collapse side panel (Ctrl+I)" data-tip-pos="left" aria-label="Collapse side panel" onClick={() => setInspectorOpen(false)}><Icon name="panel-right-close" size={14} /></button>
      </div>
      <div class="insp-scroll" classList={{ tasks: inspectorTab() === "tasks" }}>
        <Switch>
          <Match when={inspectorTab() === "plan"}><PlanAgents /></Match>
          <Match when={inspectorTab() === "details"}><Details /></Match>
          <Match when={inspectorTab() === "worktrees"}><Worktrees /></Match>
          <Match when={inspectorTab() === "tasks"}><TasksPanel /></Match>
        </Switch>
      </div>
    </aside>
  );
}
