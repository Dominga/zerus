// Task popovers: the card/task action menu (blocked entries keep their reason
// visible), the executor assignment menu with its searchable, paged
// "Specific session" list, and the 5-second reason tip a blocked action shows.
import { For, Show, createEffect, createMemo, createSignal, onCleanup, type JSX } from "solid-js";
import { Icon } from "../../lib/icons";
import { context, findSession, projectSessions, retarget, sessionLabel, taskById, type TaskSession } from "./board";

export type TaskMenuItem =
  | { label: string; icon?: string; reason?: string; danger?: boolean; checked?: boolean; action?: () => void }
  | { separator: true }
  | { heading: string };

export const [taskMenu, setTaskMenu] = createSignal<{ x: number; y: number; items: TaskMenuItem[]; label: string } | undefined>();
export const [assignMenu, setAssignMenu] = createSignal<{ x: number; y: number; task: string } | undefined>();
export const [tip, setTip] = createSignal<{ x: number; y: number; text: string } | undefined>();

export function openTaskMenu(anchor: { x: number; y: number }, label: string, items: TaskMenuItem[]) {
  setAssignMenu(undefined);
  setTaskMenu({ ...anchor, label, items });
}
export const below = (el: Element) => { const r = el.getBoundingClientRect(); return { x: r.left, y: r.bottom + 4 }; };

let tipTimer: number | undefined;
/** A blocked action says why beside it, as a blocked message send does (5 s). */
export function showReason(el: Element, text: string) {
  const r = el.getBoundingClientRect();
  clearTimeout(tipTimer);
  setTip({ x: r.left + r.width / 2, y: r.bottom + 6, text });
  tipTimer = window.setTimeout(() => setTip(undefined), 5000);
}

/** Shared popover behaviour: clamp to the viewport, close on outside press or Escape, arrow keys move focus. */
function Popover(props: { x: number; y: number; onClose: () => void; label: string; class?: string; children: JSX.Element }) {
  let el!: HTMLDivElement;
  createEffect(() => {
    const r = el.getBoundingClientRect();
    el.style.left = `${Math.max(8, Math.min(props.x, innerWidth - r.width - 8))}px`;
    el.style.top = `${Math.max(8, Math.min(props.y, innerHeight - r.height - 8))}px`;
    (el.querySelector(".menu-item:not([aria-disabled=true]), input") as HTMLElement | null)?.focus();
    const close = (e: Event) => {
      if (e instanceof KeyboardEvent && e.key !== "Escape") return;
      if (e instanceof MouseEvent && el.contains(e.target as Node)) return;
      props.onClose();
    };
    const t = setTimeout(() => { addEventListener("mousedown", close); addEventListener("keydown", close); addEventListener("blur", close); });
    onCleanup(() => { clearTimeout(t); removeEventListener("mousedown", close); removeEventListener("keydown", close); removeEventListener("blur", close); });
  });
  const onKey = (e: KeyboardEvent) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const items = [...el.querySelectorAll<HTMLElement>(".menu-item:not([aria-disabled=true])")];
    const i = items.indexOf(document.activeElement as HTMLElement);
    items[(i + (e.key === "ArrowDown" ? 1 : -1) + items.length) % items.length]?.focus();
  };
  return (
    <div class={`menu tk-pop ${props.class ?? ""}`} role="menu" aria-label={props.label} ref={el} style={{ left: `${props.x}px`, top: `${props.y}px` }} onKeyDown={onKey}>
      {props.children}
    </div>
  );
}

function MenuItems(props: { items: TaskMenuItem[]; close: () => void }) {
  return (
    <For each={props.items}>{(item) =>
      "separator" in item ? <div class="menu-sep" role="separator" /> :
      "heading" in item ? <div class="menu-label">{item.heading}</div> : (
        <button class="menu-item tk-mi" classList={{ danger: item.danger, blocked: !!item.reason }} role={item.checked !== undefined ? "menuitemcheckbox" : "menuitem"}
          aria-checked={item.checked} aria-disabled={!!item.reason} title={item.reason}
          onClick={() => { if (item.reason) return; props.close(); item.action?.(); }}>
          <Show when={item.checked !== undefined} fallback={<Show when={item.icon} fallback={<span class="tk-mi-pad" />}><Icon name={item.icon!} size={14} /></Show>}>
            <Icon name="check" size={14} style={{ opacity: item.checked ? 1 : 0 }} />
          </Show>
          <span class="tk-mi-text">
            <span>{item.label}</span>
            <Show when={item.reason}><small>{item.reason}</small></Show>
          </span>
        </button>
      )
    }</For>
  );
}

