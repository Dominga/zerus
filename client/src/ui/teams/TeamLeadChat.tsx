import { Show } from "solid-js";
import { tr } from "../../i18n";
import { selected, subagentOpen } from "../../store/ui";
import { machineCards } from "../../store/machines";
import { composer } from "../../store/composer";
import { Composer } from "../sessions/Composer";
import { leadChatBound } from "./lead-chat";
import type { TeamView } from "./types";

/** Reuse the selected lead's actual composer, including its saved draft,
 * questions, exact identity guards, durable send UUID and uncertain outcome.
 * Switching drawers neither copies a draft nor constructs a second sender. */
export function TeamLeadChat(props: { view: TeamView }) {
  const ready = () => {
    const row = selected();
    return composer.view()?.loaded && leadChatBound(props.view, row, machineCards[row?.computer ?? ""]?.machine_id, composer.view()?.key, !!subagentOpen());
  };
  return <section class="team-lead-chat" aria-label={tr("Lead chat")}>
    <Show when={ready()} fallback={<p class="dim">{tr("The lead's current conversation is not available for sending.")}</p>}>
      <p class="dim">{tr("Messages go to the lead's current conversation. Its saved draft is shared with Activity.")}</p>
      <div class="cmp-col"><Composer /></div>
    </Show>
  </section>;
}
