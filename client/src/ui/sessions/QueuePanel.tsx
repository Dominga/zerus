import { For, Show, createMemo, createSignal } from "solid-js";
import { composer } from "../../store/composer";
import { activity } from "../../store/session";
import { selected, subagentOpen } from "../../store/ui";
import { chord } from "../../store/shortcuts";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";
import { OutgoingMessage } from "./Activity";
import { queuedMessageRemovable, queuePresentation } from "./queue-message";
import { computerOffline } from "../../store/catalog";
import { detailsFor, perform } from "../../store/actions";
import { snapshotOf } from "./actions-ui";
import { teamConfirm } from "../../store/teamConfirm";

/** Native pending messages remain distinct from the sent Activity. */
export function QueuePanel() {
  const [removing, setRemoving] = createSignal(false);
  const display = createMemo(() => {
    const queue = composer.queue();
    return queue ? queuePresentation(queue, selected(), composer.view(), activity.target(), activity.summary(), !!subagentOpen()) : undefined;
  }, undefined, { equals: (a, b) => JSON.stringify(a?.items) === JSON.stringify(b?.items)
    && JSON.stringify(a?.scope) === JSON.stringify(b?.scope) });
  const tooltip = () => {
    const hint = composer.queue()?.hint ?? "", keys = chord("queue.sendNow");
    // Keep the existing Send now guard and customized shortcut.
    return keys ? hint ? `${hint} (${keys})` : keys : hint;
  };
  // Task #247: Qt's removeQueuedMessage: only OpenCode's not-delivered inbox
  // message, confirmed first; the core refuses a message that is no longer
  // reported as queued.
  const removable = () => {
    const session = selected();
    if (!session) return false;
    const offline = computerOffline(session.computer);
    return queuedMessageRemovable(session, composer.queue(), offline)
      && (detailsFor(session, !offline)?.allowed_actions?.includes("remove_queued") ?? false);
  };
  const remove = async () => {
    const queue = composer.queue(), session = selected();
    if (!queue || !session || !removable() || removing()) return;
    const ok = await teamConfirm.ask({
      name: "removeQueuedConfirm",
      title: tr("Remove queued message"),
      text: tr("Remove this queued message? It is still waiting in OpenCode's queue and runs first with your next message unless you remove it."),
      action: tr("Remove"),
    });
    if (!ok) return;
    setRemoving(true);
    try { await perform(snapshotOf(session), { kind: "remove_queued", message_id: queue.id }); }
    finally { setRemoving(false); }
  };
  return (
    <Show when={composer.queue()}>{(q) => (
      <div class="act-queue" role="region" aria-label={tr("Queued in agent")}>
        <div class="act-queue-head">
          <Icon name="clock" size={15} /><b>{tr("Queued in agent")}</b><span class="grow" />
          <Show when={removable()}>
            <button type="button" class="btn small" data-action="remove-queued" disabled={removing()} onClick={() => void remove()}>
              {removing() ? tr("Removing…") : tr("Remove")}
            </button>
          </Show>
          <button type="button" class="btn small" disabled={!q().enabled} title={tooltip()}
            onClick={() => { void composer.sendNow(q().id); }}>{q().label}</button>
        </div>
        <div class="act-queue-text">
          <For each={display()?.items}>{(item) => <div class="act-queue-message">
            <OutgoingMessage item={item} scope={display()?.scope} session={display()?.scope ? selected() : undefined} />
          </div>}</For>
        </div>
      </div>
    )}</Show>
  );
}
