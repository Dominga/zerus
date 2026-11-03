import { For, Show, createMemo, createResource, onCleanup, onMount } from "solid-js";
import { desktopService, retainedSourceDetails } from "../../store/desktopService";
import { loadQtPreferences, qtPreferenceRows } from "../../store/qtPreferences";
import { tr } from "../../i18n";

/** This dialog reads the already published native status and has no retry seam. */
export function MigrationReportDialog(props: { origin: HTMLElement; close(): void }) {
  const origin = props.origin;
  const details = createMemo(() => retainedSourceDetails(desktopService.state()));
  // The one-time Qt preference import, read locally from Settings: key names only.
  const [preferences] = createResource(() => loadQtPreferences().catch(() => undefined));
  const preferenceRows = createMemo(() => qtPreferenceRows(preferences()));
  let dialog!: HTMLDialogElement, closeButton!: HTMLButtonElement;
  onMount(() => { dialog.showModal(); closeButton.focus(); });
  onCleanup(() => {
    if (dialog.open) dialog.close();
    queueMicrotask(() => { if (origin.isConnected) origin.focus(); });
  });
  return <dialog ref={dialog} class="dlg" style={{ width: "600px" }} aria-labelledby="migration-report-title" onCancel={props.close} onClose={props.close}>
    <div class="dlg-head"><h2 id="migration-report-title">{tr("Migration report")}</h2></div>
    <div class="dlg-body">
      <Show when={details() || preferenceRows().length} fallback={<p>{tr("No migration report is available for this connection.")}</p>}>
        <For each={["settings", "creations", "capture", "localReads", "localReadsReport", "drafts", "worktreeCache"] as const}>{(key) => <Show when={details()?.[key]}>{(value) => <section>
          <h3>{key === "settings" ? tr("Qt and Zerus settings") : key === "creations" ? tr("Saved creation report") : key === "capture" ? tr("Session creation journal") : key === "localReads" ? tr("Local read marks") : key === "drafts" ? tr("Drafts") : key === "worktreeCache" ? tr("Saved folder catalogs") : tr("Historical local read-mark report")}</h3>
          <p>{value().reason}</p>
          <p class="hint">{value().next}</p>
          <details><summary>{tr("Technical details")}</summary>
            <dl><dt>{tr("Reason code")}</dt><dd><code>{value().code || tr("None")}</code></dd>
              <dt>{tr("Report status")}</dt><dd>{value().phase ?? tr("Not created yet")}</dd></dl>
          </details>
        </section>}</Show>}</For>
        <Show when={preferenceRows().length}>
          <section>
            <h3>{tr("Qt preferences")}</h3>
            <p class="hint">{tr("Read once from the Qt app. Zerus never changes the Qt settings, and this list shows setting names, not their values.")}</p>
            <dl><For each={preferenceRows()}>{(row) => <>
              <dt>{row.label}</dt>
              <dd>{row.status}: <For each={row.keys}>{(key, index) => <>{index() > 0 ? ", " : ""}<code>{key}</code></>}</For>
                <Show when={row.reason}>{(reason) => <> ({tr("reason")}: <code>{reason()}</code>)</>}</Show></dd>
            </>}</For></dl>
          </section>
        </Show>
      </Show>
    </div>
    <div class="dlg-foot"><span class="spacer" /><button ref={closeButton} type="button" class="btn primary" onClick={props.close}>{tr("Close")}</button></div>
  </dialog>;
}
