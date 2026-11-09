import { Show, onCleanup, onMount, type JSX } from "solid-js";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";
import {
  wslDialogClose, wslFileOpen, wslFileOpenOriginal, wslFilePreview, wslMappingOpen, wslMappingPreview, wslMappingUse,
  type WslFields, type WslScope,
} from "../../bridge";
import { createFileDialog, createMappingDialog, problem, type FileDialogState, type MappingDialogState } from "./wsl-mapping";
import "../styles/wsl.css";
import { TextInput } from "../lib/TextField";

// Task 73: Qt's WslPathMappingDialog and WslFileOpenDialog. Cancel is the
// default button and waits for a running request; Enter never submits.

function Field(props: { id: string; label: string; value: string; placeholder?: string; disabled: boolean; onInput(value: string): void }) {
  return <div class="form-row"><label for={props.id}>{props.label}</label>
    <TextInput id={props.id} class="field mono" value={props.value} placeholder={props.placeholder} disabled={props.disabled}
      onInput={(event) => props.onInput(event.currentTarget.value)} onKeyDown={(event) => { if (event.key === "Enter") event.preventDefault(); }} /></div>;
}

function MappingFields(props: { fields: WslFields; disabled: boolean; set(name: keyof WslFields, value: string): void; prefix: string }) {
  return <>
    <Field id={`${props.prefix}-root`} label={tr("Allowed Windows workspace root")} value={props.fields.windows_root} placeholder="C:\Projects\Example"
      disabled={props.disabled} onInput={(v) => props.set("windows_root", v)} />
    <Field id={`${props.prefix}-mount`} label={tr("WSL drive mount root")} value={props.fields.mount_root} placeholder={tr("For example /mnt, the configured mount root")}
      disabled={props.disabled} onInput={(v) => props.set("mount_root", v)} />
    <Field id={`${props.prefix}-workspace`} label={tr("Workspace identity")} value={props.fields.workspace_id} disabled={props.disabled}
      onInput={(v) => props.set("workspace_id", v)} />
  </>;
}

function Frame(props: { id: string; title: string; busy: () => boolean; close(): void; children: JSX.Element; foot: JSX.Element }) {
  let dialog!: HTMLDialogElement;
  const origin = document.activeElement instanceof HTMLElement ? document.activeElement : undefined;
  onMount(() => { dialog.showModal(); queueMicrotask(() => dialog.querySelector<HTMLElement>("[data-initial]")?.focus()); });
  onCleanup(() => { if (dialog.open) dialog.close(); if (origin?.isConnected) origin.focus(); });
  return <dialog ref={dialog} class="dlg wsl-dlg" aria-labelledby={`${props.id}-title`} onCancel={(event) => { event.preventDefault(); props.close(); }}>
    <div class="dlg-head"><Icon name="folder-open" size={18} class="accent" /><h2 id={`${props.id}-title`}>{props.title}</h2><span class="grow" />
      <button type="button" class="icon-btn" aria-label={tr("Close")} disabled={props.busy()} onClick={props.close}><Icon name="x" size={16} /></button></div>
    <div class="dlg-body">{props.children}</div>
    <div class="dlg-foot">{props.foot}</div>
  </dialog>;
}

