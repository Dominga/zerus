import { createEffect, Show } from "solid-js";
import { toast } from "../../store/ui";
import { actOnSessionFile, closeSessionFile, downloadSessionFile, saveSessionFile, sessionFile } from "./sessionFiles";
import { Icon } from "../lib/icons";

export function SessionFileDialog() {
  let dialog: HTMLDialogElement | undefined;
  createEffect(() => {
    if (sessionFile() && dialog && !dialog.open) dialog.showModal();
  });
  const copyPath = () => {
    const path = sessionFile()?.path;
    if (path) void navigator.clipboard?.writeText(path).then(() => toast("File reference", "Path copied"), () => toast("File reference", "The clipboard is unavailable"));
  };
  return (
    <Show when={sessionFile()}>
      {(file) => (
        <dialog class="dlg session-file" ref={dialog} aria-labelledby="session-file-title"
          onCancel={(event) => { event.preventDefault(); closeSessionFile(); }} onClose={closeSessionFile}>
          <div class="dlg-head"><Icon name="file" size={18} class="accent" /><h2 id="session-file-title">File reference</h2><span class="grow" /><button class="icon-btn" aria-label="Close" onClick={closeSessionFile}><Icon name="x" size={16} /></button></div>
          <div class="dlg-body">
            <div class="dlg-detail">{file().title}</div>
            <div class="dlg-detail session-file-path">{file().path}</div>
            <Show when={file().location}><div class="dlg-detail">Source position: {file().location}</div></Show>
            <Show when={file().checking}><div class="dlg-detail">Checking the file…</div></Show>
            <Show when={file().downloading}><div class="dlg-detail">Downloading…</div></Show>
            <Show when={file().saved && !file().downloading}><div class="dlg-detail">Saved.</div></Show>
            <Show when={file().notice}><div class="dlg-detail">{file().notice}</div></Show>
            <Show when={file().error}><div class="dlg-error">{file().error}</div></Show>
            <div class="set-actions">
              <Show when={file().local_path && !file().downloading}>
                <button class="btn primary" data-initial onClick={() => actOnSessionFile("open_file")}>{file().remote ? "Open local copy" : "Open file"}</button>
                <Show when={!file().remote}><button class="btn" onClick={() => actOnSessionFile("open_folder")}>Open folder</button></Show>
                <Show when={file().remote}><button class="btn" onClick={() => actOnSessionFile("show_file")}>Show file</button></Show>
              </Show>
              <Show when={file().remote && !file().local_path && !file().downloading}>
                <button class="btn primary" data-initial onClick={() => downloadSessionFile(true)}>Download and open</button>
                <button class="btn" onClick={() => downloadSessionFile(false)}>Download…</button>
              </Show>
              <Show when={file().remote && file().local_path && !file().downloading}>
                <button class="btn" onClick={saveSessionFile}>Save local copy…</button>
              </Show>
              <Show when={file().copy_path}>
                <button class="btn" onClick={() => actOnSessionFile("open_copy")}>Open local copy</button>
              </Show>
              <button class="icon-btn" aria-label="Copy path" title="Copy path" onClick={copyPath}><Icon name="copy" size={14} /></button>
              <Show when={file().remote}><button class="btn" onClick={() => actOnSessionFile("open_ssh")}><Icon name="square-terminal" size={13} />Open SSH terminal</button></Show>
            </div>
          </div>
          <div class="dlg-foot"><span class="spacer" /><button class="btn" onClick={closeSessionFile}>Close</button></div>
        </dialog>
      )}
    </Show>
  );
}
