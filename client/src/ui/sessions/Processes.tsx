import { For, Show, createEffect, createSignal, on, onCleanup } from "solid-js";
import { Spinner } from "../components/Status";
import { copy } from "../../store/actions";
import { processes } from "../../store/processes";
import { setMenu } from "../../store/ui";
import type { ProcessesView } from "../../types";

// The Processes tab (tray/src/ProcessView.cpp:36-79) over the Rust hub's view
// (src-tauri processes.rs): the summary with the busy indicator and Stop
// selected, the source and stop notices, the tree of Command, Agent, State
// and Duration, and the current process's details and output.

/** Qt's top-level items start expanded except Finished; a toggle lasts for the window's life. */
const [collapsed, setCollapsed] = createSignal<Record<string, boolean>>({ finished: true });

/** Within this distance of the end, the output stays at its end when it grows. */
const OUTPUT_END = 8;

/**
 * The row menu (ProcessView.cpp:63-75): Terminate, or Terminate selected (N),
 * enabled and titled as Stop selected. It acts only while the view and its
 * selection are still the ones it opened on.
 */
function rowMenu(v: ProcessesView, at: { x: number; y: number }) {
  const key = v.key, ids = [...v.selected];
  setMenu({ x: at.x, y: at.y, items: [{
    label: ids.length === 1 ? "Terminate" : `Terminate selected (${ids.length})`,
    disabled: !v.stop.enabled, hint: v.stop.tooltip,
    action: () => {
      const now = processes.view();
      if (now?.key === key && now.selected.length === ids.length && now.selected.every((id, i) => id === ids[i])) processes.stop();
    },
  }] });
}

export function ProcessesTab() {
  const view = () => processes.view();
  let list!: HTMLDivElement;
  let output: HTMLPreElement | undefined;
  // A right click on an unselected row selects it first; the menu opens once the hub shows that selection.
  let pending: { id: string; at: { x: number; y: number }; until: number } | undefined;
  createEffect(() => {
    const v = view();
    if (!pending || !v) return;
    if (Date.now() > pending.until) { pending = undefined; return; }
    if (v.selected.length === 1 && v.selected[0] === pending.id) { const at = pending.at; pending = undefined; rowMenu(v, at); }
  });
  onCleanup(() => { pending = undefined; });
  // Another session's view starts at the top.
  createEffect(on(() => view()?.key, () => { if (list) list.scrollTop = 0; }, { defer: true }));
  // The output keeps its end in view when it was there before it changed.
  let atEnd = true;
  createEffect(on(() => view()?.detail?.output, () => {
    if (!output) return;
    if (atEnd) output.scrollTop = output.scrollHeight;
  }));

  function contextMenu(e: MouseEvent, id: string) {
    e.preventDefault();
    const v = view();
    if (!v) return;
    const at = { x: e.clientX, y: e.clientY };
    if (v.selected.includes(id)) { rowMenu(v, at); return; }
    pending = { id, at, until: Date.now() + 1000 };
    processes.contextOn(id);
  }

  return (
    <div class="proc-tab">
      <Show when={view()} fallback={<div class="proc-summary">Reading processes…</div>}>
        {(v) => (
          <>
            <div class="proc-summary">
              <span class="proc-summary-text" title={v().summary_tooltip}>{v().summary}</span>
              <Show when={v().busy}><Spinner size={12} /></Show>
              <span class="grow" />
              <button class="btn small danger" disabled={!v().stop.enabled} title={v().stop.tooltip} onClick={() => processes.stop()}>{v().stop.label}</button>
            </div>
            <Show when={v().notes}><div class="proc-note hint">{v().notes}</div></Show>
            <Show when={v().stop_notice}><div class="proc-note hint">{v().stop_notice}</div></Show>
            <Show when={v().failed}>{(text) => <div class="proc-note warn">{text()}</div>}</Show>
            <div class="proc-list" ref={list}>
              <table class="proc-table">
                <thead><tr><th>Command</th><th>Agent</th><th>State</th><th>Duration</th></tr></thead>
                <tbody>
                  <For each={v().groups}>{(g) => (
                    <>
                      <tr class="grp" onClick={() => setCollapsed((c) => ({ ...c, [g.group]: !c[g.group] }))}>
                        <td colspan="4"><span class="proc-chevron">{collapsed()[g.group] ? "▸" : "▾"}</span>{g.title}</td>
                      </tr>
                      <Show when={!collapsed()[g.group]}>
                        <For each={g.rows}>{(r) => (
                          <tr classList={{ sel: v().selected.includes(r.id), current: v().current === r.id }}
                            onClick={(e) => processes.click(r.id, { toggle: e.ctrlKey || e.metaKey, range: e.shiftKey })}
                            onContextMenu={(e) => contextMenu(e, r.id)}>
                            <td class="mono" title={r.command_full}>{r.command}</td>
                            <td>{r.owner}</td>
                            <td class={`proc-state ${r.tone}`} title={r.tooltip}>{r.status}</td>
                            <td class="tabular">{r.duration}</td>
                          </tr>
                        )}</For>
                      </Show>
                    </>
                  )}</For>
                </tbody>
              </table>
            </div>
            <div class="proc-detail">
              <Show when={v().detail} fallback={<div class="hint">Select a command to view its output.</div>}>
                {(d) => (
                  <>
                    <b class="mono proc-command">{d().command}</b>
                    <Show when={d().details}><div class="hint proc-details">{d().details}</div></Show>
                    <div class="row">
                      <span class="hint grow">{d().notice}</span>
                      <button class="btn small" onClick={() => void copy(d().command)}>Copy command</button>
                    </div>
                    <pre class="act-code mono proc-output" ref={output}
                      onScroll={(e) => { const el = e.currentTarget; atEnd = el.scrollHeight - el.scrollTop - el.clientHeight < OUTPUT_END; }}>{d().output ?? ""}</pre>
                  </>
                )}
              </Show>
            </div>
          </>
        )}
      </Show>
    </div>
  );
}
