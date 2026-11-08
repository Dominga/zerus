import { Show, type Component } from "solid-js";
import { tr } from "../../i18n";
import { questions } from "../../store/questions";
import { composer } from "../../store/composer";
import { LazyBoundary } from "../lazy-boundary";
import { lazyView } from "../lazy-view";

// The question card and the composer's confirmations load outside the entry
// chunk (zerus-ycycu). One that is pending before its module arrives (the
// first moments after start, or a slow or failed load) is never shown as
// nothing: a notice says it is waiting until the card appears, and a failed
// load offers Retry. The question or confirmation itself stays pending in its
// store, so nothing is answered or dropped meanwhile.
const QuestionCardView = lazyView(() => import("./QuestionCard"), (m) => m.QuestionCard);
const ComposerDialogsView = lazyView(() => import("./ComposerDialogs"), (m) => m.ComposerDialogs);

export function QuestionSlot(props: { view?: Component; pending?: () => boolean }) {
  const View = props.view ?? QuestionCardView;
  const pending = () => (props.pending ? props.pending() : !!questions.view());
  return (
    <LazyBoundary failed={tr("The question could not be shown.")}
      loading={<Show when={pending()}><div class="lazy-waiting" role="status">{tr("A question is waiting. Loading it…")}</div></Show>}>
      <View />
    </LazyBoundary>
  );
}

export function ComposerDialogsSlot(props: { onClosed?: () => void; view?: Component<{ onClosed?: () => void }>; pending?: () => boolean }) {
  const View = props.view ?? ComposerDialogsView;
  const pending = () => (props.pending ? props.pending() : !!composer.dialog());
  return (
    <LazyBoundary failed={tr("The confirmation could not be shown.")}
      loading={<Show when={pending()}><div class="lazy-waiting" role="status">{tr("A confirmation is waiting. Loading it…")}</div></Show>}>
      <View onClosed={props.onClosed} />
    </LazyBoundary>
  );
}
