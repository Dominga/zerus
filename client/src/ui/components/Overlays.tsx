import { For, Show, createEffect, onCleanup } from "solid-js";
import { Icon } from "../lib/icons";
import { ProviderGlyph } from "../sessions/SessionCard";
import { agentKind } from "../../store/catalog";
import { menuItemActivates } from "../sessions/actions-ui";
import { menu, setMenu } from "../../store/ui";
import { dismissNotice, notices } from "../../store/notices";

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
        <div class="menu" role="menu" data-shortcut-context="none" ref={el} style={{ left: `${m().x}px`, top: `${m().y}px` }} onKeyDown={onKey}>
          <For each={m().items}>
            {(item) =>
              "separator" in item ? <div class="menu-sep" role="separator" /> :
              "heading" in item ? <div class="menu-label">{item.heading}</div> : (
                <button class="menu-item" classList={{ danger: item.danger }} role={item.checked !== undefined ? "menuitemcheckbox" : "menuitem"}
                  aria-checked={item.checked} aria-disabled={item.disabled} title={item.hint}
                  onClick={() => { if (!menuItemActivates(item)) return; setMenu(undefined); item.action?.(); }}>
                  <Show when={item.checked !== undefined} fallback={<Show when={item.icon} fallback={<span style={{ width: "14px" }} />}><Icon name={item.icon!} size={14} /></Show>}>
                    <Icon name={item.checked && item.checked !== "mixed" ? "check" : "minus"} size={14} style={{ opacity: item.checked ? 1 : 0 }} />
                  </Show>
                  <Show when={item.agent}><span class="menu-agent" style={{ color: `var(--z-agent-${agentKind(item.agent)})` }}><ProviderGlyph agent={agentKind(item.agent)} size={13} /></span></Show>
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

/**
 * The window's one notice bus, in the design's bottom-right corner: copies,
 * action results, updates and refusals. Notices let clicks through to the
 * controls under them; only a sticky notice's Dismiss takes a click.
 */
export function Toasts() {
  return (
    <div class="toasts" aria-live="polite">
      <For each={notices()}>
        {(n) => (
          <div class={`toast ${n.tone}`} classList={{ sticky: n.sticky }}>
            <Show when={n.sticky}><button class="icon-btn dismiss" aria-label="Dismiss" onClick={() => dismissNotice(n.id)}><Icon name="x" size={14} /></button></Show>
            <Show when={n.title}><b>{n.title}</b></Show>
            <span class="note" classList={{ muted: !!n.title }}>{n.text}</span>
          </div>
        )}
      </For>
    </div>
  );
}
