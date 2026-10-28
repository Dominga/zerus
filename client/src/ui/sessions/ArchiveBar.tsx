import { For, Show, createEffect, createSignal, on, onCleanup } from "solid-js";
import { Icon } from "../lib/icons";
import { archive, buttonText } from "../../store/archive";
import {
  MAX_DAY, MIN_DAY, PERIODS, applyDraft, draftHint, draftValid, editDraft, pickerLabel, rangeCaption, stageDraft, type Draft, type Period,
} from "./archive-dates";

// The Archive bar above the list under the Archive filter (Qt's
// ArchiveBrowser): the date range and Refresh, the status with Newer and
// Older, then the errors by machine.

export function ArchiveBar() {
  const [width, setWidth] = createSignal(0);
  const observe = (el: HTMLElement) => {
    const ro = new ResizeObserver(() => setWidth(el.clientWidth));
    ro.observe(el);
    onCleanup(() => ro.disconnect());
  };
  const status = () => archive.status();
  // A new window starts at the top of the list (Qt's windowChanged).
  createEffect(on(archive.window, () => document.querySelector(".sp-panel .sp-scroll")?.scrollTo({ top: 0 }), { defer: true }));
  return (
    <div class="archive-browser" ref={observe}>
      <div class="archive-bar">
        <DatePicker />
        <button class="icon-btn archive-refresh" title="Refresh archive" aria-label="Refresh archive" disabled={!status().refreshEnabled} onClick={() => archive.refresh()}>
          <Icon name="refresh" size={16} />
        </button>
      </div>
      <div class="archive-bar">
        <span class="archive-status ell" title={status().hint} aria-description={status().hint}>{status().summary}</span>
        <Show when={status().newer.visible}>
          <button class="btn small archive-nav" title="Newer sessions" aria-label="Newer sessions" disabled={!status().newer.enabled} onClick={() => archive.newer()}>
            <Icon name="chevron-left" size={14} />{buttonText(width(), "Newer")}
          </button>
        </Show>
        <Show when={status().older.visible}>
          <button class="btn small archive-nav" title="Older sessions" aria-label="Older sessions" disabled={!status().older.enabled} onClick={() => archive.older()}>
            <Icon name="chevron-right" size={14} />{buttonText(width(), "Older")}
          </button>
        </Show>
      </div>
      <Show when={status().error}>
        <div class="archive-error" title={status().errorHint}>{status().error}</div>
      </Show>
    </div>
  );
}

/**
 * The archived date (ArchiveDatePicker): a button with the applied range, and
 * a panel where the quick ranges and the two dates change only the draft;
 * Apply range commits it, Cancel, Escape or a click outside drop it, and
 * focus returns to the picker button.
 */
function DatePicker() {
  let button: HTMLButtonElement | undefined;
  let panel: HTMLDivElement | undefined;
  const [open, setOpen] = createSignal(false);
  const [draft, setDraft] = createSignal<Draft>({ period: 0, from: "", until: "" });
  const [place, setPlace] = createSignal({ left: 0, top: 0, width: 480 });

  const stage = (period: Period) => setDraft(stageDraft(period, archive.range(), new Date()));
  const edit = (edited: "from" | "until", value: string) => setDraft((d) => editDraft(d, edited, value, new Date()));
  const close = () => { setOpen(false); button?.focus(); };
  const apply = () => {
    const out = applyDraft(draft(), archive.range());
    if (!out) return;
    close();
    archive.setRange(out.range);
  };
  const show = () => {
    stage(archive.range().period);
    const r = button!.getBoundingClientRect();
    const width = Math.min(480, innerWidth - 16);
    setPlace({ left: Math.max(8, Math.min(r.left, innerWidth - width - 8)), top: r.bottom + 6, width });
    setOpen(true);
  };

  createEffect(() => {
    if (!open() || !panel || !button) return;
    // Below the button when it fits, else above it; inside the window.
    const r = button.getBoundingClientRect();
    const height = panel.offsetHeight;
    const below = r.bottom + 6;
    const top = below + height <= innerHeight - 8 ? below : r.top - height - 6;
    setPlace((p) => ({ ...p, top: Math.max(8, Math.min(top, innerHeight - height - 8)) }));
    panel.querySelector<HTMLInputElement>("input")?.focus();
    // The press only closes the panel and focus returns to the picker, as Qt's popup consumes it.
    const outside = (e: MouseEvent) => {
      if (panel?.contains(e.target as Node) || button?.contains(e.target as Node)) return;
      e.preventDefault();
      close();
    };
    const t = setTimeout(() => addEventListener("mousedown", outside));
    onCleanup(() => { clearTimeout(t); removeEventListener("mousedown", outside); });
  });

  return (
    <>
      <button ref={button} class="archive-period" title={pickerLabel(archive.range())} aria-label={pickerLabel(archive.range())}
        aria-haspopup="dialog" aria-expanded={open()} onClick={() => (open() ? setOpen(false) : show())}>
        <Icon name="calendar" size={16} />
        <span class="ell">{rangeCaption(archive.range())}</span>
        <Icon name="chevron-down" size={14} />
      </button>
      <Show when={open()}>
        <div class="archive-pop" ref={panel} role="dialog" aria-label="Archived date"
          style={{ left: `${place().left}px`, top: `${place().top}px`, width: `${place().width}px` }}
          onKeyDown={(e) => { if (e.key === "Escape") { e.stopPropagation(); close(); } }}>
          <div class="archive-pop-title">Archived date</div>
          <div class="archive-pop-body">
            <div class="archive-pop-custom">
              <label class="archive-pop-label" for="archive-from">From</label>
              <input id="archive-from" class="field" type="date" min={MIN_DAY} max={MAX_DAY} aria-label="Archived from, inclusive"
                value={draft().from} onInput={(e) => edit("from", e.currentTarget.value)} />
              <label class="archive-pop-label" for="archive-until">Through</label>
              <input id="archive-until" class="field" type="date" min={MIN_DAY} max={MAX_DAY} aria-label="Archived through, inclusive"
                value={draft().until} onInput={(e) => edit("until", e.currentTarget.value)} />
              <div class="hint archive-pop-hint" classList={{ invalid: !draftValid(draft()) }}>{draftHint(draft())}</div>
              <div class="archive-pop-actions">
                <button type="button" class="btn primary" disabled={!draftValid(draft())} onClick={apply}>Apply range</button>
                <button type="button" class="btn" onClick={close}>Cancel</button>
              </div>
            </div>
            <div class="archive-pop-quick">
              <div class="archive-pop-label">Quick ranges</div>
              <For each={PERIODS}>
                {(caption, i) => (
                  <button type="button" class="archive-preset" aria-pressed={draft().period === i()} onClick={() => stage(i() as Period)}>{caption}</button>
                )}
              </For>
            </div>
          </div>
        </div>
      </Show>
    </>
  );
}
