import { createEffect, createMemo, createSignal, For, onCleanup, Show } from "solid-js";
import { keys } from "../../store/shortcuts";
import { activateGoTo, closeGoTo, goToItems, goToOrigin } from "../../store/goTo";
import { goToKey, rankGoTo, visibleGoToRows, type GoToKeyState } from "./goTo";

export function GoToDialog() {
  let field: HTMLInputElement | undefined;
  let frame: HTMLDivElement | undefined;
  const [query, setQuery] = createSignal("");
  const [current, setCurrent] = createSignal(0);
  const [start, setStart] = createSignal(0);
  const closeState: GoToKeyState = { typed: [] };
  const ranked = createMemo(() => rankGoTo(goToItems(), query()));
  const visible = createMemo(() => visibleGoToRows(ranked(), current(), 12, start()));
  createEffect(() => {
    const next = visible().start;
    if (next !== start()) setStart(next);
  });
  createEffect(() => {
    if (!goToOrigin()) return;
    setQuery("");
    setCurrent(0);
    setStart(0);
    closeState.typed = [];
    queueMicrotask(() => field?.focus());
    const outside = (event: MouseEvent) => { if (!frame?.contains(event.target as Node)) closeGoTo(); };
    const t = setTimeout(() => addEventListener("mousedown", outside));
    onCleanup(() => {
      clearTimeout(t);
      removeEventListener("mousedown", outside);
    });
  });
  const onKey = (event: KeyboardEvent) => {
    const action = goToKey(event, closeState, keys("go.anywhere"));
    if (action === "none") return;
    event.preventDefault();
    event.stopPropagation();
    if (action === "close") { closeGoTo(); return; }
    if (action === "hold") return;
    const count = ranked().length;
    if (action === "up") setCurrent(Math.max(0, current() - 1));
    else if (action === "down") setCurrent(Math.min(Math.max(0, count - 1), current() + 1));
    else if (action === "pageUp") setCurrent(Math.max(0, current() - 10));
    else if (action === "pageDown") setCurrent(Math.min(Math.max(0, count - 1), current() + 10));
    else if (action === "activate") {
      const item = ranked()[current()];
      if (item) activateGoTo(item);
    }
  };
  return (
    <Show when={goToOrigin()}>
      <div class="go-to" data-shortcut-context="none" ref={frame} role="dialog" aria-label="Go to">
        <input ref={field} class="go-to-query" value={query()} placeholder="Go to… (type > for commands)" onInput={(event) => { setQuery(event.currentTarget.value); setCurrent(0); }} onKeyDown={onKey} />
        <Show when={visible().rows.length} fallback={<div class="go-to-empty">No matching results</div>}>
          <div class="go-to-results" role="listbox" aria-label="Results">
            <For each={visible().rows}>{(item, index) => (
              <button type="button" class="go-to-row" classList={{ on: visible().start + index() === current() }} role="option" aria-selected={visible().start + index() === current()}
                onMouseEnter={() => setCurrent(visible().start + index())} onClick={() => activateGoTo(item)}>
                <span class="go-to-main">{item.title}</span><span class="go-to-detail">{item.detail}</span><span class="go-to-chord">{item.chord}</span>
              </button>
            )}</For>
          </div>
        </Show>
      </div>
    </Show>
  );
}
