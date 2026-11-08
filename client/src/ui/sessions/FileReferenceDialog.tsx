import { Show, createSignal, onCleanup, onMount } from "solid-js";
import { Icon } from "../lib/icons";
import { TextInput } from "../lib/TextField";
import { fileReferenceCancel, fileReferenceCheck, fileReferenceCheckRemote, fileReferencePick, fileReferenceRemoteSupport } from "../../bridge";
import { tr } from "../../i18n";
import { referenceSentence, remoteForm, wellFormed } from "./file-reference";
import type { Style } from "./path-forms";

// Reference an existing file (Qt's referenceFile dialog, MessageComposer.cpp
// 865-959; zerus-5wmh.13.5). A local path is checked once, off the UI thread,
// for an existing regular file. A remote one is checked for its form, and, when
// its computer's hgs has `path-info` (zerus-5wmh.13.18), for an existing file
// there; an older computer keeps Qt's unchecked insert. The dialog hands the
// sentence to the composer, which inserts it only while the draft it opened on
// is unchanged.

/** The session the dialog references a file for, as the composer saw it when it opened. */
export interface ReferenceTarget {
  /** This computer, or the session's remote computer (`host`). */
  local: boolean;
  machine: string;
  host: string;
  /** The remote computer's path style; undefined before it reports its platform. */
  style: Style | undefined;
}

// Rust remembers recently closed dialog numbers (zerus-a0imb): a reloaded
// webview must not reuse one, so they start at a random point.
let nextDialog = 1 + Math.floor(Math.random() * 2 ** 40);

export function FileReferenceDialog(props: {
  target: ReferenceTarget;
  /** Inserts the sentence; returns why it cannot (the message limit), or undefined once inserted. */
  insert: (sentence: string) => string | undefined;
  close: () => void;
}) {
  let dialog!: HTMLDialogElement;
  let field: HTMLInputElement | undefined;
  const id = nextDialog++;
  const [path, setPath] = createSignal("");
  const [error, setError] = createSignal("");
  const [checking, setChecking] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  /** Whether the remote computer checks paths; undefined until its hgs answered. */
  const [checks, setChecks] = createSignal<boolean>();
  let attempt = 0;
  let gone = false;
  onCleanup(() => {
    gone = true; attempt++;
    if (!props.target.local) void fileReferenceCancel(id).catch(() => {});
  });
  onMount(() => {
    dialog.showModal();
    queueMicrotask(() => field?.focus());
    if (!props.target.local) {
      void fileReferenceRemoteSupport(id, props.target.host).then((known) => { if (!gone && known !== null) setChecks(known); }).catch(() => {});
    }
  });

  // A changed path makes a pending check's answer obsolete: it is dropped,
  // and Insert checks the new path (zerus-thzuz).
  const edited = (value: string) => {
    setPath(value);
    setError("");
    setBusy(false);
    if (checking()) { attempt++; setChecking(false); }
  };

  const machine = () => props.target.local ? tr("this computer") : props.target.machine || props.target.host;

  const finish = (value: string) => {
    const refused = props.insert(referenceSentence(value));
    if (refused) setError(refused);
    else props.close();
  };

  const accept = () => {
    if (checking()) return;
    const value = path();
    if (!wellFormed(value)) { setError(tr("Enter a file path without line breaks or control characters.")); return; }
    if (!props.target.local) {
      if (!remoteForm(value, props.target.style)) { setError(tr("Enter an absolute path or ~/ path on %1.", machine())); return; }
      checkRemote(value);
      return;
    }
    const mine = ++attempt;
    setChecking(true);
    setError("");
    void fileReferenceCheck(id, value).then((answer) => {
      // A late answer for a closed dialog, or an earlier attempt, is dropped.
      if (gone || mine !== attempt) return;
      setChecking(false);
      if (answer.status === "file") { setBusy(false); finish(answer.path); }
      else if (answer.status === "busy") { setBusy(true); setError(tr("Cannot check this path now.")); }
      else { setBusy(false); setError(tr("Choose an existing file on this computer.")); }
    }).catch((failure) => {
      if (gone || mine !== attempt) return;
      setChecking(false);
      setError(String(failure));
    });
  };

  const checkRemote = (value: string) => {
    const mine = ++attempt;
    setChecking(true);
    setError("");
    void fileReferenceCheckRemote(id, props.target.host, value).then((answer) => {
      // A late answer for a closed dialog, or an earlier attempt, is dropped.
      if (gone || mine !== attempt) return;
      setChecking(false);
      setBusy(answer.status === "busy");
      if (answer.status !== "unchecked" && answer.status !== "busy" && answer.status !== "superseded") setChecks(true);
      if (answer.status === "file" || answer.status === "unchecked") finish(value);
      else if (answer.status === "missing") setError(tr("Choose an existing file on %1.", machine()));
      else if (answer.status === "unreadable") setError(tr("This path cannot be inspected on %1.", machine()));
      else if (answer.status === "busy") setError(tr("Cannot check this path now."));
    }).catch((failure) => {
      if (gone || mine !== attempt) return;
      setChecking(false);
      setError(String(failure));
    });
  };

  const browse = () => {
    void fileReferencePick().then((picked) => { if (!gone && picked) edited(picked); }).catch(() => {});
  };

  return (
    <dialog class="dlg file-reference" ref={dialog} aria-labelledby="file-reference-title" aria-describedby="file-reference-hint" style={{ width: "520px" }}
      onCancel={(e) => { e.preventDefault(); props.close(); }} onClose={() => { if (!gone) props.close(); }}>
      <div class="dlg-head"><Icon name="file-text" size={18} class="accent" /><h2 id="file-reference-title">{tr("Reference file — %1", machine())}</h2></div>
      <div class="dlg-body">
        <p class="confirm-body" id="file-reference-hint">{props.target.local
          ? tr("Insert a path to an existing file on this computer. Only the path is added; the file is not read or uploaded.")
          : checks()
            ? tr("Enter the path of an existing file on %1 (%2), not on this computer. Only the path is added; the file is not read or uploaded.", machine(), props.target.host)
            : tr("Enter the path of an existing file on %1 (%2), not on this computer. Only the path is added; its existence is not checked here.", machine(), props.target.host)}</p>
        <div class="file-reference-row">
          <TextInput ref={field} class="field grow" value={path()} aria-label={tr("File path on %1", machine())}
            placeholder={props.target.local ? tr("Absolute file path") : tr("/path/to/file, ~/file or C:\\path\\file")}
            onInput={(e) => edited(e.currentTarget.value)}
            onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); accept(); } }} />
          <Show when={props.target.local}><button type="button" class="btn" onClick={browse}>{tr("Browse…")}</button></Show>
        </div>
        <Show when={error()}><p class="dlg-detail error" role="alert">{error()}</p></Show>
      </div>
      <div class="dlg-foot">
        <span class="spacer" />
        <button type="button" class="btn" onClick={() => props.close()}>{tr("Cancel")}</button>
        <button type="button" class="btn primary" disabled={checking()} onClick={accept}>{busy() ? tr("Retry") : tr("Insert reference")}</button>
      </div>
    </dialog>
  );
}