/** Map Windows path…: refusals before the dialog shows go to `refused`. */
export function WslMappingDialog(props: { alias: string; onUse(path: string): void; onClose(): void; refused(text: string): void; state?: MappingDialogState }) {
  const state = props.state ?? createMappingDialog({ open: wslMappingOpen, preview: wslMappingPreview, use: wslMappingUse, close: wslDialogClose });
  let shown = false;
  void state.open(props.alias).then(() => { shown = true; }, (error) => { props.refused(problem(error)); props.onClose(); });
  const close = () => { if (state.close()) props.onClose(); };
  const use = async () => { const path = await state.use(); if (path) { props.onUse(path); props.onClose(); } };
  onCleanup(() => { if (!shown) state.close(); });
  return <Show when={state.target()}>{(target) =>
    <Frame id="wsl-map" title={tr("Map a Windows path to this WSL environment")} busy={state.busy} close={close}
      foot={<>
        <button type="button" class="btn" disabled={!state.canPreview()} onClick={() => void state.preview()}>{tr("Preview mapping")}</button>
        <button type="button" class="btn" disabled={!state.canUse()} onClick={() => void use()}>{tr("Use mapped path")}</button>
        <span class="spacer" />
        <button type="button" class="btn primary" data-initial disabled={state.busy()} onClick={close}>{tr("Cancel")}</button>
      </>}>
      <p class="wsl-target">{tr("Machine: %1\nDistribution: %2\nLinux user: %3\nEnvironment: %4", target().alias, target().distribution, target().user, target().environment)}</p>
      <p class="muted">{tr("Define the workspace alias explicitly. Preview is lexical only: it does not create folders, change saved project identities or prove that the mount exists.")}</p>
      <Field id="wsl-map-source" label={tr("Windows path to map")} value={state.source()} disabled={state.busy()} onInput={(v) => state.setSource(v)} />
      <MappingFields prefix="wsl-map" fields={state.fields()} disabled={state.busy()} set={(name, value) => state.setField(name, value)} />
      <label class="check"><input type="checkbox" checked={state.remember()} disabled={state.busy()} onChange={(e) => state.setRemember(e.currentTarget.checked)} />{tr("Remember this mapping on this device after using it")}</label>
      <p class="wsl-notice selectable" role="status">{state.notice()}</p>
    </Frame>}
  </Show>;
}

/** Open mapped Windows original…: `onDone` gets the notice of what opened. */
export function WslFileOpenDialog(props: { alias: string; scope: WslScope; onDone(notice: string): void; onClose(): void; state?: FileDialogState }) {
  const state = props.state ?? createFileDialog({ open: wslFileOpen, preview: wslFilePreview, openOriginal: wslFileOpenOriginal, close: wslDialogClose });
  void state.open(props.alias, props.scope);
  const close = () => { if (state.close()) props.onClose(); };
  const open = async () => { const notice = await state.openOriginal(); if (notice) { props.onDone(notice); props.onClose(); } };
  onCleanup(() => state.close());
  return <Frame id="wsl-file" title={tr("Open a mapped Windows original")} busy={state.busy} close={close}
    foot={<>
      <button type="button" class="btn" disabled={!state.canPreview()} onClick={() => void state.preview()}>{tr("Preview and verify")}</button>
      <button type="button" class="btn" disabled={!state.canOpen()} onClick={() => void open()}>{tr("Open mapped original")}</button>
      <span class="spacer" />
      <button type="button" class="btn primary" data-initial disabled={state.busy()} onClick={close}>{tr("Cancel")}</button>
    </>}>
    <p class="wsl-target">{state.target() ? tr("Machine: %1\nDistribution: %2\nLinux user: %3", state.target()!.alias, state.target()!.distribution, state.target()!.user) : tr("Reading the selected local WSL connection…")}</p>
    <p class="wsl-target selectable">{tr("Conversation: %1\nNative path: %2", props.scope.session, props.scope.path)}</p>
    <p class="muted">{tr("This opens the original Windows file, not a downloaded copy. Editing it changes that file. The selected conversation must still authorize the WSL read, and both sides must have matching contents. An explicit mapping is an alias you select, not proof that two files are the same filesystem object.")}</p>
    <MappingFields prefix="wsl-file" fields={state.fields()} disabled={state.busy() || !state.loaded()} set={(name, value) => state.setField(name, value)} />
    <label class="check"><input type="checkbox" checked={state.remember()} disabled={state.busy() || !state.loaded()} onChange={(e) => state.setRemember(e.currentTarget.checked)} />{tr("Remember this explicit mapping on this device")}</label>
    <p class="wsl-notice selectable" role="status">{state.notice()}</p>
  </Frame>;
}
