import { For, Show, createEffect, createSignal, onCleanup, onMount } from "solid-js";
import { Icon } from "../lib/icons";
import { selected } from "../../store/ui";
import { flag } from "../../store/settings";
import { socraticode, type SocratiCodeVerb } from "../../store/socraticode";
import { useNow } from "../../store/latest";
import { blankState, connectConfirm, controlsEnabled, detailText, nextAction, nextVerb, stateParts, stepConfirm } from "./socraticode-ui";
import { tr } from "../../i18n";
import "../styles/socraticode.css";

// WhiteWh fork: the SocratiCode card of the session inspector
// (tray/src/SocratiCodePanel.h). It shows whether the plugin, Qdrant and the
// index are there and whether the folder is watched, and offers the one next
// step: install the plugin, install the native Qdrant, or start: bring up
// Qdrant and Ollama, then index and watch the folder with a SocratiCode server
// Zerus keeps running. The native Qdrant is the fork's Settings > Extensions
// switch; without Docker, Connect agents points the folder's agents at it.
// Nothing is parsed here: the core's panel only needs wording.

interface Confirm {
  title: string;
  text: string;
  verb: SocratiCodeVerb;
  confirm: string;
}

/** Qt's 10-second poll while the runner works or the index is being built. */
const POLL_MS = 10_000;

export function SocratiCodePanel() {
  const now = useNow(30_000);
  const [confirm, setConfirm] = createSignal<Confirm>();
  const target = () => {
    const row = selected();
    return row && row.cwdPath ? { host: row.computer, path: row.cwdPath, native: flag("socraticode/nativeQdrant") === true } : undefined;
  };
  createEffect(() => {
    const context = target();
    if (!context) {
      socraticode.close();
      return;
    }
    socraticode.open(context.host, context.path, context.native);
  });
  onCleanup(() => socraticode.close());
  // Poll only while work runs, as Qt's QTimer; a status read restarts the wait.
  createEffect(() => {
    if (!socraticode.data()?.poll) return;
    const timer = setInterval(() => socraticode.read(), POLL_MS);
    onCleanup(() => clearInterval(timer));
  });

  const working = () => socraticode.busy() || socraticode.acting();
  const state = () => socraticode.data()?.state ?? blankState(socraticode.busy(), socraticode.error());
  const parts = () => stateParts(state(), now());
  const details = () => {
    const view = socraticode.data();
    const lines = (view?.details ?? []).map(detailText);
    const error = socraticode.error();
    if (error) lines.push(error);
    return lines;
  };
  const next = () => socraticode.data()?.action ?? null;
  const label = () => (next() ? nextAction(next()!) : undefined);
  const enabled = () => controlsEnabled(socraticode.data(), socraticode.busy(), socraticode.acting());

  const act = () => {
    const step = next();
    if (!step || !enabled()) return;
    const path = target()?.path ?? "";
    const ask = stepConfirm(step, path);
    if (ask) setConfirm({ ...ask, verb: nextVerb(step), confirm: nextVerb(step) === "install-qdrant" ? tr("Install Qdrant") : tr("Index and watch") });
    else void socraticode.act(nextVerb(step));
  };
  const connect = () => {
    const path = target()?.path;
    if (!path || !enabled()) return;
    setConfirm({ ...connectConfirm(path), verb: "connect", confirm: tr("Connect agents") });
  };
  const stop = () => {
    if (!enabled()) return;
    setConfirm({
      title: tr("Stop SocratiCode?"),
      text: tr("Zerus stops the SocratiCode server it started for this folder. Its index stays, and Watch starts it again."),
      verb: "stop",
      confirm: tr("Stop"),
    });
  };
  const answer = (yes: boolean) => {
    const copy = confirm();
    setConfirm(undefined);
    if (yes && copy) void socraticode.act(copy.verb);
  };

  return (
    <div class="sc-card">
      <div class="sc-head">
        <span class="sc-state" role="status" aria-label={parts().join(", ")}>
          <For each={parts()}>{(part, index) => <span class="sc-part" classList={{ lead: index() === 0 }}>{part}</span>}</For>
        </span>
        <button type="button" class="icon-btn small" title={tr("Refresh")} aria-label={tr("Refresh SocratiCode status")}
          disabled={working()} onClick={() => socraticode.read()}><Icon name="refresh" size={13} /></button>
      </div>
      <For each={details()}>{(line) => <p class="sc-detail">{line}</p>}</For>
      <div class="row sc-actions">
        <Show when={next() && label()}>
          <button type="button" class="btn small" disabled={!enabled()} title={label()!.tip} onClick={act}>{label()!.text}</button>
        </Show>
        <Show when={socraticode.data()?.connect}>
          <button type="button" class="btn small" disabled={!enabled()} title={tr("Let this folder's agents use the same Qdrant and Ollama (.claude/settings.local.json)")}
            onClick={connect}>{tr("Connect agents")}</button>
        </Show>
        <Show when={socraticode.data()?.stop}>
          <button type="button" class="btn small" disabled={!enabled()} title={tr("Stop the SocratiCode server that Zerus started for this folder")} onClick={stop}>{tr("Stop")}</button>
        </Show>
      </div>
      <Show when={confirm()} keyed>{(copy) => <ConfirmDialog copy={copy} onCancel={() => answer(false)} onConfirm={() => answer(true)} />}
      </Show>
    </div>
  );
}

/** One confirmation over the inspector; Cancel is focused and the default. */
function ConfirmDialog(props: { copy: Confirm; onCancel(): void; onConfirm(): void }) {
  let el!: HTMLDialogElement;
  let gone = false;
  onMount(() => { el.showModal(); queueMicrotask(() => el.querySelector<HTMLElement>("[autofocus]")?.focus()); });
  onCleanup(() => { gone = true; if (el.open) el.close(); });
  return (
    <dialog class="dlg sc-confirm" ref={el} role="alertdialog" aria-labelledby="sc-confirm-title" aria-describedby="sc-confirm-body"
      onCancel={(e) => { e.preventDefault(); props.onCancel(); }} onClose={() => { if (!gone) props.onCancel(); }}>
      <div class="dlg-head"><Icon name="triangle-alert" size={18} class="accent" /><h2 id="sc-confirm-title">{props.copy.title}</h2></div>
      <div class="dlg-body"><p class="confirm-body" id="sc-confirm-body">{props.copy.text}</p></div>
      <div class="dlg-foot">
        <span class="spacer" />
        <button type="button" class="btn solid-danger" onClick={props.onConfirm}>{props.copy.confirm}</button>
        <button type="button" class="btn primary" autofocus onClick={props.onCancel}>{tr("Cancel")}</button>
      </div>
    </dialog>
  );
}
