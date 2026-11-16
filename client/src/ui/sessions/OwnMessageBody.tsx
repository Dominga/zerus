import { Show, createMemo, createSignal } from "solid-js";
import { tr } from "../../i18n";
import { Body } from "./MarkdownBody";

/** Keep ordinary messages whole; larger bodies have an explicit expansion. */
export function OwnMessageBody(props: { text: string; truncated?: boolean }) {
  const [expanded, setExpanded] = createSignal(false);
  const prefix = createMemo(() => {
    let count = 0, end = 0;
    for (const character of props.text) {
      if (count++ === 4000) break;
      end += character.length;
    }
    return props.text.slice(0, end);
  });
  const large = () => prefix().length < props.text.length;
  const shown = () => large() && !expanded() ? prefix() : props.text;
  return <>
    <Body text={shown()} />
    <Show when={large()}><button type="button" class="link" aria-expanded={expanded()} onClick={() => setExpanded(value => !value)}>{expanded() ? tr("Show less") : tr("Show more")}</button></Show>
    <Show when={props.truncated}><p class="act-inbox-hint">{tr("Only part of this message was recorded. Open Terminal to read the complete message.")}</p></Show>
  </>;
}
