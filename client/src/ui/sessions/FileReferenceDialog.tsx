import { Show, createSignal, onMount } from "solid-js";
import { tr } from "../../i18n";
import * as bridge from "../../bridge";
import { pathProblem, remotePath } from "./file-reference";

// Qt parity (I5a): MessageComposer::referenceFile's dialog. Only the path
// goes into the draft; the file is never read or uploaded. A local path is
// checked on this computer (src-tauri composer/reference.rs) and may be
// chosen with Browse…; a remote one is checked for its form only.

export interface ReferenceTarget {
  /** The composer the dialog opened for: a reference reaches only that one. */
  handle: number;
  local: boolean;
  /** The computer as people know it ("this computer" for a local session). */
  machine: string;
  /** The remote computer's alias. */
  host: string;
}

/**
 * `insert` puts the checked path into the composer: "" when it did (or the
 * composer changed meanwhile, which drops the reference), otherwise why not,
 * which the dialog shows.
 */
export function FileReferenceDialog(props: { target: ReferenceTarget; insert: (path: string) => string; onClose: () => void }) {
  let dialog!: HTMLDialogElement;
  let field!: HTMLInputElement;
  const [path, setPath] = createSignal("");
  const [error, setError] = createSignal("");
  const [checking, setChecking] = createSignal(false);
  onMount(() => { dialog.showModal(); field.focus(); });
  const accept = async () => {
    if (checking()) return;
    let value = path();
    const problem = pathProblem(value);
    if (problem) { setError(problem); return; }
    if (props.target.local) {
      setChecking(true);
      let found: string | null = null;
      try { found = await bridge.referenceLocalFile(value); } catch { found = null; } finally { setChecking(false); }
      if (!found) { setError(tr("Choose an existing file on this computer.")); return; }
      value = found;
    } else if (!remotePath(value)) {
      setError(tr("Enter an absolute path or ~/ path on %1.", props.target.machine));
      return;
    }
    const refused = props.insert(value);
    if (refused) { setError(refused); return; }
    dialog.close();
  };
  const browse = async () => {
    let picked: string | null = null;
    try { picked = await bridge.pickReferenceFile(); } catch { /* outside the app */ }
    if (picked) { setPath(picked); setError(""); }
    field.focus();
  };
  return (
    <dialog ref={dialog} class="dlg file-ref-dlg" aria-labelledby="file-ref-title" aria-describedby="file-ref-hint" style={{ width: "520px" }} data-shortcut-context="none"
      onCancel={(e) => { e.preventDefault(); dialog.close(); }} onClose={() => props.onClose()}>
      <div class="dlg-head"><h2 id="file-ref-title" class="ell">{tr("Reference file: %1", props.target.machine)}</h2></div>
      <div class="dlg-body file-ref-body">
        <p id="file-ref-hint" class="confirm-body">{props.target.local
          ? tr("Insert a path to an existing file on this computer. Only the path is added; the file is not read or uploaded.")
          : tr("Enter the path of an existing file on %1 (%2), not on this computer. Only the path is added; its existence is not checked here.", props.target.machine, props.target.host)}</p>
        <div class="file-ref-row">
          <input ref={field} class="field mono" type="text" spellcheck={false} autocomplete="off" value={path()}
            aria-label={tr("File path on %1", props.target.machine)} aria-invalid={!!error()}
            placeholder={props.target.local ? tr("Absolute file path") : tr("/path/to/file, ~/file or C:\\path\\file")}
            onInput={(e) => { setPath(e.currentTarget.value); setError(""); }}
            onKeyDown={(e) => { if (e.key === "Enter" && !e.isComposing) { e.preventDefault(); void accept(); } }} />
          <Show when={props.target.local}><button type="button" class="btn" onClick={() => void browse()}>{tr("Browse…")}</button></Show>
        </div>
        <Show when={error()}><div class="hint danger" role="alert">{error()}</div></Show>
      </div>
      <div class="dlg-foot">
        <span class="spacer" />
        <button type="button" class="btn" onClick={() => dialog.close()}>{tr("Cancel")}</button>
        <button type="button" class="btn primary" disabled={checking()} onClick={() => void accept()}>{tr("Insert reference")}</button>
      </div>
    </dialog>
  );
}
