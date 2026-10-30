import { createEffect, Show } from "solid-js";
import { toast } from "../../store/ui";
import { actOnSessionFile, closeSessionFile, downloadSessionFile, saveSessionFile, sessionFile } from "./sessionFiles";

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
          <div class="dlg-head"><h2 id="session-file-title">File reference</h2></div>
          <div class="dlg-body">
            <div class="dlg-detail">{file().title}</div>
            <div class="dlg-detail wrap-pre">{file().path}{file().location}</div>
            <Show when={file().checking}><div class="dlg-detail">Checking the file…</div></Show>
            <Show when={file().downloading}><div class="dlg-detail">Downloading…</div></Show>
            <Show when={file().saved && !file().downloading}><div class="dlg-detail">Saved.</div></Show>
            <Show when={file().notice}><div class="dlg-detail">{file().notice}</div></Show>
            <Show when={file().error}><div class="dlg-error">{file().error}</div></Show>
            <div class="set-actions">
              <Show when={file().local_path && !file().downloading}>
                <button class="btn primary" data-initial onClick={() => actOnSessionFile("open_file")}>Open →</button>
                <button class="btn" onClick={() => actOnSessionFile("open_folder")}>Open folder</button>
                <button class="btn" onClick={() => actOnSessionFile("show_file")}>Show in file manager</button>
              </Show>
              <Show when={file().remote && !file().local_path && !file().downloading}>
                <button class="btn primary" data-initial onClick={() => downloadSessionFile(true)}>Download and open</button>
                <button class="btn" onClick={() => downloadSessionFile(false)}>Download</button>
              </Show>
              <Show when={file().remote && file().local_path && !file().downloading}>
                <button class="btn" onClick={saveSessionFile}>Save local copy…</button>
              </Show>
              <Show when={file().copy_path}>
                <button class="btn" onClick={() => actOnSessionFile("open_copy")}>Open saved copy</button>
              </Show>
              <button class="btn" onClick={copyPath}>Copy path</button>
              <button class="btn" disabled title="Arrives with Open terminal.">Open in Terminal</button>
            </div>
          </div>
          <div class="dlg-foot"><span class="spacer" /><button class="btn" onClick={closeSessionFile}>Close</button></div>
        </dialog>
      )}
    </Show>
  );
}
