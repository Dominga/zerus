import { For, Show, createEffect, onCleanup, type JSX } from "solid-js";
import { Icon } from "../lib/icons";
import { ProviderGlyph } from "../screens/sessions/SessionCard";
import { menu, setMenu, toasts, dialog, setDialog } from "../state";

/** Context and dropdown menus share one positioned popover. */
export function MenuLayer() {
  let el: HTMLDivElement | undefined;
  createEffect(() => {
    const m = menu();
    if (!m || !el) return;
    const r = el.getBoundingClientRect();
    el.style.left = `${Math.min(m.x, innerWidth - r.width - 8)}px`;
    el.style.top = `${Math.min(m.y, innerHeight - r.height - 8)}px`;
    (el.querySelector(".menu-item:not([aria-disabled=true])") as HTMLElement | null)?.focus();
    const close = (e: Event) => {
      if (e instanceof KeyboardEvent && e.key !== "Escape") return;
      if (e instanceof MouseEvent && el?.contains(e.target as Node)) return;
      setMenu(undefined);
    };
    const t = setTimeout(() => {
      addEventListener("mousedown", close);
      addEventListener("keydown", close);
      addEventListener("blur", close);
    });
    onCleanup(() => {
      clearTimeout(t);
      removeEventListener("mousedown", close);
      removeEventListener("keydown", close);
      removeEventListener("blur", close);
    });
  });
  const onKey = (e: KeyboardEvent) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const items = [...(el?.querySelectorAll<HTMLElement>(".menu-item:not([aria-disabled=true])") ?? [])];
    const i = items.indexOf(document.activeElement as HTMLElement);
    items[(i + (e.key === "ArrowDown" ? 1 : -1) + items.length) % items.length]?.focus();
  };
  return (
    <Show when={menu()}>
      {(m) => (
        <div class="menu" role="menu" ref={el} style={{ left: `${m().x}px`, top: `${m().y}px` }} onKeyDown={onKey}>
          <For each={m().items}>
            {(item) =>
              "separator" in item ? <div class="menu-sep" role="separator" /> :
              "heading" in item ? <div class="menu-label">{item.heading}</div> : (
                <button class="menu-item" classList={{ danger: item.danger }} role={item.checked !== undefined ? "menuitemcheckbox" : "menuitem"}
                  aria-checked={item.checked} aria-disabled={item.disabled} onClick={() => { setMenu(undefined); item.action?.(); }}>
                  <Show when={item.checked !== undefined} fallback={<Show when={item.icon} fallback={<span style={{ width: "14px" }} />}><Icon name={item.icon!} size={14} /></Show>}>
                    <Icon name={item.checked ? "check" : "minus"} size={14} style={{ opacity: item.checked ? 1 : 0 }} />
                  </Show>
                  {/* Accounts lead with their type: the provider glyph, then the name. */}
                  <Show when={item.agent}><span class="menu-agent" style={{ color: `var(--z-agent-${item.agent})` }}><ProviderGlyph agent={item.agent!} size={13} /></span></Show>
                  <span>{item.label}</span>
                  <Show when={item.kbd}><span class="kbd">{item.kbd}</span></Show>
                </button>
              )
            }
          </For>
        </div>
      )}
    </Show>
  );
}

export function Toasts() {
  return (
    <div class="toasts" aria-live="polite">
      <For each={toasts.items}>
        {(t) => (
          <div class="toast">
            <b>{t.title}</b>
            <span class="muted">{t.text}</span>
          </div>
        )}
      </For>
    </div>
  );
}

/** Native <dialog> opened with showModal(); closes back to the dialog signal. */
export function Dialog(props: { id: string; title: string; width?: string; children: JSX.Element; footer?: JSX.Element; icon?: string }) {
  let el!: HTMLDialogElement;
  createEffect(() => {
    if (dialog() === props.id) {
      if (!el.open) el.showModal();
    } else if (el.open) el.close();
  });
  return (
    <dialog class="dlg" ref={el} aria-labelledby={`${props.id}-title`} style={props.width ? { width: props.width } : undefined}
      onClose={() => { if (dialog() === props.id) setDialog(undefined); }}>
      <Show when={dialog() === props.id}>
        <div class="dlg-head">
          <Show when={props.icon}><Icon name={props.icon!} size={18} class="accent" /></Show>
          <h2 id={`${props.id}-title`}>{props.title}</h2>
          <span class="grow" />
          <button class="icon-btn" aria-label="Close" onClick={() => setDialog(undefined)}><Icon name="x" size={16} /></button>
        </div>
        <div class="dlg-body">{props.children}</div>
        <Show when={props.footer}><div class="dlg-foot">{props.footer}</div></Show>
      </Show>
    </dialog>
  );
}

/** Destructive confirmations: the safe action gets initial focus. */
export function Confirm(props: { id: string; title: string; body: JSX.Element; confirm: string; danger?: boolean; onConfirm?: () => void }) {
  return (
    <Dialog id={props.id} title={props.title} width="460px" icon={props.danger ? "triangle-alert" : "circle-question-mark"}
      footer={<>
        <span class="spacer" />
        <button class="btn" autofocus onClick={() => setDialog(undefined)}>Cancel</button>
        <button class={props.danger ? "btn solid-danger" : "btn primary"} onClick={() => { props.onConfirm?.(); setDialog(undefined); }}>{props.confirm}</button>
      </>}>
      <div class="confirm-body">{props.body}</div>
    </Dialog>
  );
}
