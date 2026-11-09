import { Show, createSignal, onCleanup, onMount } from "solid-js";
import { accountBindExisting } from "../../bridge";
import { Icon } from "../lib/icons";
import { tr } from "../../i18n";
import { TextInput } from "../lib/TextField";

/** Qt's OpenCodeExistingDialog: preview the exact native conversation, then bind it. */
export default function OpenCodeBindDialog(props: {
  host: string; machine: string; agentHome: string; close(): void; bound(name: string): void;
}) {
  const requestId = crypto.randomUUID();
  const [nativeId, setNativeId] = createSignal("");
  const [directory, setDirectory] = createSignal("");
  const [name, setName] = createSignal("");
  const [identity, setIdentity] = createSignal("");
  const [notice, setNotice] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  let dialog!: HTMLDialogElement;
  onMount(() => dialog.showModal());
  onCleanup(() => dialog.close());
  const filled = () => !!props.agentHome && !!nativeId().trim() && !!directory().trim() && !!name().trim();
  const selection = (preview: boolean) => ({
    host: props.host, request_id: requestId, name: name().trim(), conversation_id: nativeId().trim(),
    directory: directory().trim(), agent_home: props.agentHome, preview,
    ...(preview ? {} : { expected_identity: identity() }),
  });
  const submit = async (commit: boolean) => {
    if (busy() || !filled() || (commit && !identity())) return;
    setBusy(true);
    setNotice(commit ? tr("Recording the selected binding…") : tr("Verifying the selected native conversation…"));
    try {
      const result = (await accountBindExisting(selection(!commit))) as Record<string, unknown>;
      const exact = result.schema_version === 1 && result.native_changed === false
        && result.request_id === requestId && result.name === name().trim()
        && result.conversation_id === nativeId().trim() && result.directory === directory().trim()
        && result.agent_home === props.agentHome && /^[0-9a-f]{64}$/.test(String(result.identity ?? ""));
      const status = String(result.status ?? "");
      if (!exact) {
        setIdentity("");
        setNotice(tr("The reply does not match this selection. Nothing is confirmed; preview again."));
        return;
      }
      if (commit) {
        if (result.identity !== identity() || !["bound", "already_bound"].includes(status)) {
          setNotice(tr("The reply does not match this selection. Nothing is confirmed; preview again."));
          return;
        }
        props.bound(String(result.name ?? name().trim()));
        return;
      }
      if (!["preview", "already_bound"].includes(status)) {
        setIdentity("");
        setNotice(tr("The reply does not match this selection. Nothing is confirmed; preview again."));
        return;
      }
      setIdentity(String(result.identity));
      setNotice(`${tr("Verified native title: %1", String(result.native_title ?? ""))}\n${status === "already_bound"
        ? tr("This exact conversation is already bound; its run identity will be preserved.")
        : tr("Ready to create one local Zerus binding.")}`);
    } catch (error) {
      setNotice(String(error));
      if (commit) setIdentity("");
    } finally {
      setBusy(false);
    }
  };
  return <dialog ref={dialog} class="dlg" style={{ width: "640px" }} aria-labelledby="oc-bind-title"
    onCancel={(e) => { e.preventDefault(); if (!busy()) props.close(); }} onClose={() => props.close()}>
    <form onSubmit={(e) => { e.preventDefault(); void submit(true); }}>
      <div class="dlg-head"><Icon name="link" size={18} class="accent" /><h2 id="oc-bind-title">{tr("Bind an existing OpenCode conversation")}</h2>
        <span class="grow" /><button type="button" class="icon-btn" aria-label={tr("Close")} disabled={busy()} onClick={props.close}><Icon name="x" size={16} /></button></div>
      <div class="dlg-body">
        <p>{tr("Choose one native conversation. Preview verifies its exact ID, directory and profile. Binding adds it to Zerus without creating a native conversation, changing its history or sending a message.")}</p>
        <p class="muted">{tr("Machine: %1\nNative profile: %2", props.machine || tr("This computer"), props.agentHome)}</p>
        <div class="form-row"><label for="oc-native-id">{tr("Native conversation ID")}</label><TextInput id="oc-native-id" class="field" placeholder="ses_..." value={nativeId()} onInput={(e) => { setNativeId(e.currentTarget.value); setIdentity(""); }} /></div>
        <div class="form-row"><label for="oc-directory">{tr("Native directory")}</label><TextInput id="oc-directory" class="field" value={directory()} onInput={(e) => { setDirectory(e.currentTarget.value); setIdentity(""); }} placeholder={tr("Exact absolute directory inside the selected environment")} /></div>
        <div class="form-row"><label for="oc-name">{tr("Zerus session name")}</label><TextInput id="oc-name" class="field" value={name()} onInput={(e) => { setName(e.currentTarget.value); setIdentity(""); }} placeholder="opencode/project/session" /></div>
        <Show when={notice()}><p class="muted" style={{ "white-space": "pre-wrap" }}>{notice()}</p></Show>
      </div>
      <div class="dlg-foot"><span class="hint" /><span class="spacer" />
        <button type="button" class="btn" disabled={busy()} onClick={props.close}>{tr("Cancel")}</button>
        <button type="button" class="btn" disabled={busy() || !filled()} onClick={() => void submit(false)}>{busy() ? tr("Verifying…") : tr("Preview")}</button>
        <button type="submit" class="btn primary" disabled={busy() || !filled() || !identity()}>{busy() ? tr("Binding…") : tr("Bind conversation")}</button>
      </div>
    </form>
  </dialog>;
}
