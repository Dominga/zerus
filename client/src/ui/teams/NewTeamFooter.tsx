import { Show } from "solid-js";
import { tr } from "../../i18n";

/** Task #328 (F10b): the footer Qt shows at the bottom of New team. */
export interface NewTeamFooterProps {
  draft: boolean;
  dirty: boolean;
  revision: number;
  busy: boolean;
  preflightErrors?: number;
  createWhy: string;
  onUseLead?: () => void;
  onDiscard?: () => void;
  onCreate: () => void;
}

export function NewTeamFooter(props: NewTeamFooterProps) {
  const note = () => props.draft
    ? (props.dirty ? tr("Draft, unsaved changes") : tr("Draft saved, revision %1", props.revision))
    : tr("Nothing is created until you press Start planning.");
  return (
    <footer class="nt-foot">
      <span class="nt-hint grow">{note()}</span>
      <Show when={!props.draft}><span class="nt-hint">{tr("Start planning first")}</span></Show>
      <Show when={props.draft}>
        <Show when={props.preflightErrors !== undefined}>
          <span class="nt-hint">{props.preflightErrors ? tr("Preflight: %1 errors", props.preflightErrors) : tr("Preflight: ok")}</span>
        </Show>
        <Show when={props.dirty}>
          <button type="button" class="btn small ghost" title={tr("Drop your unsaved changes and show the draft as the lead last saved it")} onClick={() => props.onUseLead?.()}>{tr("Show the lead's version")}</button>
        </Show>
        <button type="button" class="btn small ghost danger" disabled={props.busy} onClick={() => props.onDiscard?.()}>{tr("Discard draft")}</button>
      </Show>
      <button type="button" class="btn small primary" disabled={!props.draft || props.busy || !!props.createWhy}
        title={!props.draft ? tr("Start planning first") : props.createWhy} onClick={() => props.onCreate()}>{tr("Create team")}</button>
    </footer>
  );
}
