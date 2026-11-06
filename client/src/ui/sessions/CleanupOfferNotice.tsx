import { Show } from "solid-js";
import { tr } from "../../i18n";
import { PROGRAM_LIST_CHANGED, unavailableServiceRemoval } from "./termination-offer";

/** No receipt or permission: manual review keeps its own native checks. */
export function CleanupOfferNotice(props: { error: string; review?: () => void }) {
  const unsupported = () => props.error === "termination_program_stop_unavailable";
  return <div role="status" class="cleanup-offer-notice">
    <p class="hint">{unsupported()
      ? tr("Update Zerus before stopping programs in the working folder.")
      : props.error === PROGRAM_LIST_CHANGED
      ? tr("The program list changed or could not be checked. Review the working folder again.")
      : unavailableServiceRemoval(props.error)
      ? tr("Automatic folder removal after Terminate is unavailable for this agent in background mode. Review the folder manually.")
      : tr("The folder removal option could not be checked. The working folder will be kept.")}</p>
    <Show when={!unsupported() && !unavailableServiceRemoval(props.error) && props.error !== PROGRAM_LIST_CHANGED}><p class="dlg-error">{props.error}</p></Show>
    <Show when={props.review}><button type="button" class="btn" onClick={props.review}>{tr("Review working folder…")}</button></Show>
  </div>;
}
