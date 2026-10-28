import { For, Show, createSignal, onCleanup, onMount } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { inTauri } from "../../../bridge";
import { Icon } from "../../lib/icons";
import { Actions, Group, Lines, SectionHead, Select, Status, Table } from "./ui";
import { createMobile, type MobileState, type View } from "./mobile-page";

// Settings > Mobile connection (WorkspaceSettings.h, PhonePairing.cpp,
// MobileDeliverySettings.h, SettingsPage.h:124-139) in the approved
// mockup's layout. The reads and their answers live in the app, one of each
// kind at a time, for the window's life. The workspace's actions stay with
// their own work (zerus-5wmh.24.4 to .24.8): they are shown as Qt shows
// them for the workspace's state, disabled, with what to use meanwhile.
// Only the shared notification timing changes here, through hgs, as in Qt.

const DELIVERY: { value: string; label: string }[] = [
  { value: "immediate", label: "Immediately, even if I'm at a computer" },
  { value: "away", label: "As soon as I'm away" },
  { value: "away_5", label: "After 5 minutes away" },
  { value: "away_10", label: "After 10 minutes away" },
  { value: "away_15", label: "After 15 minutes away" },
  { value: "away_30", label: "After 30 minutes away" },
];

/** QLocale's short date and time, or a dash when unknown. */
const when = (seconds: number | null) =>
  seconds ? new Date(seconds * 1000).toLocaleString(undefined, { dateStyle: "short", timeStyle: "short" }) : "—";

/** A workspace's heading: where its phones were read from. */
export const listTitle = (list: { host: string; workspace_name: string }) =>
  list.host ? `${list.workspace_name} (via ${list.host})` : `${list.workspace_name} (this computer)`;

function WorkspacePanel(props: { view?: View; refresh: () => void }) {
  const read = () => props.view?.workspace ?? undefined;
  const busy = () => !!props.view?.workspace_busy;
  const controls = () => read()?.controls;
  return (
    <Group title="Workspace">
      <p class="set-group-hint">Connect your computers and phones to one workspace. Session content is encrypted between your devices; the relay keeps routing and delivery metadata.</p>
      <Show when={read()} fallback={
        <Show when={props.view?.workspace_error} fallback={<Status kind="busy">Workspace: checking…</Status>}>
          <Lines text="Workspace status is unconfirmed" />
        </Show>
      }>
        {(workspace) => (
          <>
            <Lines text={workspace().summary} />
            <span class="muted">{workspace().connection}</span>
            <Show when={workspace().pending}>{(text) => <Status kind="warn">{text()}</Status>}</Show>
          </>
        )}
      </Show>
      <Show when={props.view?.workspace_error}>{(text) => <Status kind="error">{text()}</Status>}</Show>
      <Actions>
        <Show when={controls()?.change}><button type="button" class="btn" disabled>Change workspace…</button></Show>
        <Show when={controls()?.enrollment}><button type="button" class="btn" disabled>Create workspace</button></Show>
        <Show when={controls()?.start}><button type="button" class="btn" disabled><Icon name="play" size={13} />Start connector</button></Show>
        <Show when={controls()?.stop}><button type="button" class="btn" disabled><Icon name="square" size={12} />Stop connector</button></Show>
        <button type="button" class="btn" disabled={busy() || !props.view} onClick={props.refresh}><Icon name="refresh-cw" size={13} />Refresh status</button>
      </Actions>
      <Show when={controls()?.check}>{(text) => <Actions><button type="button" class="btn" disabled>{text()}</button></Actions>}</Show>
      <Show when={controls()?.invitations}>
        <Actions>
          <button type="button" class="btn" disabled><Icon name="key-round" size={13} />Give computer code</button>
          <button type="button" class="btn" disabled><Icon name="copy" size={13} />Copy computer code</button>
          <button type="button" class="btn" disabled>Cancel code</button>
        </Actions>
      </Show>
      <Actions>
        <button type="button" class="btn ghost" disabled><Icon name="chevron-right" size={13} />Advanced: legacy connector configuration</button>
      </Actions>
      <p class="set-group-hint">Changing the workspace, starting or stopping the connector, checking a saved connection, computer invitation codes and the legacy connector configuration are not available in this client yet.</p>
    </Group>
  );
}

