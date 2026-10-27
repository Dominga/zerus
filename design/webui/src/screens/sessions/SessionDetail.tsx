import { For, Show, Switch, Match, onMount, onCleanup, createEffect } from "solid-js";
import { Icon } from "../../lib/icons";
import { ActivityView } from "./Activity";
import { Composer } from "./Composer";
import { Inspector } from "./Inspector";
import { AgentBadge, MachineBadge } from "./SessionCard";
import { TaskView } from "../tasks/TaskView";
import { selected, detailTab, setDetailTab, inspectorOpen, setInspectorOpen, setMenu, setDialog, toast, subagentOpen, STATUS_LABEL, type DetailTab } from "../../state";
import { projects, terminalLines, processes } from "../../mock/data";
import { skin } from "../../skin/runtime";

function headerCaption(): string {
  const s = selected();
  if (!s) return "";
  if (s.status === "working") return s.phase === "tool" ? "Using a tool" : s.phase === "compacting" ? "Compacting" : "Working";
  if (s.status === "attention") return s.attention === "approval" ? "Needs approval" : "Needs input";
  if (s.status === "scheduled") return `Scheduled · ${s.scheduleNext}`;
  if (s.status === "error") return "Rate limit reached";
  return STATUS_LABEL[s.status];
}

function TerminalTab() {
  let host!: HTMLDivElement;
  // xterm.js loads only when the Terminal tab first opens.
  onMount(async () => {
    const [{ Terminal }, { FitAddon }] = await Promise.all([import("@xterm/xterm"), import("@xterm/addon-fit"), import("@xterm/xterm/css/xterm.css")]);
    const css = getComputedStyle(document.documentElement);
    const term = new Terminal({
      fontFamily: css.getPropertyValue("--z-font-mono"),
      fontSize: 13,
      cursorBlink: true,
      scrollback: 5000,
      theme: { background: css.getPropertyValue("--z-color-terminal-bg").trim(), foreground: css.getPropertyValue("--z-color-terminal-fg").trim(), cursor: css.getPropertyValue("--z-color-accent").trim() },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host);
    fit.fit();
    terminalLines.forEach((l) => term.writeln(l));
    term.write("\x1b[33m⠋\x1b[0m ");
    const ro = new ResizeObserver(() => fit.fit());
    ro.observe(host);
    createEffect(() => {
      skin();
      queueMicrotask(() => {
        const c = getComputedStyle(document.documentElement);
        term.options.theme = { background: c.getPropertyValue("--z-color-terminal-bg").trim(), foreground: c.getPropertyValue("--z-color-terminal-fg").trim(), cursor: c.getPropertyValue("--z-color-accent").trim() };
      });
    });
    onCleanup(() => { ro.disconnect(); term.dispose(); });
  });
  return (
    <div class="term-tab">
      <div class="term-bar">
        <span class="state-dot working" /><span>Connected: {selected()?.name} on {selected()?.machine}</span>
        <span class="grow" />
        <button class="icon-btn" title="Reconnect"><Icon name="refresh-cw" size={15} /></button>
        <button class="icon-btn" title="Disconnect: the session keeps running"><Icon name="plug" size={15} /></button>
      </div>
      <div class="term-host" ref={host} />
    </div>
  );
}

function ProcessesTab() {
  return (
    <div class="proc-tab">
      <div class="proc-summary"><b>3 live</b> · 1 active · 0 unconfirmed · 12 recorded<span class="grow" /><button class="btn small danger" title="TERM, then KILL after 2 s">Stop selected (1)</button></div>
      <table class="proc-table">
        <thead><tr><th>Command</th><th>Agent</th><th>State</th><th>Duration</th></tr></thead>
        <tbody>
          <tr class="grp"><td colspan="4">Live shells / processes</td></tr>
          <For each={processes}>{(p, i) => (
            <tr classList={{ sel: i() === 0 }}>
              <td class="mono ell">{p.command}</td><td>{p.kind}</td>
              <td><span class={`state-dot ${p.state === "running" ? "working" : "ready"}`} />Running</td><td class="tabular">{["2m 10s", "1m 52s", "1h 31m", "1h 41m"][i()]}</td>
            </tr>
          )}</For>
          <tr class="grp"><td colspan="4">▸ Finished (12)</td></tr>
        </tbody>
      </table>
      <div class="proc-detail">
        <div class="row"><b class="mono">cargo test --workspace</b><span class="grow" /><button class="btn small">Copy command</button></div>
        <div class="hint">workstation · /home/example/src/aurora · running · started 11:43</div>
        <pre class="act-code mono">running 42 tests{"\n"}test pagination::cursor_round_trip ... ok{"\n"}test routes::orders::v1_shape_unchanged ... ok{"\n"}…</pre>
        <div class="hint">Showing the retained output tail.</div>
      </div>
    </div>
  );
}

function NativeTab() {
  return (
    <div class="native-tab">
      <div class="term-bar"><b>DeepSeek workspace / {selected()?.machine}</b><span class="grow" /><button class="btn small">Respond in Activity</button><button class="icon-btn" title="Reload"><Icon name="refresh-cw" size={15} /></button></div>
      <div class="native-frame">
        <div class="empty"><Icon name="monitor-smartphone" size={30} /><h3>Agent's own web UI</h3><p>Shown in an isolated webview (Tauri child webview), or in the system browser when embedding is unavailable.</p></div>
      </div>
    </div>
  );
}

function EmptyDetail() {
  return (
    <div class="detail-empty">
      <h2>A little more focus.</h2>
      <p>Select a session to see what your agent is doing.<br />New sessions appear here automatically.</p>
    </div>
  );
}

function Header() {
  const s = () => selected()!;
  const project = () => projects.find((p) => p.id === s().project);
  const tone = () => (s().status === "attention" ? "attention" : s().status);
  const actions = () => [
    { label: "Open folder", icon: "folder-open" },
    { label: "Open shell in session folder", icon: "square-terminal" },
    { label: "Copy session ID", icon: "copy" },
    { separator: true as const },
    { label: "Rename…", icon: "pencil", kbd: "F2", action: () => setDialog("rename") },
    { label: "Fork session…", icon: "git-fork", action: () => setDialog("fork") },
    { separator: true as const },
    { label: "Copy connect command", icon: "link" },
    { label: s().status === "ended" ? "Restore session" : "Pause session", icon: s().status === "ended" ? "play" : "pause" },
    { separator: true as const },
    { label: "Clear session…", icon: "rotate-ccw", action: () => setDialog("clear-session") },
    { label: "Move to archive", icon: "archive", disabled: s().status !== "ready" },
    { label: "Terminate session…", icon: "circle-stop", danger: true, action: () => setDialog("terminate") },
  ];
  return (
    <header class="detail-head">
      <div class="dh-row1">
        <h1 class="ell" title={`${s().agent}/${s().project}/${s().name}`}>{project()?.name} / {s().name}<Show when={subagentOpen()}> / {s().subagents?.find((k) => k.id === subagentOpen())?.name}</Show></h1>
        <Show when={s().machine !== "workstation" && s().status === "working"}><span class="busy-arc" title="Waiting for the remote read" /></Show>
        <span class="grow" />
        <span class={`hbadge t-${tone()}`}>{headerCaption()}</span>
        <button class="icon-btn" data-tip="Open session folder in file manager"><Icon name="folder-open" size={16} /></button>
        <button class="icon-btn" data-tip="Open shell in session folder"><Icon name="square-terminal" size={16} /></button>
        <button class="icon-btn" data-tip="Copy session ID" onClick={() => toast("Copied", "Session ID copied")}><Icon name="copy" size={15} /></button>
        <button class="icon-btn" data-tip="Clear session" onClick={() => setDialog("clear-session")}><Icon name="rotate-ccw" size={15} /></button>
        <Show when={s().status === "ended"}><button class="btn small primary"><Icon name="play" size={12} />Restore</button></Show>
        <button class="icon-btn" data-tip="Session actions" onClick={(e) => setMenu({ x: e.clientX - 200, y: e.clientY + 12, items: actions() })}><Icon name="ellipsis" size={16} /></button>
      </div>
      <div class="dh-row2">
        <AgentBadge agent={s().agent} />
        <MachineBadge alias={s().machine} />
        <span class="dh-meta ell" title={s().cwd}>{s().cwd.split(/[\\/]/).pop()}{s().branch ? ` / ${s().branch}` : ""}{s().worktree ? ` · worktree ${s().worktree}` : ""}</span>
        <span class="grow" />
        <span class="dh-model ell">{s().model} {s().effort}</span>
      </div>
      <Show when={s().goal}>
        <button class="goal-btn" onClick={() => toast("Plan & agents", "Opens the goal details in the side panel")}><b>Pursuing goal</b><span class="dim">1 h 12 min</span><span class="grow" /><span class="accent">Details →</span></button>
      </Show>
    </header>
  );
}

const TAB_LIST = (): { id: DetailTab; label: string; show: boolean }[] => {
  const s = selected();
  return [
    { id: "activity", label: "Activity", show: true },
    { id: "terminal", label: "Terminal", show: !!s && s.status !== "ended" && s.status !== "offline" },
    { id: "native", label: "Native UI", show: s?.agent === "deepseek" },
    { id: "processes", label: `Processes (${s?.status === "working" || s?.status === "background" ? 3 : 0})`, show: true },
    { id: "taskview", label: "Task View", show: detailTab() === "taskview" },
  ];
};

export function SessionDetail() {
  return (
    <section class="detail" aria-label="Session">
      <Show when={selected()} fallback={<EmptyDetail />}>
        <Header />
        <div class="work" classList={{ "with-inspector": inspectorOpen() }}>
          <div class="work-main">
            <div class="detail-tabs tabs" role="tablist">
              <For each={TAB_LIST().filter((t) => t.show)}>{(t) => (
                <button class="tab" role="tab" classList={{ on: detailTab() === t.id }} aria-selected={detailTab() === t.id} onClick={() => setDetailTab(t.id)}>{t.label}</button>
              )}</For>
              <span class="grow" />
              <Show when={!inspectorOpen()}>
                <button class="icon-btn small" data-tip="Show tasks, plan, agents, details and worktrees (Ctrl+I)" data-tip-pos="left" onClick={() => setInspectorOpen(true)}><Icon name="panel-right" size={15} /></button>
              </Show>
            </div>
            <div class="tab-body">
              <Switch>
                <Match when={detailTab() === "activity"}>
                  <div class="activity-wrap">
                    <ActivityView />
                    <div class="cmp-col" classList={{ full: skin().layout["activity-column"] === "full" }}><Composer /></div>
                  </div>
                </Match>
                <Match when={detailTab() === "terminal"}><TerminalTab /></Match>
                <Match when={detailTab() === "processes"}><ProcessesTab /></Match>
                <Match when={detailTab() === "native"}><NativeTab /></Match>
                <Match when={detailTab() === "taskview"}><TaskView /></Match>
              </Switch>
            </div>
          </div>
          <Show when={inspectorOpen()}><Inspector /></Show>
        </div>
      </Show>
    </section>
  );
}
