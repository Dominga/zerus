import { For, Show, createSignal, onCleanup, onMount } from "solid-js";
import { Icon } from "../lib/icons";
import "../styles/ironrules.css";
import { tr } from "../../i18n";
import { trText } from "../../text";
import { accountPermissionCheck, type AccountEditValues, type PermissionCheck, type PermissionEditor } from "../../bridge";
import type { AccountPrompt } from "../../store/accountManagement";
import { EDITOR, afterEditor, saveAction, withValue } from "./account-permissions";

type Check = (handle: string, selection: Record<string, string>) => Promise<PermissionCheck>;

/** Task 68: Qt's PermissionCatalogDialog. One selector per discovered control,
 * unknown saved settings kept visible until removed, Save disabled while the
 * core reports a problem with the selection. */
function NativeEditor(props: { handle: string; editor: PermissionEditor; selection: Record<string, string>; subtitle: string; check: Check;
  save(check: PermissionCheck): void; cancel(): void }) {
  const [selection, setSelection] = createSignal(props.selection);
  const [check, setCheck] = createSignal<PermissionCheck>();
  const [error, setError] = createSignal("");
  let generation = 0;
  const ask = async (next: Record<string, string>) => {
    const current = ++generation;
    setSelection(next); setCheck(undefined);
    try { const answer = await props.check(props.handle, next); if (current === generation) { setCheck(answer); setError(""); } }
    catch (thrown) { if (current === generation) setError(String(thrown)); }
  };
  let dialog!: HTMLDialogElement;
  onMount(() => { dialog.showModal(); void ask(props.selection); queueMicrotask(() => dialog.querySelector<HTMLElement>("select, button.nperm-remove")?.focus()); });
  onCleanup(() => { if (dialog.open) dialog.close(); });
  const problem = () => { const c = check(); return c && c.problem.source ? trText(c.problem) : ""; };
  const canSave = () => !!check() && !problem() && !props.editor.malformed;
  const reset = async () => { try { props.save(await props.check(props.handle, {})); } catch (thrown) { setError(String(thrown)); } };
  return (
    <dialog ref={dialog} class="dlg nperm" style={{ width: "520px" }} aria-labelledby="nperm-title" onCancel={(event) => { event.preventDefault(); props.cancel(); }}>
      <form onSubmit={(event) => { event.preventDefault(); const c = check(); if (c && canSave()) props.save(c); }}>
        <div class="dlg-head"><Icon name="shield-check" size={18} class="accent" /><h2 id="nperm-title">{tr("Native permissions")}</h2><span class="grow" /><button type="button" class="icon-btn" aria-label={tr("Close")} onClick={props.cancel}><Icon name="x" size={16} /></button></div>
        <div class="dlg-body">
          <Show when={props.subtitle}><p class="muted">{props.subtitle}</p></Show>
          <Show when={problem() || error()}><p class="nperm-warning" role="alert">{error() || problem()}</p></Show>
          <Show when={!props.editor.malformed} fallback={<>
            <p class="muted">{tr("The saved native permission settings cannot be read. Use %1 settings to reset them, then choose again.", props.editor.harness)}</p>
            <button type="button" class="btn small" onClick={() => void reset()}>{trText(props.editor.reset_label)}</button>
          </>}>
            <For each={check()?.rows ?? []}>{(row) => (
              <div class="nperm-row" data-control={row.id}>
                <label class="nperm-label" for={`nperm-${row.id}`}>{trText(row.label)}</label>
                <Show when={row.description.source && trText(row.description)}><p class="hint">{trText(row.description)}</p></Show>
                <Show when={!row.unknown} fallback={
                  <div class="row"><span class="mono selectable grow">{row.selected}</span>
                    <button type="button" class="btn small nperm-remove" onClick={() => void ask(withValue(selection(), row.id, ""))}>{tr("Remove")}</button></div>
                }>
                  <select id={`nperm-${row.id}`} class="field" value={row.selected} onChange={(event) => void ask(withValue(selection(), row.id, event.currentTarget.value))}>
                    <option value="">{trText(props.editor.reset_label)}</option>
                    <For each={row.options}>{(option) => <option value={option.value} title={option.description || undefined}>{option.label}</option>}</For>
                    <Show when={row.unavailable}><option value={row.selected} title={tr("This saved value is no longer offered.")}>{tr("Unavailable: %1", row.selected)}</option></Show>
                  </select>
                </Show>
              </div>
            )}</For>
          </Show>
        </div>
        <div class="dlg-foot"><span class="spacer" /><button type="button" class="btn" onClick={props.cancel}>{tr("Cancel")}</button><button type="submit" class="btn primary" disabled={!canSave()}>{tr("Save")}</button></div>
      </form>
    </dialog>
  );
}

