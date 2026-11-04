import { Show } from "solid-js";
import { tr } from "../../i18n";
import { unavailableServiceRemoval } from "./termination-offer";

/** No receipt or permission: manual review keeps its own native checks. */
export function CleanupOfferNotice(props: { error: string; review?: () => void }) {
  return <div role="status" class="cleanup-offer-notice">
    <p class="hint">{unavailableServiceRemoval(props.error)
      ? tr("Automatic folder removal after Terminate is unavailable for this agent in background mode. Review the folder manually.")
      : tr("The folder removal option could not be checked. The working folder will be kept.")}</p>
    <Show when={!unavailableServiceRemoval(props.error)}><p class="dlg-error">{props.error}</p></Show>
    <Show when={props.review}><button type="button" class="btn" onClick={props.review}>{tr("Review working folder…")}</button></Show>
  </div>;
}
