import { Show, createEffect, createSignal, onCleanup, onMount } from "solid-js";
import { Icon } from "../lib/icons";
import { composer } from "../../store/composer";
import { openInspector } from "../../store/inspector";
import type { ChipTone } from "../../types";
import { tr } from "../../i18n";

const USUAL_CACHE_CLEAR = () => tr("Clear this agent’s conversation, like /clear in Terminal. Your unsent message stays here.");

const toneClass = (tone: ChipTone) => (tone === "danger" ? "danger" : tone === "warning" ? "warn" : tone === "success" ? "ok" : "");

/** The cache chip and its popover (SessionsWindow.cpp 987-1002, 3789-3795; CacheStatus::Chip). */
export function CacheChip(props: { compact: boolean }) {
  const v = () => composer.view()!;
  const [open, setOpen] = createSignal(false);
  const [pos, setPos] = createSignal({ left: 0, bottom: 0 });
  let chip!: HTMLButtonElement;
  let pop: HTMLDivElement | undefined;
  const close = (returnFocus = false) => { setOpen(false); if (returnFocus) chip.focus(); };
  const outside = (e: PointerEvent) => { if (open() && !pop?.contains(e.target as Node) && !chip.contains(e.target as Node)) close(); };
  onMount(() => addEventListener("pointerdown", outside, true));
  onCleanup(() => removeEventListener("pointerdown", outside, true));
  // The chip goes (a session without a cache): so does its popover.
  createEffect(() => { if (!v().cache.visible) setOpen(false); });
  const detail = () => (v().cache_warning ? `${v().cache_warning}\n` : "") + v().cache_details;
  return (
    <Show when={v().cache.visible}>
      <span class="att-anchor">
        <button ref={chip} type="button" class={`tchip ${toneClass(v().cache.tone)}`} classList={{ on: open() }} aria-haspopup="dialog" aria-expanded={open()}
          aria-label={v().cache.label} title={detail()}
          onClick={(e) => {
            const r = chip.getBoundingClientRect();
            setPos({ left: Math.max(8, Math.min(r.left, innerWidth - 368)), bottom: innerHeight - r.top + 6 });
            setOpen(!open());
            if (open()) queueMicrotask(() => (e.detail === 0 ? pop?.querySelector<HTMLElement>("button:not(:disabled)") ?? pop : pop)?.focus({ preventScroll: true }));
          }}>
          <Icon name={v().cache.tone === "success" ? "timer" : "context-warning"} size={13} />
          {props.compact ? v().cache.short_label : v().cache.label}
        </button>
        <Show when={open()}>
          <div class="att-pop cache-pop" ref={pop} tabindex="-1" role="dialog" aria-label={tr("Prompt cache")} data-shortcut-context="none"
            style={{ left: `${pos().left}px`, bottom: `${pos().bottom}px` }}
            onKeyDown={(e) => { if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); close(true); } }}>
            <Show when={v().cache_warning}><p class="cache-warning">{v().cache_warning}</p></Show>
            <p class="cache-detail">{v().cache_details}</p>
            <div class="cache-actions">
              {/* Qt's Usage details: the side panel's Details tab, with the account usage (993-1000).
                  Both actions remove the focused button with the popover: focus goes back to the chip, as on Escape. */}
              <button type="button" class="btn small" onClick={() => { close(true); openInspector("details"); }}>{tr("Usage details")}</button>
              {/* Shown with a warning only (3793); enabled by the cache rule, not Clear session's (3794). */}
              <Show when={v().cache_warning}>
                <button type="button" class="btn small" disabled={!v().clear.cache_enabled} title={v().clear.cache_reason || USUAL_CACHE_CLEAR()}
                  onClick={() => { close(true); composer.openClear(); }}>{tr("Clear context")}</button>
              </Show>
            </div>
          </div>
        </Show>
      </span>
    </Show>
  );
}
