import { For, Show, createMemo } from "solid-js";
import { composer } from "../../store/composer";
import { selected, subagentOpen } from "../../store/ui";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";
import { heldMessage } from "./held-message";

/**
 * A "Compact and continue" message while the agent compacts first
 * (zerus-pabah): shown as pending above the composer, so it is never out of
 * sight, until it is sent. If it is not sent, it goes back to the composer.
 */
export function HeldMessage() {
  const held = createMemo(() => heldMessage(composer.held(), composer.view(), selected(), !!subagentOpen()));
  return (
    <Show when={held()}>{(h) => (
      <div class="act-queue act-held" role="status" aria-label={tr("Sends after compaction")}>
        <div class="act-queue-head"><Icon name="clock" size={15} /><b>{tr("Sends after compaction")}</b></div>
        <div class="act-queue-text">
          <div class="act-queue-message">
            <p class="wrap-pre">{h().text}</p>
            <Show when={h().attachments.length}>
              <div class="act-held-files"><For each={h().attachments}>{(name) => <span class="chip">{name}</span>}</For></div>
            </Show>
          </div>
        </div>
        <p class="act-held-note">{tr("The agent is compacting its context. This exact message is sent once, automatically, when compaction completes, and can no longer be cancelled. If it is not sent, it returns to the composer.")}</p>
      </div>
    )}</Show>
  );
}
