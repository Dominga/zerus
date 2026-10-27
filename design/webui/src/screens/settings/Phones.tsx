import { For, Show, createMemo, createSignal } from "solid-js";
import { Icon } from "../../lib/icons";
import { demoState, setDialog } from "../../state";
import { PHONES } from "../../mock/settings-extra";
import { setDialogCtx } from "./store";
import { Actions, Status } from "./ui";

// Phones panel (PhonePairing.cpp:255-384): every phone of the workspaces this
// computer reaches, here or through one enabled peer. Used in Settings →
// Mobile connection and at the bottom of Machines.

export const [phones, setPhones] = createSignal(PHONES);
export const [revokedNote, setRevokedNote] = createSignal("");

export function PhonesPanel(props: { compact?: boolean }) {
  const [selected, setSelected] = createSignal<string | undefined>();
  const [refreshing, setRefreshing] = createSignal(false);
  const groups = createMemo(() => {
    const map = new Map<string, typeof PHONES>();
    for (const p of phones()) map.set(p.group, [...(map.get(p.group) ?? []), p]);
    return [...map.entries()];
  });
  const none = () => demoState() === "empty";
  const failed = () => demoState() === "error";
  const busy = () => refreshing() || demoState() === "loading";
  const status = () =>
    busy() ? "Checking phones…"
      : none() ? "Open Settings → Mobile connection to create a workspace or join one by code, then start the connector and refresh Phones."
      : failed() ? "Could not read the machine list: build-02: SSH request timed out"
      : phones().length === 1 ? "1 phone" : `${phones().length} phones`;
  const refresh = () => { setRefreshing(true); setRevokedNote(""); setTimeout(() => setRefreshing(false), 800); };
  return (
    <section class="set-phones" classList={{ compact: props.compact }}>
      <div class="set-group-head"><h3>Phones</h3></div>
      <p class="set-group-hint">Phones paired with your computers. A phone can control every computer of its workspace; revoke one you no longer use.</p>
      <div class="set-table-wrap">
        <table class="set-table" aria-label="Phones">
          <thead><tr><th scope="col">Phone</th><th scope="col">Platform</th><th scope="col">Paired</th><th scope="col">Last active</th><th scope="col">Added from</th></tr></thead>
          <tbody>
            <Show when={!none() && !busy()} fallback={<tr class="set-table-empty"><td colspan={5}>{busy() ? "" : "No phones"}</td></tr>}>
              <For each={groups()}>
                {([group, list]) => (
                  <>
                    <tr class="set-table-group"><td colspan={5}>{group}</td></tr>
                    <For each={list}>
                      {(p) => (
                        <tr classList={{ on: selected() === p.id }} tabindex={0} aria-selected={selected() === p.id} onClick={() => setSelected(p.id)}
                          onKeyDown={(e) => { if (e.key === "Delete") { setDialogCtx("phone", p.id); setDialog("settings-phone-revoke"); } }}>
                          <td><span class="row"><Icon name="smartphone" size={13} class="dim" />{p.name}</span></td>
                          <td>{p.platform}</td>
                          <td class="tabular">{p.paired}</td>
                          <td class="tabular">{p.lastActive}</td>
                          <td title={p.addedFrom ? "Verified device membership. Private device names are not stored by the relay." : "Paired with an administrator invitation, or before Zerus recorded it"}>
                            {p.addedFrom || <span class="dim">—</span>}
                          </td>
                        </tr>
                      )}
                    </For>
                  </>
                )}
              </For>
            </Show>
          </tbody>
        </table>
      </div>
      <Actions>
        <button class="btn" disabled={none() || busy()} onClick={() => setDialog("settings-phone-pair")}><Icon name="plus" size={14} />Pair a phone…</button>
        <button class="btn danger" disabled={!selected() || busy()} onClick={() => { setDialogCtx("phone", selected()); setDialog("settings-phone-revoke"); }}>Revoke…</button>
        <button class="btn" disabled={busy()} onClick={refresh}><Icon name="refresh-cw" size={13} />Refresh</button>
        <Status kind={busy() ? "busy" : failed() ? "error" : none() ? "warn" : "muted"}>{status()}</Status>
      </Actions>
      <Show when={revokedNote()}><Status kind="ok">{revokedNote()}</Status></Show>
    </section>
  );
}
