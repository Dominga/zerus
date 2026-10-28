import { Show, createSignal, onCleanup, onMount } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { inTauri } from "../../../bridge";
import { Icon } from "../../lib/icons";
import { Group, NumberField, Row, SectionHead, Select, Status } from "./ui";
import { GIB, formatDataSize, selectedLimit } from "./files-size";

// Settings > Files (tray/src/DownloadCacheSettings.cpp) as the approved
// mockup lays it out (design/webui/src/screens/settings/General.tsx). The
// core's download_cache counts and trims the folder on its own worker; the
// folder is shared with the Qt client.

interface FilesView { folder: string; limit: number; bytes: number; files: number; error: string | null }

/** Re-read this long after the opening grace ends (DownloadCache::OpeningGraceMs + 5 s). */
const REFRESH_MS = 65_000;

export function FilesSection() {
  const [view, setView] = createSignal<FilesView | undefined>();
  const [mode, setMode] = createSignal("never");
  const [size, setSize] = createSignal(1);
  const [checking, setChecking] = createSignal(false);
  const [confirming, setConfirming] = createSignal<number | undefined>();
  let started = false;

  const show = (next: FilesView) => {
    setView(next);
    // The controls start from the stored limit once.
    if (!started) {
      started = true;
      setMode(next.limit > 0 ? "limit" : "never");
      setSize(next.limit > 0 ? next.limit / GIB : 1);
    }
  };
  const refresh = () => {
    if (!inTauri() || checking()) return;
    setChecking(true);
    invoke<FilesView>("files_usage").then(show, () => {}).finally(() => setChecking(false));
  };
  const apply = (bytes: number) => {
    setChecking(true);
    invoke<FilesView>("files_apply_limit", { bytes }).then(show, () => {}).finally(() => setChecking(false));
  };
  const selected = () => selectedLimit(mode(), size());
  const changed = () => !!view() && selected() !== view()!.limit;
  const onApply = () => {
    const current = view()?.limit ?? 0;
    // A new or smaller limit deletes copies: Qt asks first.
    if (selected() > 0 && (!current || selected() < current)) setConfirming(selected());
    else apply(selected());
  };

  onMount(() => {
    refresh();
    const timer = setInterval(() => { if ((view()?.limit ?? 0) > 0) refresh(); }, REFRESH_MS);
    onCleanup(() => clearInterval(timer));
  });

  const usageText = () => {
    const v = view();
    if (!v || checking()) return undefined;
    return v.error ?? `Downloaded copies: ${formatDataSize(v.bytes)} in ${v.files} files`;
  };
  return (
    <>
      <SectionHead title="Files" intro="Download and open reuses a verified saved copy from the same machine and path. Show file selects it in your file manager. Download again gets a fresh copy; Download… lets you choose where to save. Existing files are preserved." />
      <Group>
        <Row label="Downloaded files" for="set-dl-folder">
          <input id="set-dl-folder" class="field mono set-path" readOnly value={view()?.folder ?? ""} />
          <button type="button" class="icon-btn" aria-label="Open folder" disabled={!view()} onClick={() => void invoke("files_open_folder").catch(() => {})}>
            <Icon name="folder" size={15} />
          </button>
        </Row>
        <Row label="Keep copies" for="set-dl-mode">
          <Select id="set-dl-mode" value={mode()} onChange={setMode}
            options={[{ value: "never", label: "Never clear" }, { value: "limit", label: "Limit size" }]} />
        </Row>
        <Row label="Maximum size" disabled={mode() !== "limit"}>
          <NumberField value={size()} min={0.1} max={1024} step={0.5} decimals={1} suffix="GiB" label="Maximum size"
            disabled={mode() !== "limit"} onChange={setSize} />
        </Row>
        <p class="set-group-hint">Never clear is the default. With a limit, Zerus removes its least recently used downloaded copies first. Files you add or edit are kept. Downloads in progress and copies used in the last minute are protected, so usage can briefly exceed the limit. This setting applies to this computer.</p>
        <div class="set-actions">
          <button type="button" class="btn primary" disabled={!changed() || checking()} onClick={onApply}>Apply</button>
          <Show when={checking()} fallback={<Show when={usageText()}>{(text) => <Status kind={view()?.error ? "error" : "muted"}>{text()}</Status>}</Show>}>
            <Status kind="busy">Checking downloaded files…</Status>
          </Show>
        </div>
      </Group>
      <Show when={confirming()}>
        {(bytes) => <LimitQuestion bytes={bytes()} answer={(yes) => { setConfirming(undefined); if (yes) apply(bytes()); }} />}
      </Show>
    </>
  );
}

/** Limit downloaded files? Apply limit, or Cancel (the default). */
function LimitQuestion(props: { bytes: number; answer: (yes: boolean) => void }) {
  let dialog: HTMLDialogElement | undefined;
  const answer = (yes: boolean) => { if (dialog?.open) dialog.close(); props.answer(yes); };
  onCleanup(() => { if (dialog?.open) dialog.close(); });
  return (
    <dialog class="dlg" ref={(el) => { dialog = el; queueMicrotask(() => el.showModal()); }} aria-labelledby="files-limit-title"
      onCancel={(e) => { e.preventDefault(); answer(false); }}>
      <div class="dlg-head"><h2 id="files-limit-title">Limit downloaded files?</h2></div>
      <div class="dlg-body">{`Zerus will automatically delete its oldest downloaded copies when they exceed ${formatDataSize(props.bytes)}. Files you added or edited will be kept.`}</div>
      <div class="dlg-foot">
        <span class="spacer" />
        <button type="button" class="btn" onClick={() => answer(true)}>Apply limit</button>
        <button type="button" class="btn primary" autofocus onClick={() => answer(false)}>Cancel</button>
      </div>
    </dialog>
  );
}
