import { catalog } from "../../store/catalog";
import { setPage, setSelectedKey, setDetailTab } from "../../store/ui";
import { For, Show, createEffect, createMemo, createResource, createSignal, onCleanup, onMount } from "solid-js";
import { MachineLabel } from "../components/MachineLabel";
import { Icon } from "../lib/icons";
import { terminationCleanupInspect } from "../../bridge";
import { cleanup, cleanupDialog, closeCleanup, openCleanup, keepCleanup, checkCleanup, removeCleanup, emptyConsents, type CleanupRow } from "../../store/terminationCleanup";

/** Approved statusbar placement; all-removed jobs use the existing toast bus. */
export function CleanupChip() {
  return <Show when={cleanup().rows.length}><button type="button" class="chip-accent" style={{ "white-space": "pre" }} classList={{ "warn-chip": cleanup().kept > 0 || cleanup().unknown > 0 }} title={cleanup().title} aria-label={cleanup().title} onClick={openCleanup}>{cleanup().text}</button></Show>;
}
const labels = { removed: "Removed", kept: "Kept", unknown: "Unknown", ready: "Ready", checking: "Checking…", removing: "Checking again and removing…" };
/** setup-batch-cleanup, Terminate variant. Opening displays recorded outcomes. */
export default function WorktreeCleanup() {
  let dialog!: HTMLDialogElement;
  const origin = cleanupDialog()?.origin;
  const [selected, setSelected] = createSignal(cleanup().rows[0]?.id);
  const [details, setDetails] = createSignal(false);
  const [consents, setConsents] = createSignal(emptyConsents());
  const [inspect, setInspect] = createSignal<CleanupRow>();
  const current = () => cleanup().rows.find((r) => r.id === selected()) ?? cleanup().rows[0];
  const targets = createMemo(() => cleanup().rows.filter((r) => r.ready));
  const consented = () => (!cleanup().required.local_files || consents().local_files) && (!cleanup().required.discard_changes || consents().discard_changes) && (!cleanup().required.unchecked_processes || consents().unchecked_processes);
  const summary = () => `${cleanup().removed} removed  ${cleanup().kept} kept  ${cleanup().unknown} unknown`;
  createEffect(() => { cleanup().revision; setConsents(emptyConsents()); });
  onMount(() => { dialog.showModal(); queueMicrotask(() => dialog.querySelector<HTMLElement>("[autofocus], .dlg-body select")?.focus()); });
  onCleanup(() => { dialog.close(); queueMicrotask(() => { if (origin?.isConnected) origin.focus(); }); });
  return <>
    <dialog ref={dialog} class="dlg su-dlg worktree-cleanup" style={{ width: "780px" }} aria-labelledby="cleanup-title" onCancel={closeCleanup} onClose={closeCleanup}>
      <div class="dlg-head"><Icon name="trash" size={18} class="accent" /><h2 id="cleanup-title">Remove working folders</h2><span class="grow" /><button class="icon-btn" aria-label="Close" onClick={closeCleanup}><Icon name="x" size={16} /></button></div>
      <div class="dlg-body bc-body">
        <h3 class="section-title">Review {cleanup().rows.length} selected worktrees</h3>
        <p class="muted">Conversations and branches are kept. Removing a working folder permanently deletes its local files.</p>
        <table class="su-table bc-table"><thead><tr><th>Folder / branch</th><th>Computer</th><th>Size</th><th>Result</th></tr></thead><tbody><For each={cleanup().rows}>{(row) =>
          <tr classList={{ on: current()?.id === row.id }} onClick={() => setSelected(row.id)} title={row.path}>
            <td><div class="ell"><b>{row.label}</b></div><div class="mono dim ell">{row.branch}</div></td><td><MachineLabel alias={row.machine} class="mbadge" /></td><td class="tabular">{row.size}</td>
            <td title={row.reason}><div class="bc-result" data-outcome={row.status}><Show when={row.status === "checking" || row.status === "removing"}><Icon name="loader-circle" size={12} class="su-spin" /></Show><b>{labels[row.status]}</b><Show when={row.status !== "removed"}><span class="hint ell wtc-reason">{row.reason}</span></Show></div></td>
          </tr>
        }</For></tbody></table>
        <div class="row">
          <button class="btn small" disabled={cleanup().busy || !current()?.can_check} onClick={() => { const row = current(); if (row) void checkCleanup(row.id); }}>Check again</button>
          <button class="btn small" disabled={!current() || cleanup().busy} onClick={() => setInspect(current())}>View files and commits…</button>
          <button class="btn small" classList={{ on: details() }} aria-pressed={details()} onClick={() => setDetails(!details())}>Details</button><span class="grow" />
        </div>
        <Show when={details()}><pre class="bc-details mono">{current()?.details}</pre></Show>
        <div class="bc-consent">
          <Show when={targets().length && cleanup().required.local_files}><label class="check"><input type="checkbox" checked={consents().local_files} disabled={cleanup().busy} onChange={(e) => setConsents({ ...consents(), local_files: e.currentTarget.checked })} />Delete the local files outside Git too</label></Show>
          <Show when={targets().length && cleanup().required.discard_changes}><label class="check"><input type="checkbox" checked={consents().discard_changes} disabled={cleanup().busy} onChange={(e) => setConsents({ ...consents(), discard_changes: e.currentTarget.checked })} />Permanently discard uncommitted and untracked files</label></Show>
          <Show when={targets().length && cleanup().required.unchecked_processes}><label class="check" title="Accept this uncertainty for this folder for five minutes. Known programs and sessions using the folder still prevent removal."><input type="checkbox" checked={consents().unchecked_processes} disabled={cleanup().busy} onChange={(e) => setConsents({ ...consents(), unchecked_processes: e.currentTarget.checked })} />Remove even if some running programs cannot be checked</label></Show>
        </div>
        <Show when={cleanup().busy}><progress aria-label="Removing working folders" /></Show>
      </div>
      <div class="dlg-foot"><span class="bc-summary tabular" role="status">{summary()}<Show when={cleanup().unknown}><span class="warn bc-note">An unknown result will not be retried.</span></Show></span><span class="grow" /><button class="btn" autofocus disabled={cleanup().busy} onClick={() => void keepCleanup()}>Keep folders</button><Show when={targets().length}><button class="btn solid-danger" disabled={cleanup().busy || !consented()} onClick={() => void removeCleanup(consents())}>Remove {targets().length} folders</button></Show></div>
    </dialog>
    <Show when={inspect()}>{(row) => <CleanupInspection row={row()} close={() => setInspect(undefined)} />}</Show>
  </>;
}

