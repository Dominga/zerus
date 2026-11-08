import { Show, createSignal, onMount } from "solid-js";
import { Icon } from "./lib/icons";
import { quitApp, sandboxCheck } from "../bridge";
import { sandboxWarningCopy } from "./sandbox-warning";

// Task #247: Qt's blocking "Zerus is in a sandbox" box (main.cpp:457). The
// owner sees why sessions and files fail before anything is staged, and can
// quit; Continue goes on to the app.

function SandboxDialog(props: { reason: string; close: () => void }) {
  let dialog!: HTMLDialogElement;
  const copy = sandboxWarningCopy(props.reason);
  onMount(() => {
    dialog.showModal();
    queueMicrotask(() => dialog.querySelector<HTMLElement>("[data-initial]")?.focus());
  });
  const quit = () => { void quitApp().catch(() => dialog.close()); };
  return (
    <dialog ref={dialog} class="dlg team-confirm sandbox-warning" aria-labelledby="sandbox-warning-title"
      onCancel={(event) => { event.preventDefault(); props.close(); }}>
      <div class="dlg-head"><Icon name="triangle-alert" size={18} class="warn" /><h2 id="sandbox-warning-title">{copy.title}</h2></div>
      <div class="dlg-body"><p class="team-confirm-text">{copy.text}</p></div>
      <div class="dlg-foot"><span class="spacer" />
        <button type="button" class="btn solid-danger" data-initial onClick={quit}>{copy.quit}</button>
        <button type="button" class="btn" onClick={props.close}>{copy.stay}</button>
      </div>
    </dialog>
  );
}

export function SandboxWarning() {
  const [reason, setReason] = createSignal<string>();
  onMount(() => { void sandboxCheck().then((found) => setReason(found ?? undefined), () => undefined); });
  return <Show when={reason()}>{(found) => <SandboxDialog reason={found()} close={() => setReason(undefined)} />}</Show>;
}
