import { Show } from "solid-js";
import { tr } from "../../i18n";

/** The footer at the bottom of New team: what blocks Create team, the save
 * as template button, and Create itself. */
export interface NewTeamFooterProps {
  busy: boolean;
  /** Why Create team cannot run yet ("" when it can). */
  createWhy: string;
  /** The last refusal of a create or a save, always shown. */
  error?: string;
  canSave: boolean;
  saving: boolean;
  /** False while the one create intent waits for its Check or Dismiss. */
  showCreate?: boolean;
  planning?: boolean;
  onSaveTemplate: () => void;
  onCancel: () => void;
  onCreate: () => void;
}

export function NewTeamFooter(props: NewTeamFooterProps) {
  return (
    <footer class="nt-foot" data-new-team-footer>
      <Show when={props.error}>
        <div class="notice danger nt-blocker" role="alert" data-create-error>
          <b>{tr("New team did not go through:")}</b> <span>{props.error}</span>
        </div>
      </Show>
      <Show when={!props.error && props.createWhy}>
        <div class="notice warn nt-blocker" role="status" data-create-blocker>
          <b>{tr("Create team is blocked:")}</b> <span>{props.createWhy}</span>
        </div>
      </Show>
      <div class="nt-foot-row">
        <span class="nt-hint grow">{props.planning ? tr("Nothing starts until you press Start planning.") : tr("Nothing is created until you press Create team.")}</span>
        <button type="button" class="btn nt-save" disabled={!props.canSave || props.saving || props.busy}
          onClick={props.onSaveTemplate}>{tr("Save as template")}</button>
        <Show when={props.showCreate !== false}>
          <button type="button" class="btn primary nt-create" disabled={!!props.createWhy || props.busy}
            title={props.createWhy} onClick={props.onCreate}>{props.planning ? tr("Start planning") : tr("Create team")}</button>
        </Show>
        <button type="button" class="btn" disabled={props.busy} onClick={props.onCancel}>{tr("Cancel")}</button>
      </div>
    </footer>
  );
}
