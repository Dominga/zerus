import { onCleanup, onMount } from "solid-js";
import { closeTeamHelp, teamHelpLabel, teamHelpLanguage } from "../../store/teamHelp";
import { Icon } from "../lib/icons";
import { TeamGuideReader } from "./TeamGuideReader";

export function TeamHelpDialog() {
  let dialog!: HTMLDialogElement;
  let close!: HTMLButtonElement;
  onMount(() => { dialog.showModal(); close.focus(); });
  onCleanup(() => { if (dialog.open) dialog.close(); });
  return <dialog ref={dialog} class="dlg team-help-dialog" aria-labelledby="team-help-title"
    onCancel={(event) => { event.preventDefault(); closeTeamHelp(); }} onClose={() => { if (dialog.isConnected) closeTeamHelp(); }}>
    <header class="dlg-head"><Icon name="file-text" size={18} /><h2 id="team-help-title">{teamHelpLabel()}</h2><span class="grow" />
      <button ref={close} type="button" class="icon-btn" aria-label={teamHelpLanguage() === "ru" ? "Закрыть справку" : "Close guide"} onClick={closeTeamHelp}><Icon name="x" size={18} /></button>
    </header>
    <TeamGuideReader />
  </dialog>;
}
