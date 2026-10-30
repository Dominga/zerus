import { createEffect, createMemo, createSignal, For, Show } from "solid-js";
import { terminalRule } from "../../store/shortcuts";
import { closeShortcutHelp, customizeShortcutHelp, shortcutHelpOrigin } from "../../store/goTo";
import { filterShortcutRows, shortcutHelpRows } from "./shortcutHelp";

export function ShortcutHelpDialog() {
  let dialog: HTMLDialogElement | undefined;
  let filter: HTMLInputElement | undefined;
  const [needle, setNeedle] = createSignal("");
  const rows = createMemo(() => filterShortcutRows(shortcutHelpRows(), needle()));
  const groups = createMemo(() => [...new Set(rows().map((row) => row.group))]);
  createEffect(() => {
    if (!shortcutHelpOrigin() || !dialog || dialog.open) return;
    setNeedle("");
    dialog.show();
    queueMicrotask(() => filter?.focus());
  });
  return (
    <Show when={shortcutHelpOrigin()}>
      <dialog class="dlg shortcut-help" ref={dialog} aria-labelledby="shortcut-help-title" onCancel={(event) => { event.preventDefault(); closeShortcutHelp(); }} onClose={() => closeShortcutHelp()}>
        <div class="dlg-head"><h2 id="shortcut-help-title">Keyboard shortcuts</h2></div>
        <div class="dlg-body">
          <input ref={filter} class="dlg-input" placeholder="Filter shortcuts" value={needle()} onInput={(event) => setNeedle(event.currentTarget.value)}
            onKeyDown={(event) => { if (event.key === "Escape") { event.preventDefault(); closeShortcutHelp(); } }} />
          <div class="shortcut-help-list">
            <For each={groups()}>{(group) => (
              <section>
                <h3>{group}</h3>
                <For each={rows().filter((row) => row.group === group)}>{(row) => (
                  <div class="shortcut-help-row"><span>{row.title}</span><span class="shortcut-help-chord">{row.chord}</span></div>
                )}</For>
              </section>
            )}</For>
          </div>
          <p class="set-foot">{terminalRule()}</p>
        </div>
        <div class="dlg-foot"><button class="btn" onClick={() => closeShortcutHelp()}>Close</button><span class="spacer" /><button class="btn primary" data-initial onClick={customizeShortcutHelp}>Customize…</button></div>
      </dialog>
    </Show>
  );
}
