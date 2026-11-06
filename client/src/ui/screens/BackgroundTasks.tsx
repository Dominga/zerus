import { Index, Show, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { Portal } from "solid-js/web";
import { backgroundTasks, type BackgroundTask } from "../../store/backgroundTasks";
import { cleanup, openCleanup } from "../../store/terminationCleanup";
import { Icon } from "../lib/icons";
import { tr } from "../../i18n";

/** The status bar item: it exists only while work is running. */
export function BackgroundTasksChip() {
  const tasks = createMemo(() => backgroundTasks(cleanup()));
  return <Show when={tasks().length}><Chip tasks={tasks} /></Show>;
}

function Chip(props: { tasks: () => BackgroundTask[] }) {
  const [open, setOpen] = createSignal(false);
  let button!: HTMLButtonElement;
  const close = (focus: boolean) => { setOpen(false); if (focus) button.focus(); };
  return <>
    <button ref={button} type="button" class="chip-accent bg-tasks-chip" aria-haspopup="dialog" aria-expanded={open()} onClick={() => setOpen(!open())}>
      <Icon name="loader-circle" size={11} class="su-spin" />{tr("Background tasks %1", props.tasks().length)}
    </button>
    <Show when={open()}><TaskList tasks={props.tasks} anchor={button} close={close} /></Show>
  </>;
}

/** Opens above the chip. Its only action is the review the chip's work already has. */
function TaskList(props: { tasks: () => BackgroundTask[]; anchor: HTMLElement; close: (focus: boolean) => void }) {
  let el!: HTMLDivElement;
  onMount(() => {
    const anchor = props.anchor.getBoundingClientRect();
    el.style.left = `${Math.max(8, Math.min(anchor.left, innerWidth - el.offsetWidth - 8))}px`;
    el.style.bottom = `${innerHeight - anchor.top + 4}px`;
    el.querySelector<HTMLElement>("button")?.focus({ preventScroll: true });
    const outside = (e: PointerEvent) => { if (!el.contains(e.target as Node) && !props.anchor.contains(e.target as Node)) props.close(false); };
    const escape = (e: KeyboardEvent) => { if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); props.close(true); } };
    addEventListener("pointerdown", outside);
    addEventListener("keydown", escape, true);
    onCleanup(() => { removeEventListener("pointerdown", outside); removeEventListener("keydown", escape, true); });
  });
  return <Portal><div ref={el} class="menu bg-tasks" role="dialog" aria-label={tr("Background tasks")} data-shortcut-context="none">
    {/* Position-based slots keep the buttons and focus across store updates. */}
    <Index each={props.tasks()}>{(task) => <div class="bg-task">
      <span class="ell" title={task().path}>{task().name}</span>
      <span class="muted">{task().state}</span>
      <button type="button" class="btn small ghost" onClick={() => { const { id } = task(); props.close(true); openCleanup(id); }}>{tr("Review…")}</button>
    </div>}</Index>
  </div></Portal>;
}
