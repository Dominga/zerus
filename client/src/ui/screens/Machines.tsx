import { Index, Show, createEffect, createMemo, createSignal, on, type JSX } from "solid-js";
import { Icon } from "../lib/icons";
import { OS_NAME } from "../lib/os-icons";
import { MachineTile, type MachineState } from "../components/MachineTile";
import { MachineLabel } from "../components/MachineLabel";
import { Actions, Lines, Spinner, Status } from "../components/Status";
import { paletteIndex } from "../sessions/SessionCard";
import type { MachineMeta, MachineRow } from "../../types";
import { catalog } from "../../store/catalog";
import { loadMachines, machineCards, machinesPage, requestedMachine, resolveMachine, setRequestedMachine, testConnection } from "../../store/machines";
import { setAccountMachine } from "../../store/accounts";
import { connectionOf, osOf } from "../../store/overview";
import { setMachineFilter, setPage } from "../../store/ui";
import { MACHINE_EDITING } from "./reasons";
import { LOCAL, localState, peerState, placeholders, summary, testEnabled, type LocalState, type PeerState } from "./machines-ui";

// Machines (MachinesPage.cpp), ported from design/webui/src/screens/Machines.tsx:
// this computer and the saved peers of `hgs machine ls`, each with its stream
// state, and an editor with the Zerus overrides and the effective SSH
// connection. Editing, installing and removing arrive with machine editing
// (3c) and keep their place disabled; Test connection is a read and works.

const [selected, setSelected] = createSignal(new URLSearchParams(location.search).get("machine") ?? LOCAL);

function MetaLine(props: { meta: MachineMeta }) {
  return (
    <span class="mach-meta tabular parts">
      <Show when={osOf(props.meta.platform)}>{(os) => <span>{OS_NAME[os()]}</span>}</Show>
      <Show when={props.meta.hgs_version}><span>hgs {props.meta.hgs_version}</span></Show>
    </span>
  );
}

function MachineCard(props: { selection: string; alias: string; line: string; tile: MachineState; meta: MachineMeta | undefined; showMeta: boolean; offline: boolean }) {
  const on = () => selected() === props.selection;
  return (
    <button class="mach-card" classList={{ on: on(), offline: props.offline }} aria-current={on()} onClick={() => setSelected(props.selection)}>
      <MachineTile os={osOf(props.meta?.platform)} state={props.tile} />
      <span class="mach-col">
        <span class="row"><MachineLabel alias={props.alias} /></span>
        <span class="mach-state">{props.line}</span>
        <Show when={props.showMeta && props.meta}>{(m) => <MetaLine meta={m()} />}</Show>
      </span>
    </button>
  );
}

function Field(props: { label: string; id: string; children: JSX.Element }) {
  return (
    <div class="form-row mach-form-row">
      <label for={props.id}>{props.label}</label>
      <div class="col" style={{ gap: "4px" }}>{props.children}</div>
    </div>
  );
}

