import { onMount, onCleanup, Show } from "solid-js";
import { copyVerbatim } from "../../bridge";
import { toast } from "../../store/ui";
import { actOnSessionFile, closeSessionFile, downloadSessionFile, saveSessionFile, sessionFile } from "./sessionFiles";
import { Icon } from "../lib/icons";

export function SessionFileDialog() {
  const copyPath = () => {
    const path = sessionFile()?.path;
    if (path) void copyVerbatim(path).then(() => toast("File reference", "Path copied"), () => toast("File reference", "The clipboard is unavailable"));
  };
  return (
    <Show when={sessionFile()}>
      {(file) => {
        let dialog: HTMLDialogElement | undefined;
        onMount(() => { dialog?.showModal(); queueMicrotask(() => dialog?.querySelector<HTMLElement>("[data-initial]:not(:disabled)")?.focus()); });
        const id = file().id;
        const previous = document.activeElement;
        onCleanup(() => { dialog?.close(); if (previous instanceof HTMLElement && previous.isConnected) previous.focus(); });
        return (
        <dialog class="dlg session-file" ref={dialog} aria-labelledby="session-file-title"
          onCancel={(event) => { event.preventDefault(); closeSessionFile(); }} onClose={() => { if (sessionFile()?.id === id) closeSessionFile(); }}>
          <div class="dlg-head"><Icon name="file-text" size={18} class="accent" /><h2 id="session-file-title">File reference</h2><span class="grow" /><button class="icon-btn" aria-label="Close" onClick={closeSessionFile}><Icon name="x" size={16} /></button></div>
          <div class="dlg-body">
            <div class="dlg-detail">{file().title}</div>
            <div class="dlg-detail session-file-path">{file().path}</div>
            <Show when={file().location}><div class="dlg-detail">Source position: {file().location}</div></Show>
            <Show when={file().checking || file().hint}><div class="dlg-detail session-file-hint">{file().checking ? "Checking the downloaded copy…" : file().hint}</div></Show>
            <Show when={file().checking || file().downloading}><progress aria-label={file().downloading ? `Downloading file from ${file().computer}` : "Checking the downloaded copy…"} /></Show>
            <Show when={file().error}><div class="dlg-error">{file().error}</div></Show>
            <div class="set-actions">
              <Show when={!file().remote && file().local_available}>
                <button class="btn primary" data-initial onClick={() => actOnSessionFile("open_file")}>Open file</button>
                <button class="btn" onClick={() => actOnSessionFile("open_folder")}>Open folder</button>
              </Show>
              <Show when={file().remote}>
                <button class="btn" disabled={!file().can_download || file().downloading || file().picking} onClick={saveSessionFile}>Download…</button>
                <button class={file().local_available ? "btn" : "btn primary"} data-initial={!file().local_available || undefined}
                  disabled={!file().can_download || file().checking || file().downloading || file().picking} onClick={() => downloadSessionFile(true)}>
                  {file().local_available ? "Download again" : "Download and open"}
                </button>
                <Show when={file().local_available && !file().downloading}>
                  <button class="btn primary" data-initial disabled={file().picking} onClick={() => actOnSessionFile("open_copy")}>Open local copy</button>
                  <button class="btn" disabled={file().picking} onClick={() => actOnSessionFile("show_file")}>Show file</button>
                </Show>
              </Show>
              <button class="icon-btn" aria-label="Copy path" title="Copy path" onClick={copyPath}><Icon name="copy" size={14} /></button>
              <Show when={file().remote}><button class="btn" onClick={() => actOnSessionFile("open_ssh")}><Icon name="square-terminal" size={13} />Open SSH terminal</button></Show>
            </div>
          </div>
          <div class="dlg-foot"><span class="spacer" /><button class="btn" onClick={closeSessionFile}>Close</button></div>
        </dialog>
      ); }}
    </Show>
  );
}
