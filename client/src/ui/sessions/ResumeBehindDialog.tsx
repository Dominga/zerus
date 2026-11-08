import { Show, onMount } from "solid-js";
import { Icon } from "../lib/icons";
import { tr } from "../../i18n";
import { perform } from "../../store/actions";
import { resumeBehind, type BehindRequest } from "../../store/resumeBehind";
import { resumeBehindText } from "./resume-behind";

// Task #247: Qt's "The transcript is behind" box (SessionsWindow.cpp:6086):
// Resume with the lost dialogue (default), Resume as it is, Cancel (escape).

function Dialog(props: { request: BehindRequest }) {
  let dialog!: HTMLDialogElement;
  onMount(() => {
    dialog.showModal();
    queueMicrotask(() => dialog.querySelector<HTMLElement>("[data-initial]")?.focus());
  });
  const answer = (choice: "lost" | "as-is" | "cancel") => {
    resumeBehind.close();
    if (choice === "cancel") return;
    void perform(props.request.row, { kind: "resume_stale", with_lost_dialogue: choice === "lost", accept_stale: choice === "as-is" });
  };
  return (
    <dialog ref={dialog} class="dlg team-confirm" aria-labelledby="resume-behind-title"
      onCancel={(event) => { event.preventDefault(); answer("cancel"); }}>
      <div class="dlg-head"><Icon name="triangle-alert" size={18} class="warn" /><h2 id="resume-behind-title">{tr("The transcript is behind")}</h2></div>
      <div class="dlg-body"><p class="team-confirm-text wrap-pre">{resumeBehindText(props.request.behind)}</p></div>
      <div class="dlg-foot"><span class="spacer" />
        <button type="button" class="btn primary" data-initial onClick={() => answer("lost")}>{tr("Resume with the lost dialogue")}</button>
        <button type="button" class="btn" onClick={() => answer("as-is")}>{tr("Resume as it is")}</button>
        <button type="button" class="btn" onClick={() => answer("cancel")}>{tr("Cancel")}</button>
      </div>
    </dialog>
  );
}

export function ResumeBehindDialog() {
  return <Show when={resumeBehind.state()} keyed>{(request) => <Dialog request={request} />}</Show>;
}