function PhonesPanel(props: { view?: View; refresh: () => void }) {
  const phones = () => props.view?.phones ?? undefined;
  const busy = () => !!props.view?.phones_busy;
  return (
    <Group title="Phones" hint="Phones paired with your computers. A phone can control every computer of its workspace; revoke one you no longer use.">
      <For each={phones()?.lists ?? []}>
        {(list) => (
          <>
            <span class="set-label">{listTitle(list)}</span>
            <Table label="Phones" rows={list.phones} rowKey={(phone) => phone.id}
              columns={[
                { key: "phone", label: "Phone", render: (phone) => phone.name },
                { key: "platform", label: "Platform", render: (phone) => phone.platform },
                { key: "paired", label: "Paired", class: "tabular", render: (phone) => when(phone.paired_at) },
                { key: "last", label: "Last active", class: "tabular", render: (phone) => when(phone.last_seen_at) },
                { key: "added", label: "Added from", render: (phone) => phone.added_from ?? (
                  <span title={list.encrypted
                    ? "Verified device membership. Private device names are not stored by the relay."
                    : "Paired with an administrator invitation, or before Zerus recorded it"}>—</span>
                ) },
              ]} />
            <div class="set-group-hint mobile-commands">
              <div>To pair a phone with this workspace, run <code>{list.invite}</code> in a terminal.</div>
              <For each={list.phones}>
                {(phone) => <div>To revoke {phone.name}, run <code>{phone.revoke}</code>.</div>}
              </For>
            </div>
          </>
        )}
      </For>
      <Actions>
        <button type="button" class="btn" disabled><Icon name="plus" size={14} />Pair a phone…</button>
        <button type="button" class="btn" disabled>Revoke…</button>
        <button type="button" class="btn" disabled={busy() || !props.view} onClick={props.refresh}><Icon name="refresh-cw" size={13} />Refresh</button>
      </Actions>
      <Show when={busy()} fallback={
        <Show when={phones()?.lines.join("\n")}>{(text) => <Lines class="muted" text={text()} />}</Show>
      }>
        <Status kind="busy">Checking phones…</Status>
      </Show>
    </Group>
  );
}

function DeliveryPanel(props: { view?: View; set: (value: string) => void }) {
  const value = () => props.view?.delivery ?? "";
  const busy = () => !!props.view?.delivery_busy;
  // Made again when the value or its call changes, so a choice that was not
  // saved never stays shown.
  const shown = () => `${value()}|${busy()}`;
  return (
    <Group title="Phone notifications">
      <label class="set-label" for="mobile-delivery">When I'm not at a computer, send notifications to my phone</label>
      <Show when={shown()} keyed>
        <Select id="mobile-delivery" label="When I'm not at a computer, send notifications to my phone" value={value()}
          disabled={busy() || !props.view?.delivery} onChange={props.set} options={DELIVERY} />
      </Show>
      <Show when={props.view?.delivery_status}>{(text) => <Status kind={busy() ? "busy" : "muted"}>{text()}</Status>}</Show>
      <Show when={props.view?.delivery_outdated}>{(text) => <Status kind="warn">{text()}</Status>}</Show>
      <Show when={props.view?.delivery_worker}>{(text) => <Status kind="warn">{text()}</Status>}</Show>
      <p class="set-group-hint">You count as away after two minutes without keyboard or mouse input on every computer running Zerus, or while their screens are locked. Your computers and the phone share this choice.</p>
    </Group>
  );
}

export function MobileSection() {
  const [state, setState] = createSignal<MobileState>({});
  const page = createMobile({
    view: () => invoke<View>("mobile_view"),
    open: () => invoke<View>("mobile_open"),
    refreshWorkspace: () => invoke<View>("mobile_refresh_workspace"),
    refreshPhones: () => invoke<View>("mobile_refresh_phones"),
    setDelivery: (value) => invoke<View>("mobile_delivery_set", { value }),
  }, setState);
  onMount(() => {
    if (!inTauri()) return;
    let gone = false;
    let stop: (() => void) | undefined;
    // Read again once listening: an answer in between would otherwise wait.
    listen("mobile-changed", () => page.changed())
      .then((off) => { if (gone) off(); else { stop = off; page.changed(); } }, () => {});
    page.open();
    // The 5-second swarm read keeps the timing and its notices current.
    const timer = setInterval(() => page.changed(), 5000);
    onCleanup(() => { gone = true; stop?.(); clearInterval(timer); });
  });
  const view = () => state().view;
  return (
    <>
      <SectionHead title="Mobile connection" />
      <Show when={state().error}>{(text) => <Status kind="error">{text()}</Status>}</Show>
      <WorkspacePanel view={view()} refresh={() => page.refreshWorkspace()} />
      <PhonesPanel view={view()} refresh={() => page.refreshPhones()} />
      <DeliveryPanel view={view()} set={(value) => page.setDelivery(value)} />
    </>
  );
}