/** The mockup's read-only setup-inspect surface; the CLI bounds every page. */
function CleanupInspection(props: { row: CleanupRow; close: () => void }) {
  let dialog!: HTMLDialogElement;
  const origin = document.activeElement;
  const [tab, setTab] = createSignal<"changes" | "commits" | "local" | "sessions">("changes");
  const [details, setDetails] = createSignal(false);
  const [mode, setMode] = createSignal("uncommitted");
  const [selectedFile, setSelectedFile] = createSignal(0);
  const [selectedSession, setSelectedSession] = createSignal<string>();
  const session = () => Object.values(catalog.rows).find((r) => r.computer === props.row.computer && r.name === selectedSession());
  const openSession = () => { const row = session(); if (!row) return; props.close(); closeCleanup(); setSelectedKey(row.key); setDetailTab("activity"); setPage("sessions"); };
  const [local, setLocal] = createSignal({ file: ".", offset: 0 });
  const [data, { refetch }] = createResource(() => terminationCleanupInspect(props.row.id));
  const [file] = createResource(local, (page) => terminationCleanupInspect(props.row.id, page.file, page.offset));
  const text = () => mode() === "base" ? data()?.comparison : data()?.changes;
  onMount(() => { dialog.showModal(); queueMicrotask(() => dialog.querySelector<HTMLElement>("[autofocus], .dlg-body select")?.focus()); });
  onCleanup(() => { dialog.close(); if (origin instanceof HTMLElement && origin.isConnected) origin.focus(); });
  return <dialog ref={dialog} class="dlg su-dlg" style={{ width: "min(900px, calc(100vw - 48px))", height: "min(680px, calc(100vh - 48px))" }} aria-labelledby="cleanup-inspect-title" onCancel={props.close} onClose={props.close}>
    <div class="dlg-head"><Icon name="eye" size={18} class="accent" /><h2 id="cleanup-inspect-title">Inspect working folder</h2><span class="grow" /><button class="icon-btn" aria-label="Close" onClick={props.close}><Icon name="x" size={16} /></button></div>
    <div class="dlg-body"><section class="su-insp" aria-label="Worktree details">
      <header class="ip-head"><h3 class="ell" title={props.row.path}>{props.row.label}</h3><button class="btn small" classList={{ on: details() }} aria-pressed={details()} onClick={() => setDetails(!details())}>Details</button><button class="icon-btn small" aria-label="Read files and commits" disabled={data.loading} onClick={() => void refetch()}><Icon name="refresh-cw" size={13} /></button></header>
      <p class="ip-brief parts"><span class="mono">{props.row.branch}</span><span>{props.row.reason}</span></p>
      <Show when={details()}><pre class="ip-summary mono">{props.row.details}</pre></Show>
      <div class="tabs ip-tabs" role="tablist"><For each={["changes", "commits", "local", "sessions"] as const}>{(name) => <button class="tab" role="tab" classList={{ on: tab() === name }} aria-selected={tab() === name} onClick={() => setTab(name)}>{name[0].toUpperCase() + name.slice(1)}</button>}</For></div>
      <div class="ip-body"><Show when={!data.loading} fallback={<div class="ip-state">Loading…</div>}><Show when={!data.error} fallback={<div class="ip-state">{String(data.error)}</div>}>
        <Show when={tab() === "changes"}><select class="field" aria-label="Changes mode" value={mode()} onChange={(e) => setMode(e.currentTarget.value)}><option value="uncommitted">Uncommitted changes</option><option value="base">Compare files with base</option></select><div class="ip-state">{data()?.files?.length ? `${data()?.files?.length} changed files${data()?.files_partial ? " (partial)" : ""}` : "No changed files."}</div><table class="su-table ip-files"><thead><tr><th>Changed file</th><th>State</th></tr></thead><tbody><For each={data()?.files}>{(f, index) => <tr classList={{ on: selectedFile() === index() }} onClick={() => setSelectedFile(index())} onDblClick={() => { setLocal({ file: f.path, offset: 0 }); setTab("local"); }}><td class="mono ell">{f.path}</td><td class="mono">{f.status}</td></tr>}</For></tbody></table><pre class="ip-diff mono">{text()?.text ?? text()?.error ?? ""}</pre></Show>
        <Show when={tab() === "commits"}><pre class="ip-diff mono grow">{data()?.commits?.text ?? data()?.commits?.error ?? "No additional commits."}</pre></Show>
        <Show when={tab() === "local"}><div class="notice warn"><Icon name="triangle-alert" size={14} /><span>Outside Git. Removing this folder deletes these files too.</span></div><div class="row"><button class="btn small" onClick={() => setLocal({ file: ".", offset: 0 })}>Browse all files</button><button class="btn small" disabled={file()?.file?.next_offset === undefined || file.loading} onClick={() => setLocal({ ...local(), offset: file()?.file?.next_offset ?? 0 })}>Next</button></div><Show when={!file.loading} fallback={<div class="ip-state">Loading…</div>}><Show when={!file.error} fallback={<div class="ip-state">{String(file.error)}</div>}><table class="su-table ip-files"><thead><tr><th>Local file / folder</th><th>Size</th></tr></thead><tbody><For each={file()?.file?.entries}>{(f) => <tr onDblClick={() => { if (!f.symlink) setLocal({ file: f.path, offset: 0 }); }}><td class="mono ell">{f.path}{f.directory ? "/" : ""}</td><td>—</td></tr>}</For></tbody></table><Show when={file()?.file?.text !== undefined || file()?.file?.binary}><pre class="ip-diff mono">{file()?.file?.binary ? "Binary file; text preview is unavailable." : file()?.file?.text}{file()?.file?.truncated ? "\nPreview is truncated." : ""}</pre></Show></Show></Show></Show>
        <Show when={tab() === "sessions"}><Show when={data()?.review?.sessions?.length} fallback={<div class="ip-state">No related sessions.</div>}><ul class="ip-sessions" role="listbox" aria-label="Related sessions"><For each={data()?.review?.sessions?.filter((s) => typeof s === "string")}>{(s) => <li role="option" aria-selected={selectedSession() === s} classList={{ on: selectedSession() === s }} onClick={() => setSelectedSession(s)} onDblClick={openSession}><Icon name="message-square" size={13} class="dim" />{s}</li>}</For></ul><button class="btn small" disabled={!session()} onClick={openSession}>Open session</button></Show></Show>
      </Show></Show></div><div class="ip-status hint parts"><span>{props.row.machine}</span><span>Local Git refs</span></div>
    </section></div>
  </dialog>;
}
