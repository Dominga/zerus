import { For, Show } from "solid-js";
import { tr } from "../../i18n";
import type { FindingView } from "./types";

/** Task #328 (F10b): the footer Qt shows at the bottom of New team. */
export interface NewTeamFooterProps {
  draft: boolean;
  dirty: boolean;
  revision: number;
  busy: boolean;
  preflightErrors?: number;
  /** Owner, 2026-11-09: the preflight errors themselves, shown, never only counted. */
  errors?: FindingView[];
  createWhy: string;
  /** Owner, 2026-11-09: a fix the owner can apply in one click (e.g. raise the member limit). */
  fix?: { label: string; run: () => void };
  onUseLead?: () => void;
  onDiscard?: () => void;
  onCreate: () => void;
}

/**
 * Owner, 2026-11-09 ("я не могу начать работу, и ты НИГДЕ не пишешь что мешает
 * старту"): what blocks Create team is written out beside the button, every
 * preflight error is listed, and a fix the form can make offers its button.
 * The bar sits under the form, where the owner edits.
 */
export function NewTeamFooter(props: NewTeamFooterProps) {
  const note = () => props.draft
    ? (props.dirty ? tr("Draft, unsaved changes") : tr("Draft saved, revision %1", props.revision))
    : tr("Nothing is created until you press Start planning.");
  return (
    <footer class="nt-foot" data-new-team-footer>
      <Show when={props.draft && props.createWhy}>
        <div class="nt-blocker" role="alert" data-create-blocker>
          <b>{tr("Create team is blocked:")}</b> <span>{props.createWhy}</span>
          <Show when={props.fix}>{(fix) => <button type="button" class="btn small" onClick={() => fix().run()}>{fix().label}</button>}</Show>
          <Show when={(props.errors?.length ?? 0) > 0}>
            <ul class="nt-blocker-list">
              <For each={props.errors}>{(error) => (
                <li>{error.text}<Show when={error.task}> <span class="nt-hint">#{error.task}</span></Show><Show when={error.member}> <span class="nt-hint">{error.member}</span></Show></li>
              )}</For>
            </ul>
          </Show>
        </div>
      </Show>
      <div class="nt-foot-row">
        <span class="nt-hint grow">{note()}</span>
        <Show when={props.draft}>
          <Show when={props.preflightErrors !== undefined}>
            <span class="nt-hint" classList={{ "nt-foot-bad": !!props.preflightErrors }}>{props.preflightErrors ? tr("Preflight: %1 errors", props.preflightErrors) : tr("Preflight: ok")}</span>
          </Show>
          <Show when={props.dirty}>
            <button type="button" class="btn small ghost" title={tr("Drop your unsaved changes and show the draft as the lead last saved it")} onClick={() => props.onUseLead?.()}>{tr("Show the lead's version")}</button>
          </Show>
          <button type="button" class="btn small ghost danger" disabled={props.busy} onClick={() => props.onDiscard?.()}>{tr("Discard draft")}</button>
        </Show>
        <button type="button" class="btn primary nt-create" disabled={!props.draft || props.busy || !!props.createWhy}
          title={!props.draft ? tr("Start planning first") : props.createWhy} onClick={() => props.onCreate()}>{tr("Create team")}</button>
      </div>
    </footer>
  );
}
