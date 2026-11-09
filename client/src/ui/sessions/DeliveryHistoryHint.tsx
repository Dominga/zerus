import { Show } from "solid-js";
import { tr } from "../../i18n";
import type { ComposerView } from "../../types";

export function DeliveryHistoryHint(props: { view: ComposerView | undefined; text: string; shown: boolean }) {
  return <Show when={props.shown && props.view?.uncertain && props.view.history_text_match && props.text === props.view.text}>
    <p class="cmp-history-hint" role="status">{tr("This text may already have been delivered")}</p>
  </Show>;
}
