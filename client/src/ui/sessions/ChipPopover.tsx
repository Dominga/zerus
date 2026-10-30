import { createEffect, createSignal, onCleanup, onMount, Show, type JSX } from "solid-js";

/** The fixed popover above a composer chip (ChipPopover). */
export function ChipPopover(props: {
  open: boolean;
  anchor: HTMLElement;
  label: string;
  width?: number;
  class?: string;
  keyboard?: boolean;
  onClose: (returnFocus: boolean) => void;
  children: JSX.Element;
}) {
  const [pos, setPos] = createSignal({ left: 8, bottom: 8 });
  let pop: HTMLDivElement | undefined;
  const close = (returnFocus: boolean) => props.onClose(returnFocus);
  const outside = (event: PointerEvent) => {
    if (props.open && !pop?.contains(event.target as Node) && !props.anchor.contains(event.target as Node)) close(false);
  };
  onMount(() => addEventListener("pointerdown", outside, true));
  onCleanup(() => removeEventListener("pointerdown", outside, true));
  createEffect(() => {
    if (!props.open) return;
    const width = props.width ?? 360;
    const rect = props.anchor.getBoundingClientRect();
    const rightHalf = rect.left + rect.width / 2 >= innerWidth / 2;
    const left = rightHalf ? rect.right - width : rect.left;
    setPos({ left: Math.max(8, Math.min(left, innerWidth - width - 8)), bottom: Math.max(8, innerHeight - rect.top + 6) });
    if (props.keyboard) queueMicrotask(() => (pop?.querySelector<HTMLElement>("button:not(:disabled)") ?? pop)?.focus({ preventScroll: true }));
  });
  return (
    <Show when={props.open}>
      <div ref={pop} class={`att-pop ${props.class ?? ""}`} role="dialog" aria-label={props.label} tabindex="-1" data-shortcut-context="none"
        style={{ left: `${pos().left}px`, bottom: `${pos().bottom}px`, width: `${props.width ?? 360}px`, "max-width": `${Math.max(240, innerWidth - 16)}px` }}
        onKeyDown={(event) => { if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); close(true); } }}>
        {props.children}
      </div>
    </Show>
  );
}
