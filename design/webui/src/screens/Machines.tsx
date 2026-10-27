import { For, Show, createMemo, createSignal, type JSX } from "solid-js";
import { createStore } from "solid-js/store";
import { Icon } from "../lib/icons";
import { demoState, setDialog, setPage, setMachineFilter, toast } from "../state";
import { machines as fleet, sessions } from "../mock/data";
import type { Machine } from "../mock/types";
import { MACHINE_CONNECTIONS, INSTALL_LOG, type MachineConnection } from "../mock/settings-extra";
import { platform } from "./settings/store";
import { PhonesPanel } from "./settings/Phones";
import { Actions, Status, Lines, Spinner } from "./settings/ui";
import { MachineLabel, labels, setLabels, labelColor } from "./settings/machine-label";
import { setAccountMachine } from "./Accounts";
import "../styles/settings.css";

// Machines (MachinesPage.cpp): SSH connections and HGS setup. The list shows
// this computer and the saved peers; the editor keeps Zerus overrides only,
// and blank fields inherit OpenSSH configuration.

const LOCAL = fleet.find((m) => m.thisComputer)!;
const params = new URLSearchParams(location.search);

export const [peers, setPeers] = createStore<MachineConnection[]>(structuredClone(MACHINE_CONNECTIONS));
export const [selectedMachine, setSelectedMachine] = createSignal<string>(params.get("machine") === "new" ? "@new" : params.get("machine") ?? "@local");
export const [machineNotice, setMachineNotice] = createSignal<{ text: string; error?: boolean } | undefined>();
export const [pendingMachine, setPendingMachine] = createSignal<string | undefined>();
export const [localSessions, setLocalSessions] = createSignal(true);
const [draft, setDraft] = createStore({ alias: "", hostname: "", user: "", port: 0, key: "", enabled: true, dirty: false });
const [installing, setInstalling] = createSignal(false);
const [log, setLog] = createSignal<string[]>([]);
const [testing, setTesting] = createSignal(false);

const machineOf = (alias: string): Machine | undefined => fleet.find((m) => m.alias === alias);
const sessionCount = (alias: string) => sessions.filter((s) => s.machine === alias).length;
const isConsole = () => demoState() === "empty" || !localSessions();
const windowsLocal = () => platform === "windows";
const visiblePeers = () => (demoState() === "empty" ? [] : peers);

function loadDraft(key: string) {
  if (key === "@new") { setDraft({ alias: "", hostname: "", user: "", port: 0, key: "", enabled: true, dirty: false }); return; }
  const c = peers.find((p) => p.alias === key);
  setDraft({ alias: key === "@local" ? LOCAL.alias : key, hostname: c?.hostname ?? "", user: c?.user ?? "", port: c?.port ?? 0, key: c?.key ?? "", enabled: c?.enabled ?? true, dirty: false });
}
loadDraft(selectedMachine());

/** Select a row; unsaved edits ask first (MachinesPage::discardChanges). */
export function selectMachine(key: string) {
  if (key === selectedMachine()) return;
  if (installing() || testing()) return;
  if (draft.dirty) { setPendingMachine(key); setDialog("machines-unsaved"); return; }
  commitSelection(key);
}
export function commitSelection(key: string) {
  setSelectedMachine(key);
  setMachineNotice(undefined);
  setLog([]);
  loadDraft(key);
}

export function removeSelectedMachine() {
  const alias = selectedMachine();
  setPeers((list) => list.filter((p) => p.alias !== alias));
  commitSelection("@local");
  setMachineNotice({ text: `${alias} was removed from this Zerus. Its agents keep running and its files stay unchanged.` });
}

export function enableLocal() {
  setInstalling(true);
  setMachineNotice({ text: "Enabling local sessions on this computer…" });
  setTimeout(() => {
    setInstalling(false);
    setLocalSessions(true);
    setMachineNotice({ text: "Local sessions are enabled on this computer. Zerus runs the stable HGS CLI from its next start." });
  }, 1200);
}

export function installHgs(alias: string) {
  setInstalling(true);
  setLog([]);
  setMachineNotice({ text: `Installing on ${alias}… Keep Zerus running until setup finishes.` });
  INSTALL_LOG.forEach((line, i) => setTimeout(() => setLog((l) => [...l, line.replace("build-01", alias)]), 350 * (i + 1)));
  setTimeout(() => { setInstalling(false); setMachineNotice({ text: "HGS installed. Checking connection…" }); test(alias); }, 350 * (INSTALL_LOG.length + 1));
}

