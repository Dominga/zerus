import { For, Show, createEffect, onCleanup } from "solid-js";
import { Icon } from "../lib/icons";
import { menu, setMenu, toasts } from "../../store/ui";

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
