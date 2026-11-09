import { For, Show, createEffect, createSignal, createUniqueId, onCleanup, onMount, splitProps, type JSX } from "solid-js";
import { Icon } from "./icons";

export interface DropdownOption {
  value: string | number;
  label: JSX.Element | (() => JSX.Element);
  /** Plain text for decorated labels, accessibility and type-ahead. */
  textValue?: string;
  disabled?: boolean;
}
type Props = Omit<JSX.ButtonHTMLAttributes<HTMLButtonElement>, "value" | "onChange" | "children" | "ref"> & {
  value: string | number | undefined | null;
  options: readonly DropdownOption[];
  onChange(value: string): void;
};
// One open dropdown, with no retained option or per-session cache.
let dismissCurrent: (() => void) | undefined;

/** A controlled select with the app's menu styling, including inside modal dialogs. */
export function Dropdown(props: Props) {
  const [own, attrs] = splitProps(props, ["value", "options", "onChange", "class", "disabled", "id"]);
  const uid = createUniqueId(), id = () => own.id || `dropdown-${uid}`;
  let button!: HTMLButtonElement, popup: HTMLDivElement | undefined;
  const [open, setOpen] = createSignal(false), [active, setActive] = createSignal<string>();
  const value = () => String(own.value ?? "");
  const chosen = () => own.options.find((o) => String(o.value) === value());
  const enabled = () => own.options.filter((o) => !o.disabled);
  const disabled = () => own.disabled || !enabled().length;
  const key = (o: DropdownOption) => String(o.value);
  const label = (o: DropdownOption | undefined) => typeof o?.label === "function" ? o.label() : o?.label;
  let typed = "", typedAt = 0;
  const close = (focus = true) => {
    setOpen(false);
    if (dismissCurrent === dismiss) dismissCurrent = undefined;
    if (focus && button.isConnected && !button.disabled) button.focus({ preventScroll: true });
  };
  const dismiss = () => close(false);
  const choose = (next: string | undefined, focus = true) => {
    const option = own.options.find((o) => key(o) === next && !o.disabled);
    close(focus);
    if (option && key(option) !== value()) own.onChange(key(option));
  };
  const show = () => {
    if (disabled()) return;
    dismissCurrent?.(); dismissCurrent = dismiss;
    button.focus({ preventScroll: true });
    setActive(chosen()?.disabled ? enabled()[0] && key(enabled()[0]) : chosen() ? value() : enabled()[0] && key(enabled()[0]));
    typed = ""; setOpen(true);
  };
  const move = (by: number, edge?: "first" | "last") => {
    const options = enabled();
    const at = options.findIndex((o) => key(o) === (open() ? active() : value()));
    const index = edge === "first" ? 0 : edge === "last" ? options.length - 1
      : Math.max(0, Math.min(options.length - 1, at < 0 ? by > 0 ? 0 : options.length - 1 : at + by));
    const next = options[index] && key(options[index]);
    if (open()) setActive(next); else if (next !== undefined && next !== value()) own.onChange(next);
  };
  const onKey = (e: KeyboardEvent) => {
    if (disabled() || e.ctrlKey || e.metaKey) return;
    if (e.key === "Escape") { if (open()) { e.preventDefault(); e.stopPropagation(); close(); } return; }
    if (e.key === "Tab") { if (open()) choose(active(), false); return; }
    if ((e.altKey && (e.key === "ArrowDown" || e.key === "ArrowUp")) || (e.key === "F4" && !e.altKey) || e.key === "Enter" || e.key === " ") {
      e.preventDefault(); e.stopPropagation(); if (open()) choose(active()); else show(); return;
    }
    if (e.altKey) return;
    if (["ArrowDown", "ArrowUp", "Home", "End", "PageDown", "PageUp"].includes(e.key)) {
      e.preventDefault(); e.stopPropagation();
      move(e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : e.key === "PageDown" ? 10 : -10,
        e.key === "Home" ? "first" : e.key === "End" ? "last" : undefined);
      return;
    }
    if (e.key.length !== 1) return;
    e.preventDefault(); e.stopPropagation();
    const now = Date.now(); typed = now - typedAt > 700 ? e.key : (typed + e.key).slice(0, 128); typedAt = now;
    const query = (/^(.)\1+$/u.test(typed) ? typed[0] : typed).toLocaleLowerCase();
    const options = enabled(), from = options.findIndex((o) => key(o) === (open() ? active() : value()));
    const start = query.length === 1 ? from + 1 : Math.max(0, from);
    for (let i = 0; i < options.length; i++) {
      const option = options[(start + i) % options.length];
      const text = option.textValue ?? (typeof option.label === "string" || typeof option.label === "number" ? String(option.label) : "");
      if (!text.trim().toLocaleLowerCase().startsWith(query)) continue;
      if (open()) setActive(key(option)); else if (key(option) !== value()) own.onChange(key(option));
      break;
    }
  };
  const place = () => {
    if (!popup) return;
    const box = button.getBoundingClientRect(), width = window.innerWidth, height = window.innerHeight;
    const below = Math.max(0, height - box.bottom - 12), above = Math.max(0, box.top - 12);
    popup.style.minWidth = `${Math.min(box.width, Math.max(0, width - 16))}px`;
    popup.style.maxWidth = `${Math.min(Math.max(box.width, 320), Math.max(0, width - 16))}px`;
    popup.style.maxHeight = `${Math.min(320, Math.max(below, above))}px`;
    const down = popup.offsetHeight <= below || below >= above;
    popup.style.left = `${Math.max(8, Math.min(box.left, width - popup.offsetWidth - 8))}px`;
    popup.style.top = `${Math.max(8, Math.min(down ? box.bottom + 4 : box.top - popup.offsetHeight - 4, height - popup.offsetHeight - 8))}px`;
    popup.style.visibility = "visible";
  };
  createEffect(() => {
    if (!open()) return;
    if (disabled()) { close(false); return; }
    if (!enabled().some((o) => key(o) === active())) setActive(enabled()[0] && key(enabled()[0]));
    const away = (e: Event) => { if (!button.contains(e.target as Node) && !popup?.contains(e.target as Node)) close(false); };
    const blur = () => close(false);
    let frame = 0;
    const reposition = (e: Event) => {
      if (e.target instanceof Node && popup?.contains(e.target)) return;
      cancelAnimationFrame(frame); frame = requestAnimationFrame(place);
    };
    document.addEventListener("pointerdown", away, true);
    window.addEventListener("blur", blur);
    window.addEventListener("resize", reposition);
    document.addEventListener("scroll", reposition, true);
    onCleanup(() => {
      cancelAnimationFrame(frame);
      document.removeEventListener("pointerdown", away, true);
      window.removeEventListener("blur", blur);
      window.removeEventListener("resize", reposition);
      document.removeEventListener("scroll", reposition, true);
    });
  });
  createEffect(() => {
    const index = own.options.findIndex((o) => key(o) === active());
    if (!open() || !popup || index < 0) return;
    const row = popup.querySelector<HTMLElement>(`[data-index="${index}"]`);
    if (!row) return;
    const box = popup.getBoundingClientRect(), item = row.getBoundingClientRect();
    if (item.top < box.top) popup.scrollTop -= box.top - item.top;
    else if (item.bottom > box.bottom) popup.scrollTop += item.bottom - box.bottom;
  });
  onCleanup(() => { if (dismissCurrent === dismiss) dismissCurrent = undefined; });
  const Popup = () => {
    let node!: HTMLDivElement, layer: HTMLDialogElement | undefined;
    const popover = typeof HTMLElement.prototype.showPopover === "function";
    onMount(() => {
      popup = node;
      // The top layer escapes dialog overflow while retaining modal ancestry.
      if (popover) node.showPopover();
      else { layer!.showModal(); node.focus({ preventScroll: true }); }
      place();
      const index = own.options.findIndex((o) => key(o) === active());
      const row = node.querySelector<HTMLElement>(`[data-index="${index}"]`);
      if (row) node.scrollTop = Math.max(0, row.offsetTop - node.clientHeight / 2);
    });
    onCleanup(() => { layer?.close(); popup = undefined; });
    const content = () => <div ref={node} id={`${id()}-listbox`} popover={popover ? "manual" : undefined} class="menu dropdown-list" role="listbox" tabindex="-1"
      aria-activedescendant={!popover && active() !== undefined ? `${id()}-option-${own.options.findIndex((o) => key(o) === active())}` : undefined}
      aria-label={attrs["aria-label"]} aria-labelledby={attrs["aria-labelledby"] || (attrs["aria-label"] ? undefined : id())}
      style={{ visibility: "hidden" }} data-shortcut-context="none" on:keydown={onKey}>
      <For each={own.options}>{(option, index) => <button type="button" class="menu-item" role="option" tabindex="-1"
        id={`${id()}-option-${index()}`} data-index={index()} data-value={key(option)}
        aria-selected={key(option) === value()} aria-disabled={option.disabled || undefined} aria-label={option.textValue}
        classList={{ active: key(option) === active() }}
        onPointerMove={() => { if (!option.disabled) setActive(key(option)); }}
        onMouseDown={(e) => e.preventDefault()} onClick={() => { if (!option.disabled) choose(key(option)); }}>
        <Icon name="check" size={14} style={{ opacity: key(option) === value() ? 1 : 0 }} />
        <span class="dropdown-label">{label(option)}</span>
      </button>}</For>
    </div>;
    return popover ? content() : <dialog ref={layer} class="dropdown-layer" aria-label={attrs["aria-label"]}
      aria-labelledby={attrs["aria-labelledby"] || (attrs["aria-label"] ? undefined : id())}
      on:cancel={(e) => { e.preventDefault(); close(); }}>{content()}</dialog>;
  };
  return <>
    <button {...attrs} ref={button} id={id()} type="button" role="combobox" class={`field dropdown-trigger ${own.class ?? ""}`}
      value={value()} disabled={disabled()} aria-haspopup="listbox" aria-expanded={open()} aria-controls={`${id()}-listbox`}
      aria-activedescendant={open() && active() !== undefined ? `${id()}-option-${own.options.findIndex((o) => key(o) === active())}` : undefined}
      data-shortcut-context={open() ? "none" : undefined} onClick={() => open() ? close() : show()} on:keydown={onKey}>
      <span class="dropdown-label">{label(chosen())}</span><Icon name="chevron-down" size={14} class="dropdown-chevron" />
    </button>
    <Show when={open()}><Popup /></Show>
  </>;
}