function Editor(props: { local: LocalState; peers: { row: MachineRow; state: PeerState }[] }) {
  const view = machinesPage;
  const isLocal = () => selected() === LOCAL;
  const peer = () => props.peers.find((p) => p.row.alias === selected());
  const row = () => peer()?.row;
  const name = () => (isLocal() ? props.local.name : selected());
  const effective = () => (view().effective?.alias === selected() ? view().effective : null);
  const check = () => (view().check?.alias === selected() ? view().check : null);
  const hint = () => placeholders(effective(), selected());
  const formHidden = () => isLocal() && props.local.formHidden;
  return (
    <div class="mach-editor">
      <div class="mach-title">
        <h2>{name()}</h2>
        <span class="grow" />
        <Show when={peer()}>{(p) => <span class="chip" classList={{ "mach-chip-ok": p().state.live }}>{p().state.line}</span>}</Show>
      </div>
      <p class="muted mach-origin">{isLocal() ? props.local.origin : row()?.origin}</p>
      <Show when={isLocal() && props.local.sessionHost}><p class="mach-host selectable">{props.local.sessionHost}</p></Show>

      <section class="mach-section">
        <h3>Appearance</h3>
        <div class="row mach-appearance">
          <button class="btn" disabled title={MACHINE_EDITING}>
            <span class="mach-swatch" style={{ "--entity": `var(--z-entity-palette-${paletteIndex(name())})` }} />Label color…
          </button>
          <select class="field" aria-label="Label style" disabled title={MACHINE_EDITING}>
            <option value="soft">Soft</option><option value="bright">Bright</option>
          </select>
          <button class="btn ghost" disabled title={MACHINE_EDITING}>Automatic</button>
        </div>
        <div class="row mach-preview">
          <span class="dim">Preview</span>
          <MachineLabel alias={name()} />
          <span class="dim">Saved on this device</span>
        </div>
      </section>

      <Show when={!formHidden()}>
        <section class="mach-section">
          <h3>Connection</h3>
          <Show when={!isLocal() && effective()}>{(e) => (
            <Show when={!e().pending} fallback={<Status kind="busy">{e().text}</Status>}>
              <Show when={!e().error} fallback={<Status kind="error"><Lines text={e().text} /></Status>}>
                <div class="mach-effective selectable"><Lines text={e().text} /></div>
              </Show>
            </Show>
          )}</Show>
          <Field label="Name / SSH alias" id="mach-alias">
            <input id="mach-alias" class="field" value={name()} readOnly disabled={isLocal()} title={MACHINE_EDITING} />
          </Field>
          <Field label="Hostname / IP" id="mach-host">
            <input id="mach-host" class="field" placeholder={hint().hostname} value={row()?.hostname ?? ""} readOnly disabled={isLocal()} title={MACHINE_EDITING} />
          </Field>
          <Field label="SSH user" id="mach-user">
            <input id="mach-user" class="field" placeholder={hint().user} value={row()?.user ?? ""} readOnly disabled={isLocal()} title={MACHINE_EDITING} />
          </Field>
          <Field label="Port" id="mach-port">
            <input id="mach-port" class="field tabular mach-port" placeholder={hint().port} value={row()?.port ?? ""} readOnly disabled={isLocal()} title={MACHINE_EDITING} />
          </Field>
          <Field label="Private key path" id="mach-key">
            <div class="row">
              <input id="mach-key" class="field mono grow" placeholder={hint().key} value={row()?.identity_file ?? ""} readOnly disabled={isLocal()} title={MACHINE_EDITING} />
              <button class="btn" disabled title={MACHINE_EDITING}>Browse…</button>
            </div>
          </Field>
          <div class="form-row mach-form-row">
            <span />
            <label class="check" title={MACHINE_EDITING}>
              <input type="checkbox" checked={isLocal() || (row()?.enabled ?? true)} disabled />Show sessions and include in background polling
            </label>
          </div>
          <Actions>
            <button class="btn primary" disabled title={MACHINE_EDITING}>Save connection</button>
            <button class="btn" disabled={!testEnabled(selected(), view())} onClick={() => void testConnection(selected())}><Icon name="plug" size={13} />Test connection</button>
            <button class="btn" disabled title={MACHINE_EDITING}><Icon name="square-terminal" size={13} />Open SSH terminal</button>
          </Actions>
        </section>
      </Show>

      <section class="mach-section">
        <h3>Setup</h3>
        <Actions>
          <Show when={isLocal() && props.local.canEnable}>
            <button class="btn primary" disabled title={MACHINE_EDITING}>Enable local sessions</button>
          </Show>
          <Show when={!formHidden()}>
            <button class="btn" disabled title={MACHINE_EDITING}><Icon name="download" size={13} />Install / update HGS…</button>
          </Show>
          <Show when={!(isLocal() && props.local.console)}>
            <button class="btn" disabled={view().busy} onClick={() => { setMachineFilter([name()]); setPage("sessions"); }}>
              <Icon name="rows-3" size={13} />View sessions
            </button>
            <button class="btn" disabled={view().busy} onClick={() => { setAccountMachine(isLocal() ? "" : selected()); setPage("accounts"); }}>
              <Icon name="user-round-cog" size={13} />Agent accounts…
            </button>
          </Show>
        </Actions>
        <Show when={check()}>{(c) => <Status kind={c().pending ? "busy" : c().error ? "error" : "muted"}><Lines text={c().text} /></Status>}</Show>
      </section>

      <Show when={!formHidden()}>
        <div class="mach-foot">
          <button class="btn" disabled title={MACHINE_EDITING}>Reset SSH overrides</button>
          <button class="btn danger" disabled title={MACHINE_EDITING}><Icon name="trash" size={13} />Remove from Zerus</button>
        </div>
      </Show>
    </div>
  );
}

export function MachinesScreen() {
  const view = machinesPage;
  // Rows per computer from the row ids alone (computer + "\n" + key): one pass per batch.
  const sessions = createMemo(() => {
    const per: Record<string, number> = {};
    for (const id of catalog.order) {
      const computer = id.slice(0, id.indexOf("\n"));
      per[computer] = (per[computer] ?? 0) + 1;
    }
    return per;
  });
  const local = () => localState(machineCards[""], view().local);
  const peers = createMemo(() => view().machines.map((row) => ({ row, state: peerState(row, catalog.computers[row.alias], sessions()[row.alias] ?? 0) })));
  // Another screen asked for a machine (Overview's Machine settings).
  createEffect(() => {
    const alias = requestedMachine();
    if (alias === undefined) return;
    setSelected(alias);
    setRequestedMachine(undefined);
  });
  // A selection that left the list falls back to this computer (loadSelection).
  createEffect(() => {
    if (view().loaded && selected() !== LOCAL && !view().machines.some((m) => m.alias === selected())) setSelected(LOCAL);
  });
  // Selecting a peer reads its effective connection (resolveConnection).
  createEffect(on(selected, (key) => { if (key !== LOCAL) void resolveMachine(key); }));
  return (
    <div class="mach-page">
      <div class="mach-shell">
        <header class="mach-head">
          <h1>Machines</h1>
          <button class="icon-btn" title="Refresh machines" aria-label="Refresh machines" disabled={view().busy} onClick={() => void loadMachines(true)}>
            <Show when={!view().refreshing} fallback={<Spinner />}><Icon name="refresh-cw" size={15} /></Show>
          </button>
          <span class="grow" />
          <button class="btn primary" disabled title={MACHINE_EDITING}><Icon name="plus" size={14} />Add machine</button>
        </header>
        <p class="muted mach-summary">{summary({ console: local().console, live: ["live", "polled"].includes(catalog.computers[""]?.status ?? "") }, peers().map((p) => p.state))}</p>
        <Show when={view().error}>{(error) => <Status kind="error"><Lines text={error()} /></Status>}</Show>
        <div class="mach-body">
          <div class="mach-list" role="list" aria-label="Machines">
            <MachineCard selection={LOCAL} alias={local().name} line={local().line} tile={connectionOf(catalog.computers[""]?.status).tile} meta={machineCards[""]} showMeta={!local().console} offline={false} />
            <Index each={peers()}>{(p) => <MachineCard selection={p().row.alias} alias={p().row.alias} line={p().state.line} tile={p().state.tile} meta={machineCards[p().row.alias]} showMeta offline={!p().state.live} />}</Index>
          </div>
          <Editor local={local()} peers={peers()} />
        </div>
      </div>
    </div>
  );
}