function test(alias: string) {
  setTesting(true);
  setMachineNotice({ text: "Testing SSH and checking HGS…" });
  setTimeout(() => {
    setTesting(false);
    const m = machineOf(alias);
    const c = peers.find((p) => p.alias === alias);
    if (!m || m.connection === "offline" || (demoState() === "error" && alias === "build-01"))
      setMachineNotice({ text: `SSH connection failed: ssh: connect to host ${c?.hostname || alias} port ${c?.effective?.port ?? 22}: Connection timed out\nOpen SSH terminal to check credentials or approve the host key, then test again.`, error: true });
    else if (!c?.tmux)
      setMachineNotice({ text: `SSH connected: ${c?.os ?? "Linux x86_64"}. Missing: tmux, Rust. Install the newest stable Rust and tmux in the SSH terminal if needed, then use Install / update HGS.`, error: true });
    else setMachineNotice({ text: `Connected: ${c.os}, hgs ${m.hgsVersion}, ${c.tmux}` });
  }, 900);
}

function stateLine(alias: string): string {
  const c = peers.find((p) => p.alias === alias);
  const m = machineOf(alias);
  if (c && !c.enabled) return "Disabled";
  if (!m || m.connection !== "online") return "Not connected";
  const n = sessionCount(alias);
  return n === 1 ? "Connected, 1 session" : `Connected, ${n} sessions`;
}

function MachineCard(props: { alias: string; local?: boolean }) {
  const key = () => (props.local ? "@local" : props.alias);
  const m = () => machineOf(props.alias);
  const live = () => props.local || m()?.connection === "online";
  return (
    <button class="mach-card" classList={{ on: selectedMachine() === key(), offline: !live() }} aria-current={selectedMachine() === key()} onClick={() => selectMachine(key())}>
      <span class="row">
        <MachineLabel alias={props.alias} />
        <span class="grow" />
        <Show when={m()}>
          <span class="mach-conn" classList={{ [m()!.connection]: true }} title={m()!.connection === "reconnecting" ? `Connecting… last answer ${m()!.lastSeen}` : m()!.connection === "offline" ? `Last answer ${m()!.lastSeen}` : "Connected"}>
            <Show when={m()!.connection === "reconnecting"} fallback={<Icon name={m()!.connection === "offline" ? "wifi-off" : "wifi"} size={12} />}><Spinner size={11} /></Show>
          </span>
        </Show>
      </span>
      <span class="mach-state">{props.local ? (isConsole() ? "This computer: control console" : "This machine") : stateLine(props.alias)}</span>
      <Show when={m() && !(props.local && isConsole())}>
        <span class="mach-meta tabular">
          {{ macos: "macOS", windows: "Windows", linux: "Linux" }[props.local ? platform : m()!.platform]} · hgs {m()!.hgsVersion}
          <Show when={!live()}> · last seen {m()!.lastSeen}</Show>
        </span>
      </Show>
    </button>
  );
}

function Field(props: { label: string; id: string; children: JSX.Element; hint?: string }) {
  return (
    <div class="form-row mach-form-row">
      <label for={props.id}>{props.label}</label>
      <div class="col" style={{ gap: "4px" }}>{props.children}<Show when={props.hint}><span class="hint">{props.hint}</span></Show></div>
    </div>
  );
}

