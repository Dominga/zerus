import { For, Show, createSignal, onMount } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { inTauri } from "../../../bridge";
import { Icon } from "../../lib/icons";
import { Actions, Group, Lines, SectionHead, Select, Status, Table } from "./ui";

// Settings > Mobile connection, read only for now (WorkspaceSettings.h,
// PhonePairing.cpp, MobileDeliverySettings.h) in the approved mockup's
// layout. The workspace's actions stay with their own work (zerus-5wmh.24.4
// to .24.8): they are shown, disabled, with what to use meanwhile. Only the
// shared notification timing changes here, through hgs, as in Qt.

interface Workspace { state: string; encrypted: boolean; summary: string; connection: string; pending: string | null; running: boolean }
interface Phone { id: string; name: string; platform: string; paired_at: number | null; last_seen_at: number | null; added_from: string | null }
interface PhoneList { host: string; workspace_id: string; workspace_name: string; encrypted: boolean; phones: Phone[] }
interface Phones { lists: PhoneList[]; lines: string[] }

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

function WorkspacePanel() {
  const [workspace, setWorkspace] = createSignal<Workspace | undefined>();
  const [checking, setChecking] = createSignal(false);
  const [notice, setNotice] = createSignal<string | undefined>();
  const refresh = () => {
    if (!inTauri() || checking()) return;
    setChecking(true);
    void invoke<Workspace>("mobile_workspace").then(
      (read) => { setWorkspace(read); setNotice(undefined); setChecking(false); },
      (error) => { setWorkspace(undefined); setNotice(String(error)); setChecking(false); },
    );
  };
  onMount(refresh);
  const active = () => workspace()?.state === "active";
  return (
    <Group title="Workspace">
      <p class="set-group-hint">Connect your computers and phones to one workspace. Session content is encrypted between your devices; the relay keeps routing and delivery metadata.</p>
      <Show when={!checking()} fallback={<Status kind="busy">Workspace: checking…</Status>}>
        <Show when={workspace()} fallback={<Lines text="Workspace status is unconfirmed" />}>
          {(read) => (
            <>
              <Lines text={read().summary} />
              <span class="muted">{read().connection}</span>
              <Show when={read().pending}>{(text) => <Status kind="warn">{text()}</Status>}</Show>
            </>
          )}
        </Show>
      </Show>
      <Show when={notice()}>{(text) => <Status kind="error">{text()}</Status>}</Show>
      <Actions>
        <button type="button" class="btn" disabled>Change workspace…</button>
        <button type="button" class="btn" disabled><Icon name="play" size={13} />Start connector</button>
        <button type="button" class="btn" disabled><Icon name="square" size={12} />Stop connector</button>
        <button type="button" class="btn" disabled={checking()} onClick={refresh}><Icon name="refresh-cw" size={13} />Refresh status</button>
      </Actions>
      <Show when={active()}>
        <Actions>
          <button type="button" class="btn" disabled><Icon name="key-round" size={13} />Give computer code</button>
          <button type="button" class="btn" disabled><Icon name="copy" size={13} />Copy computer code</button>
          <button type="button" class="btn" disabled>Cancel code</button>
        </Actions>
      </Show>
      <p class="set-group-hint">Changing the workspace, starting or stopping the connector and computer invitation codes are not available in this client yet.</p>
    </Group>
  );
}

function PhonesPanel() {
  const [phones, setPhones] = createSignal<Phones | undefined>();
  const [checking, setChecking] = createSignal(false);
  const [failure, setFailure] = createSignal<string | undefined>();
  const refresh = () => {
    if (!inTauri() || checking()) return;
    setChecking(true);
    void invoke<Phones>("mobile_phones").then(
      (read) => { setPhones(read); setFailure(undefined); setChecking(false); },
      (error) => { setFailure(String(error)); setChecking(false); },
    );
  };
  onMount(refresh);
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
          </>
        )}
      </For>
      <Actions>
        <button type="button" class="btn" disabled><Icon name="plus" size={14} />Pair a phone…</button>
        <button type="button" class="btn" disabled>Revoke…</button>
        <button type="button" class="btn" disabled={checking()} onClick={refresh}><Icon name="refresh-cw" size={13} />Refresh</button>
      </Actions>
      <p class="set-group-hint">
        To pair or revoke a phone, run <code>hgs [@computer] mobile invite --json</code> or{" "}
        <code>hgs [@computer] mobile revoke-phone ID --json</code> in a terminal.
      </p>
      <Show when={checking()} fallback={
        <Show when={failure() ?? phones()?.lines.join("\n")}>{(text) => <Lines class="muted" text={text()} />}</Show>
      }>
        <Status kind="busy">Checking phones…</Status>
      </Show>
    </Group>
  );
}

function DeliveryPanel() {
  const [value, setValue] = createSignal<string | undefined>();
  const [busy, setBusy] = createSignal(false);
  const [status, setStatus] = createSignal("");
  onMount(() => {
    if (!inTauri()) return;
    void invoke<string>("mobile_delivery").then(setValue, (error) =>
      setStatus(`Could not read the shared setting: ${String(error) || "unexpected response from hgs"}`));
  });
  const save = (next: string) => {
    if (busy() || next === value()) return;
    const previous = value();
    setValue(next);
    setBusy(true);
    setStatus("Saving…");
    void invoke<string>("mobile_delivery_set", { value: next }).then(
      (saved) => { setValue(saved); setBusy(false); setStatus("Saved. Your other computers and the phone receive it with the next sync."); },
      (error) => { setValue(previous); setBusy(false); setStatus(`Could not save: ${String(error) || "unexpected response from hgs"}`); },
    );
  };
  return (
    <Group title="Phone notifications">
      <label class="set-label" for="mobile-delivery">When I'm not at a computer, send notifications to my phone</label>
      <Select id="mobile-delivery" label="When I'm not at a computer, send notifications to my phone" value={value() ?? ""}
        disabled={busy() || value() === undefined} onChange={save} options={DELIVERY} />
      <Show when={status()}>{(text) => <Status kind={busy() ? "busy" : "muted"}>{text()}</Status>}</Show>
      <p class="set-group-hint">You count as away after two minutes without keyboard or mouse input on every computer running Zerus, or while their screens are locked. Your computers and the phone share this choice.</p>
    </Group>
  );
}

export function MobileSection() {
  return (
    <>
      <SectionHead title="Mobile connection" />
      <WorkspacePanel />
      <PhonesPanel />
      <DeliveryPanel />
    </>
  );
}