/** Task 68: Qt's account permissions dialog. The harness's own settings,
 * Bypass, its native options (or the native editor for several controls),
 * and a saved choice no longer offered kept visible with Save blocked. */
export default function AccountPermissionsDialog(props: { prompt: AccountPrompt; editor: PermissionEditor; close(handle: string): void; confirm(values: AccountEditValues, handle: string): void; check?: Check }) {
  const view = props.prompt, check = props.check ?? accountPermissionCheck;
  const [choices, setChoices] = createSignal(props.editor.choices);
  const [value, setValue] = createSignal(props.editor.initial);
  const [selection, setSelection] = createSignal(props.editor.selected);
  const [editing, setEditing] = createSignal(false);
  const subtitle = tr("%1 on %2", view.label, view.machine);
  let dialog!: HTMLDialogElement, select!: HTMLSelectElement;
  onMount(() => { dialog.showModal(); queueMicrotask(() => select.focus()); });
  onCleanup(() => { dialog.close(); if (view.origin.isConnected) view.origin.focus(); });
  const chosen = () => choices().find((c) => c.value === value());
  const hasEditor = () => choices().some((c) => c.editor);
  const openEditor = () => setEditing(true);
  const choose = (next: string) => {
    if (choices().find((c) => c.value === next)?.editor) { select.value = value(); openEditor(); return; }
    setValue(next);
  };
  const saved = (answer: PermissionCheck) => {
    setEditing(false);
    setSelection(Object.fromEntries(answer.rows.filter((row) => row.selected).map((row) => [row.id, row.selected])));
    const next = afterEditor(choices(), answer);
    setChoices(next.choices); setValue(next.value);
    queueMicrotask(() => select.focus());
  };
  const submit = (event: Event) => {
    event.preventDefault();
    const action = saveAction(choices(), value());
    if (action === "editor") openEditor();
    else if (action === "save") props.confirm({ mode: value() }, view.handle);
  };
  return <dialog ref={dialog} class="dlg" style={{ width: "520px" }} aria-labelledby="accounts-permissions-title" onCancel={(event) => { event.preventDefault(); if (!editing()) props.close(view.handle); }} onClose={() => props.close(view.handle)}>
    <form onSubmit={submit}>
      <div class="dlg-head"><Icon name="shield-check" size={18} class="accent" /><h2 id="accounts-permissions-title">{tr("Account permissions")}</h2><span class="grow" /><button type="button" class="icon-btn" aria-label={tr("Close")} onClick={() => props.close(view.handle)}><Icon name="x" size={16} /></button></div>
      <div class="dlg-body"><p>{subtitle}</p>
        <div class="row">
          <select ref={select} class="field acct-perm-select grow" aria-label={tr("Permission mode")} value={value()} onChange={(event) => choose(event.currentTarget.value)}>
            <For each={choices()}>{(choice) => <option value={choice.value} title={trText(choice.tooltip)}>{trText(choice.label)}</option>}</For>
          </select>
          <Show when={hasEditor()}><button type="button" class="btn small" onClick={openEditor}>{tr("Edit…")}</button></Show>
        </div>
        <p classList={{ danger: !!chosen()?.bypass }}>{chosen() ? trText(chosen()!.explanation) : ""}</p>
        <p class="hint">{trText(props.editor.source_note)}</p>
        <p class="hint">{tr("New and resumed sessions only. Running sessions keep their mode.")}</p>
      </div>
      <div class="dlg-foot"><span class="spacer" /><button type="button" class="btn" onClick={() => props.close(view.handle)}>{tr("Cancel")}</button><button type="submit" class="btn primary" disabled={saveAction(choices(), value()) === "blocked"}>{tr("Save")}</button></div>
    </form>
    <Show when={editing()}>
      <NativeEditor handle={view.handle} editor={props.editor} selection={selection()} subtitle={subtitle} check={check} save={saved} cancel={() => { setEditing(false); queueMicrotask(() => select.focus()); }} />
    </Show>
  </dialog>;
}