function Editor() {
  const key = () => selectedMachine();
  const isNew = () => key() === "@new";
  const isLocal = () => key() === "@local";
  const c = () => peers.find((p) => p.alias === key());
  const busy = () => installing() || testing();
  const alias = () => (isLocal() ? LOCAL.alias : isNew() ? draft.alias.trim() : key());
  const validAlias = () => /^[A-Za-z0-9_.][A-Za-z0-9_.-]{0,79}$/.test(draft.alias.trim());
  const duplicate = () => isNew() && (draft.alias.trim() === LOCAL.alias || peers.some((p) => p.alias === draft.alias.trim()));
  const formHidden = () => isLocal() && (isConsole() || windowsLocal());
  const fieldsDisabled = () => busy() || isLocal();
  const eff = () => c()?.effective;
  const loading = () => demoState() === "loading";
  const effectiveError = () => demoState() === "error" && key() === "build-02";
  const edit = (field: "hostname" | "user" | "key" | "alias", value: string) => setDraft({ [field]: value, dirty: true });
  const save = () => {
    const name = draft.alias.trim();
    if (isNew()) {
      setPeers((list) => [...list, { alias: name, hostname: draft.hostname, user: draft.user, port: draft.port, key: draft.key, enabled: draft.enabled, source: "local", inherited: false }]);
    } else {
      const i = peers.findIndex((p) => p.alias === key());
      setPeers(i, { hostname: draft.hostname, user: draft.user, port: draft.port, key: draft.key, enabled: draft.enabled });
    }
    setSelectedMachine(name);
    setDraft("dirty", false);
    setMachineNotice({ text: `Connection saved for ${name}.` });
  };
  const inherited = (v: string | number | undefined, fallback: string) => (eff() && !draft.dirty ? `Inherited: ${v}` : fallback);
  return (
    <div class="mach-editor">
      <div class="mach-title">
        <Show when={!isNew()} fallback={<h2>New machine</h2>}>
          <h2>{alias()}</h2>
        </Show>
        <span class="grow" />
        <Show when={!isNew() && !isLocal()}><span class="chip" classList={{ "mach-chip-ok": machineOf(key())?.connection === "online" }}>{stateLine(key())}</span></Show>
      </div>
      <p class="muted mach-origin">
        <Show when={isLocal()} fallback={<>{c()?.source === "config" ? "This alias comes from HGS configuration. " : ""}SSH supplies inherited values from your OpenSSH configuration and defaults. Fields below are Zerus overrides; blank fields keep the inherited value.</>}>
          {isConsole()
            ? <>This computer is a control console: agents run on the remote machines you add here, over SSH.{windowsLocal() || demoState() === "empty" ? " Enable local sessions to run them on this computer too." : ""}</>
            : "This device runs Zerus. Add a remote machine to manage its agents over SSH."}
        </Show>
      </p>
      <Show when={isLocal() && windowsLocal() && !isConsole()}>
        <p class="mach-host selectable">Session host: running, PID 4120, 2 ConPTY, Medium integrity, Windows session 1</p>
      </Show>

      <section class="mach-section">
        <h3>Appearance</h3>
        <div class="row mach-appearance">
          <button class="btn" disabled={isNew()} onClick={() => setDialog("machines-color")}>
            <span class="mach-swatch" style={{ "--entity": labelColor(alias()) }} />Label color…
          </button>
          <select class="field" aria-label="Label style" disabled={isNew()} value={labels[alias()]?.vivid ? "bright" : "soft"}
            onChange={(e) => setLabels(alias(), { hex: labels[alias()]?.hex, vivid: e.currentTarget.value === "bright" })}>
            <option value="soft">Soft</option><option value="bright">Bright</option>
          </select>
          <button class="btn ghost" disabled={isNew()} title="Use the default color for this machine" onClick={() => setLabels(alias(), { hex: undefined, vivid: false })}>Automatic</button>
        </div>
        <div class="row mach-preview">
          <span class="dim">Preview</span>
          <Show when={!isNew()} fallback={<span class="entity mach-label" style={{ "--entity": "var(--z-entity-palette-7)" }}>new-machine</span>}><MachineLabel alias={alias()} /></Show>
          <span class="dim">{isNew() ? "Save the machine to choose its color" : "Saved on this device"}</span>
        </div>
      </section>

      <Show when={!formHidden()}>
        <section class="mach-section">
          <h3>Connection</h3>
          <Show when={!isNew() && !isLocal()}>
            <Show when={!loading()} fallback={<Status kind="busy">Reading effective SSH connection…</Status>}>
              <Show when={!effectiveError()} fallback={<Status kind="error"><Lines text={"Could not read effective SSH settings: ssh -G timed out\nYour connection overrides are shown below."} /></Status>}>
                <Show when={eff()}>
                  <div class="mach-effective selectable">
                    <Lines text={[
                      `${draft.dirty ? "Saved connection" : "Effective connection"}: ${eff()!.user}@${eff()!.hostname}, port ${eff()!.port}`,
                      `Key paths: ${eff()!.keys.join(", ")}`,
                      ...(eff()!.jump ? [`Jump host: ${eff()!.jump}`] : eff()!.proxyCommand ? ["Proxy command configured in OpenSSH"] : []),
                      ...(draft.dirty ? ["Unsaved edits are not included. Save to refresh these values."] : []),
                    ].join("\n")} />
                  </div>
                </Show>
              </Show>
            </Show>
          </Show>
          <Field label="Name / SSH alias" id="mach-alias">
            <input id="mach-alias" class="field" placeholder="e.g. studio-mac" value={draft.alias} readOnly={!isNew()} disabled={busy() || isLocal()} autofocus={isNew()}
              aria-invalid={isNew() && !!draft.alias && (!validAlias() || duplicate())} onInput={(e) => edit("alias", e.currentTarget.value)} />
            <Show when={duplicate()}><span class="danger hint">This name already exists. Select the machine in the list to edit it.</span></Show>
          </Field>
          <Field label="Hostname / IP" id="mach-host">
            <input id="mach-host" class="field" placeholder={inherited(eff()?.hostname, "Address or existing SSH alias")} value={draft.hostname} disabled={fieldsDisabled()} onInput={(e) => edit("hostname", e.currentTarget.value)} />
          </Field>
          <Field label="SSH user" id="mach-user">
            <input id="mach-user" class="field" placeholder={inherited(eff()?.user, "Use SSH configuration")} value={draft.user} disabled={fieldsDisabled()} onInput={(e) => edit("user", e.currentTarget.value)} />
          </Field>
          <Field label="Port" id="mach-port">
            <input id="mach-port" class="field tabular mach-port" type="number" min={0} max={65535} placeholder={inherited(eff()?.port, "SSH configuration (usually 22)")}
              value={draft.port || ""} disabled={fieldsDisabled()} onInput={(e) => setDraft({ port: Number(e.currentTarget.value) || 0, dirty: true })} />
          </Field>
          <Field label="Private key path" id="mach-key">
            <div class="row">
              <input id="mach-key" class="field mono grow" placeholder={inherited(eff()?.keys[0], "Use SSH agent / configuration")} value={draft.key} disabled={fieldsDisabled()} onInput={(e) => edit("key", e.currentTarget.value)} />
              <button class="btn" disabled={fieldsDisabled()} onClick={() => setDraft({ key: "~/.ssh/id_ed25519", dirty: true })} title="Choose SSH private key">Browse…</button>
            </div>
          </Field>
          <div class="form-row mach-form-row">
            <span />
            <label class="check"><input type="checkbox" checked={draft.enabled} disabled={fieldsDisabled()} onChange={(e) => setDraft({ enabled: e.currentTarget.checked, dirty: true })} />Show sessions and include in background polling</label>
          </div>
          <Actions>
            <button class="btn primary" disabled={busy() || isLocal() || !validAlias() || duplicate() || !(draft.dirty || isNew())}
              title={duplicate() ? "This name already exists. Select the machine in the list to edit it." : undefined} onClick={save}>Save connection</button>
            <Show when={isNew()}><button class="btn" disabled={busy()} onClick={() => commitSelection("@local")}>Discard draft</button></Show>
            <button class="btn" disabled={busy() || isLocal() || isNew() || draft.dirty} onClick={() => test(key())}><Icon name="plug" size={13} />Test connection</button>
            <button class="btn" disabled={busy() || isLocal() || isNew() || draft.dirty} onClick={() => toast("SSH terminal", `Opened a terminal: ssh ${key()}`)}><Icon name="square-terminal" size={13} />Open SSH terminal</button>
          </Actions>
        </section>
      </Show>

      <section class="mach-section">
        <h3>Setup</h3>
        <Actions>
          <Show when={isLocal() && isConsole()}>
            <button class="btn primary" disabled={busy()} title="Run agents on this computer too. Zerus stages the HGS CLI and a session host in your user profile." onClick={() => setDialog("machines-enable-local")}>Enable local sessions</button>
          </Show>
          <Show when={!formHidden()}>
            <button class="btn" disabled={busy() || isLocal() || isNew() || draft.dirty} onClick={() => setDialog("machines-install")}><Icon name="download" size={13} />Install / update HGS…</button>
          </Show>
          <Show when={!(isLocal() && isConsole())}>
            <button class="btn" disabled={busy() || isNew()} onClick={() => { setMachineFilter(isLocal() ? [LOCAL.alias] : [key()]); setPage("sessions"); }}><Icon name="rows-3" size={13} />View sessions</button>
            <button class="btn" disabled={busy() || isNew()} onClick={() => { setAccountMachine(isLocal() ? LOCAL.alias : key()); setPage("accounts"); }}><Icon name="user-round-cog" size={13} />Agent accounts…</button>
          </Show>
        </Actions>
        <Show when={machineNotice()}>
          <Status kind={machineNotice()!.error ? "error" : busy() ? "busy" : "muted"}><Lines text={machineNotice()!.text} /></Status>
        </Show>
        <Show when={log().length}>
          <pre class="mach-log" aria-label="Setup output">{log().join("\n")}</pre>
        </Show>
      </section>

      <Show when={!formHidden()}>
        <div class="mach-foot">
          <button class="btn" disabled={busy() || isLocal() || isNew() || draft.dirty || !(c()?.inherited && c()?.source === "local")}
            title="Restore the SSH settings of a machine from the shared configuration." onClick={() => {
              const i = peers.findIndex((p) => p.alias === key());
              setPeers(i, { hostname: "", user: "", port: 0, key: "", inherited: false });
              loadDraft(key());
              setMachineNotice({ text: `SSH overrides of ${key()} were reset to the shared configuration.` });
            }}>Reset SSH overrides</button>
          <button class="btn danger" disabled={busy() || isLocal() || isNew() || draft.dirty} onClick={() => setDialog("machines-remove")}><Icon name="trash" size={13} />Remove from Zerus</button>
        </div>
      </Show>
    </div>
  );
}