/** Executor plan choices (TaskInspector::showAssignment). Choosing stores intent; it starts nothing. */
function AssignPopover(props: { x: number; y: number; taskId: string }) {
  const [sub, setSub] = createSignal(false);
  const [query, setQuery] = createSignal("");
  const [page, setPage] = createSignal(0);
  const c = () => context();
  const close = () => setAssignMenu(undefined);
  const assign = (target: Parameters<typeof retarget>[1]) => { retarget(props.taskId, target); close(); };
  const PAGE = 50;
  const matches = createMemo(() => {
    const q = query().toLowerCase();
    return projectSessions().filter((s) => s.name.toLowerCase().includes(q) || s.host.toLowerCase().includes(q));
  });
  const pageCount = () => Math.max(1, Math.ceil(matches().length / PAGE));
  const shown = () => matches().slice(Math.min(page(), pageCount() - 1) * PAGE, Math.min(page(), pageCount() - 1) * PAGE + PAGE);
  const current = () => taskById(props.taskId)?.target;
  const isCurrent = (s: TaskSession) => current()?.kind === "session" && current()?.session === s.name && (current()?.host ?? "") === s.host;
  return (
    <Popover x={props.x} y={props.y} onClose={close} label="Task executor" class="tk-assign">
      <div class="tk-assign-cols">
        <div class="tk-assign-main">
          <MenuItems close={close} items={[
            { label: `Main session: ${sessionLabel(c().host, c().session)}`, icon: "monitor", reason: c().session ? undefined : "Open the session to send this task to", action: () => assign({ kind: "session", host: c().host, session: c().session }) },
            { heading: "New executor, created when the task starts" },
            { label: "Subagent", icon: "bot", action: () => assign({ kind: "subagent", host: c().host, session: c().session }) },
            { label: "New session", icon: "plus", action: () => assign({ kind: "new_session", host: c().host, session: c().session }) },
            { separator: true },
            { label: "Any session (the one selected at start)", icon: "users", action: () => assign({ kind: "any" }) },
          ]} />
          <button class="menu-item tk-mi" classList={{ open: sub() }} role="menuitem" aria-haspopup="true" aria-expanded={sub()} onClick={() => setSub(!sub())}
            onKeyDown={(e) => { if (e.key === "ArrowRight") { e.preventDefault(); setSub(true); } }}>
            <Icon name="list-filter" size={14} /><span class="tk-mi-text"><span>Specific session</span></span><Icon name="chevron-right" size={13} class="tk-mi-end" />
          </button>
        </div>
        <Show when={sub()}>
          <div class="tk-assign-sub" role="group" aria-label="Specific session">
            <input class="field tk-assign-search" placeholder="Find a session…" value={query()} onInput={(e) => { setQuery(e.currentTarget.value); setPage(0); }}
              onKeyDown={(e) => { if (e.key === "ArrowLeft" && !query()) { e.preventDefault(); setSub(false); } }} aria-label="Find a session" />
            <div class="tk-assign-list">
              <For each={shown()} fallback={<div class="hint tk-assign-none">No session matches.</div>}>{(s) => (
                <button class="menu-item tk-mi" role="menuitemradio" aria-checked={isCurrent(s)} onClick={() => assign({ kind: "session", host: s.host, session: s.name })}>
                  <span class={`tk-dot k-${s.kind}`} />
                  <span class="tk-mi-text"><span class="ell">{sessionLabel(s.host, s.name)}</span></span>
                  <Show when={isCurrent(s)}><Icon name="check" size={13} class="tk-mi-end accent" /></Show>
                </button>
              )}</For>
            </div>
            <div class="tk-assign-nav">
              <button class="btn small" disabled={page() === 0} onClick={() => setPage(page() - 1)}>Previous</button>
              <span class="dim tabular">{Math.min(page(), pageCount() - 1) + 1} / {pageCount()}</span>
              <button class="btn small" disabled={page() >= pageCount() - 1} onClick={() => setPage(page() + 1)}>Next</button>
            </div>
          </div>
        </Show>
      </div>
    </Popover>
  );
}

export function TaskOverlays() {
  return (
    <>
      <Show when={taskMenu()}>{(m) => (
        <Popover x={m().x} y={m().y} onClose={() => setTaskMenu(undefined)} label={m().label}>
          <MenuItems items={m().items} close={() => setTaskMenu(undefined)} />
        </Popover>
      )}</Show>
      <Show when={assignMenu()}>{(m) => <AssignPopover x={m().x} y={m().y} taskId={m().task} />}</Show>
      <Show when={tip()}>{(t) => (
        <div class="tk-tip" role="status" style={{ left: `${t().x}px`, top: `${t().y}px` }}
          ref={(el) => queueMicrotask(() => {
            // Keep the tip inside the window: centred under the action unless that would clip it.
            const w = el.offsetWidth;
            el.style.left = `${Math.max(8 + w / 2, Math.min(t().x, innerWidth - 8 - w / 2))}px`;
          })}>{t().text}</div>
      )}</Show>
    </>
  );
}