export function MachinesScreen() {
  const online = createMemo(() => (isConsole() ? 0 : 1) + visiblePeers().filter((p) => p.enabled && machineOf(p.alias)?.connection === "online").length);
  const saved = () => visiblePeers().length + (isConsole() ? 0 : 1);
  const [refreshing, setRefreshing] = createSignal(false);
  const refresh = () => {
    if (draft.dirty) { setPendingMachine(selectedMachine()); setDialog("machines-unsaved"); return; }
    setRefreshing(true); setTimeout(() => setRefreshing(false), 700);
  };
  const add = () => {
    if (selectedMachine() === "@new") { (document.getElementById("mach-alias") as HTMLInputElement | null)?.focus(); return; }
    if (draft.dirty) { setPendingMachine("@new"); setDialog("machines-unsaved"); return; }
    commitSelection("@new");
    queueMicrotask(() => (document.getElementById("mach-alias") as HTMLInputElement | null)?.focus());
  };
  return (
    <div class="mach-page">
      <div class="mach-shell">
        <header class="mach-head">
          <h1>Machines</h1>
          <button class="icon-btn" title="Refresh machines" aria-label="Refresh machines" disabled={installing()} onClick={refresh}>
            <Show when={!refreshing()} fallback={<Spinner />}><Icon name="refresh-cw" size={15} /></Show>
          </button>
          <span class="grow" />
          <button class="btn primary" disabled={installing()} onClick={add}><Icon name="plus" size={14} />Add machine</button>
        </header>
        <p class="muted mach-summary">{saved() === 1 ? `${online()} connected, 1 machine` : `${online()} connected, ${saved()} machines`}</p>
        <div class="mach-body">
          <div class="mach-list" role="list" aria-label="Machines">
            <MachineCard alias={LOCAL.alias} local />
            <For each={visiblePeers()}>{(p) => <MachineCard alias={p.alias} />}</For>
            <Show when={selectedMachine() === "@new"}>
              <button class="mach-card draft on" title="This connection is a draft. Save it to add the machine to Zerus." aria-current>
                <span class="row"><span class="entity mach-label" style={{ "--entity": "var(--z-entity-palette-7)" }}>New machine</span></span>
                <span class="mach-state">Unsaved connection</span>
              </button>
            </Show>
          </div>
          <Editor />
        </div>
        <div class="mach-phones"><PhonesPanel compact /></div>
      </div>
    </div>
  );
}
