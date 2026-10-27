import { Show, createSignal, createMemo } from "solid-js";
import { Icon } from "../../lib/icons";
import { demoState, setDialog, toast } from "../../state";
import { CONNECTOR_CONFIG, DELIVERY_OPTIONS, OFFICIAL_RELAY, WORKSPACE } from "../../mock/settings-extra";
import { prefs, setPrefs, platform } from "./store";
import { PhonesPanel } from "./Phones";
import { SectionHead, Group, Row, Select, Actions, Status, Lines } from "./ui";

// Settings → Mobile connection: the workspace (encrypted relay) panel, the
// legacy connector configuration, phones and the shared notification timing.

export const [workspaceChange, setWorkspaceChange] = createSignal<{ mode: "create" | "join"; relay: string; code: string } | undefined>();
export const [legacyPending, setLegacyPending] = createSignal(false);
const [connector, setConnector] = createSignal<"running" | "draining" | "stopped">("running");
const [notice, setNotice] = createSignal("");

function validRelay(url: string) {
  try {
    const u = new URL(url);
    return u.protocol === "https:" && !u.username && !u.password && !u.search && !u.hash;
  } catch { return false; }
}

function WorkspacePanel() {
  const state = () => (demoState() === "loading" ? "checking" : demoState() === "empty" ? "none" : demoState() === "error" ? "legacy" : "encrypted");
  const [changing, setChanging] = createSignal(false);
  const [mode, setMode] = createSignal<"create" | "join">("create");
  const [relay, setRelay] = createSignal(OFFICIAL_RELAY);
  const [code, setCode] = createSignal("");
  const [invite, setInvite] = createSignal<"none" | "ready">("none");
  const [codeStatus, setCodeStatus] = createSignal("");
  const enrolled = () => state() === "encrypted" || state() === "legacy";
  const formOpen = () => state() === "none" || changing();
  const relayError = () => (validRelay(relay()) ? "" : "Enter an HTTPS relay URL without credentials, query or fragment.");
  const codeError = () => (mode() === "join" && !(code().startsWith("ze1.") && code().length <= 4096) ? "Enter a computer invitation code from an enrolled computer." : "");
  const submitLabel = () => (enrolled() ? (mode() === "join" ? "Change and join workspace" : "Change and create workspace") : mode() === "join" ? "Join workspace" : "Create workspace");
  const submit = () => {
    if (relayError() || codeError()) return;
    if (enrolled()) { setWorkspaceChange({ mode: mode(), relay: relay(), code: code() }); setDialog("settings-workspace-change"); return; }
    setNotice("Workspace connected.");
    toast("Workspace", mode() === "join" ? "Joined the workspace." : "Workspace created.");
  };
  const summary = createMemo(() => {
    switch (state()) {
      case "checking": return { text: "Workspace: checking…", sub: "" };
      case "none": return { text: "No workspace connected", sub: "Create a workspace or use a computer invitation from an enrolled computer." };
      case "legacy": return { text: "Legacy connection: session content is visible to this relay.", sub: "Choose Change workspace to create or join an encrypted workspace." };
      default: return { text: `${WORKSPACE.name} · ${WORKSPACE.computer}\nPrivate session content is encrypted.`, sub: "" };
    }
  });
  return (
    <Group class="ws-panel">
      <div class="ws-summary">
        <span class="ws-icon" classList={{ ok: state() === "encrypted", warn: state() === "legacy" }}>
          <Icon name={state() === "encrypted" ? "lock" : state() === "legacy" ? "triangle-alert" : "network"} size={18} />
        </span>
        <div class="col grow" style={{ gap: "2px" }}>
          <Show when={state() !== "checking"} fallback={<Status kind="busy">Workspace: checking…</Status>}>
            <Lines text={summary().text} class="ws-title" />
          </Show>
          <Show when={summary().sub}><span class="muted">{summary().sub}</span></Show>
          <Show when={enrolled()}>
            <span class="muted">{connector() === "running" ? "Connector: running" : connector() === "draining" ? "Connector: draining its current request" : "Connector: stopped"}</span>
          </Show>
        </div>
      </div>
      <Actions>
        <Show when={enrolled() && !changing()}>
          <button class="btn" onClick={() => setChanging(true)}>Change workspace…</button>
        </Show>
        <button class="btn" disabled={!enrolled() || connector() === "running"} onClick={() => { setConnector("running"); setNotice(""); }}><Icon name="play" size={13} />Start connector</button>
        <button class="btn" disabled={!enrolled() || connector() !== "running"} onClick={() => { setConnector("draining"); setNotice("The connector is draining its current request. Refresh status to confirm it has stopped."); }}><Icon name="square" size={12} />Stop connector</button>
        <button class="btn" onClick={() => { if (connector() === "draining") { setConnector("stopped"); setNotice("Connector stopped."); } }}><Icon name="refresh-cw" size={13} />Refresh status</button>
      </Actions>

      <Show when={formOpen()}>
        <div class="ws-enroll">
          <Row label="Connection" for="ws-mode">
            <Select id="ws-mode" value={mode()} onChange={(v) => setMode(v as "create" | "join")} options={[{ value: "create", label: "Create workspace" }, { value: "join", label: "Join by code" }]} />
          </Row>
          <Row label="Relay (HTTPS)" for="ws-relay" hint={relayError() ? <span class="danger">{relayError()}</span> : undefined}>
            <input id="ws-relay" class="field mono set-wide-field" value={relay()} onInput={(e) => setRelay(e.currentTarget.value)} aria-invalid={!!relayError()} />
          </Row>
          <Show when={mode() === "join"}>
            <Row label="Computer invitation code" for="ws-code" hint={code() && codeError() ? <span class="danger">{codeError()}</span> : undefined}>
              <input id="ws-code" class="field mono set-wide-field" type="password" maxLength={4096} placeholder="ze1.…" value={code()} onInput={(e) => setCode(e.currentTarget.value)} />
            </Row>
          </Show>
          <Actions>
            <button class="btn primary" disabled={!!relayError() || !!codeError()} onClick={submit}>{submitLabel()}</button>
            <Show when={changing()}><button class="btn ghost" onClick={() => setChanging(false)}>Cancel</button></Show>
          </Actions>
        </div>
      </Show>

      <Show when={legacyPending()}>
        <div class="notice warn">
          <Icon name="triangle-alert" size={14} />
          <div class="col" style={{ gap: "8px" }}>
            <span>The new encrypted workspace is ready. The old relay has not confirmed retirement. You can explicitly disconnect its local configuration before switching.</span>
            <Actions>
              <button class="btn small" onClick={() => toast("Workspace", "The old relay has not confirmed retirement. The new connection is saved; your current configuration is unchanged.")}>Continue connecting</button>
              <button class="btn small danger" onClick={() => setDialog("settings-workspace-legacy")}>Disconnect legacy locally and switch…</button>
            </Actions>
          </div>
        </div>
      </Show>

      <Show when={state() === "encrypted"}>
        <div class="ws-invite">
          <Actions>
            <button class="btn" disabled={invite() === "ready"} onClick={() => { setInvite("ready"); setCodeStatus("Computer code ready until 16:12. Keep this computer online while the other computer joins."); }}><Icon name="key-round" size={13} />Give computer code</button>
            <button class="btn" disabled={invite() !== "ready"} onClick={() => toast("Copied", "Computer code copied. It stays a credential until used or expired.")}><Icon name="copy" size={13} />Copy computer code</button>
            <button class="btn" disabled={invite() !== "ready"} onClick={() => { setInvite("none"); setCodeStatus(""); setNotice("Code canceled. Computers already joined keep their access."); }}>Cancel code</button>
          </Actions>
          <Show when={codeStatus()}><Status kind="ok">{codeStatus()}</Status></Show>
        </div>
      </Show>
      <Show when={notice()}><Status>{notice()}</Status></Show>
    </Group>
  );
}

function LegacyRelay() {
  const [relay, setRelay] = createSignal("");
  const [saved, setSaved] = createSignal("");
  const copy = (what: string) => toast("Copied", `${what} copied to the clipboard.`);
  return (
    <Group title="Mobile connection" class="set-legacy" hint="The computer connector uses this private configuration. Saving does not restart it or change the active connection.">
      <p class="muted mono small-path">Configuration: {CONNECTOR_CONFIG}</p>
      <Status>Configured relay: {OFFICIAL_RELAY}</Status>
      <Status kind="ok">Connector service: running. Relay connectivity is not verified here.</Status>
      <p class="muted">Implementation: Rust connector (zerus-mobile-connector 0.48.0).</p>
      <Row label="Relay (HTTPS)" for="lr-relay"><input id="lr-relay" class="field mono set-wide-field" placeholder={OFFICIAL_RELAY} value={relay()} onInput={(e) => setRelay(e.currentTarget.value)} /></Row>
      <Row label="Replacement computer credential" for="lr-cred" hint="The configured computer credential stays hidden. Leave replacement blank to retain it for this relay.">
        <input id="lr-cred" class="field set-wide-field" type="password" placeholder="Leave blank to keep the configured credential" />
      </Row>
      <Row label="Private receipt directory" for="lr-receipts">
        <input id="lr-receipts" class="field mono set-wide-field" value="~/.local/state/zerus-mobile/receipts-3" />
      </Row>
      <p class="set-group-hint">The official relay migration keeps the existing credential and receipts. A different relay or replacement credential requires a new, unused private receipt directory. Old receipts are never copied or deleted.</p>
      <Actions>
        <button class="btn" onClick={() => setRelay(OFFICIAL_RELAY)}>Use official relay</button>
        <button class="btn" onClick={() => toast("Receipt directory", "~/.local/state/zerus-mobile/receipts-4 (new, unused)")}>New receipt directory</button>
        <button class="btn primary" onClick={() => setSaved("Configuration saved privately. Changes apply when the connector restarts. Service status does not verify relay connectivity or that this configuration is active.")}>Save configuration</button>
        <button class="btn" onClick={() => { setRelay(""); setSaved(""); }}>Reload configuration</button>
      </Actions>
      <Show when={saved()}><Status kind="ok">{saved()}</Status></Show>
      <p class="set-group-hint">Changes apply when the connector restarts. Restart only after pending connector requests and receipts have drained.</p>
      <p class="set-group-hint">A restart stops only the connector process (KillMode=process). Native agents keep running.</p>
      <Actions>
        <button class="btn small" onClick={() => copy("Restart command")}><Icon name="copy" size={12} />Copy restart command</button>
        <button class="btn small" onClick={() => copy("Start command")}><Icon name="copy" size={12} />Copy start command</button>
        <button class="btn small" onClick={() => copy("Protection command")}><Icon name="copy" size={12} />Copy protection command</button>
      </Actions>
      <p class="dim">Connector setup is documented in docs/mobile-deployment.md in the Zerus source checkout.</p>
    </Group>
  );
}

function WindowsMobile() {
  const [state, setState] = createSignal<"running" | "stopped" | "stopping">("stopped");
  const [msg, setMsg] = createSignal("");
  return (
    <Group title="Mobile connection" class="set-legacy" hint="Connect this Windows PC directly to the relay. Enable local sessions in Machines to control its Codex, Claude and Kimi sessions from a phone.">
      <p class="set-group-hint">Enter the computer credential issued by your relay. Saving does not start the connector. Start and Stop affect only the connector; closing Zerus keeps it and native sessions running.</p>
      <Row label="Relay (HTTPS)" for="wm-relay"><input id="wm-relay" class="field mono set-wide-field" value={OFFICIAL_RELAY} /></Row>
      <Row label="Replacement computer credential" for="wm-cred"><input id="wm-cred" class="field set-wide-field" type="password" placeholder="Leave blank to keep the configured credential" /></Row>
      <Status kind={state() === "running" ? "ok" : "muted"}>{state() === "running" ? "Connector: running. Relay connectivity is not verified here." : state() === "stopping" ? "Stop requested. The current phone request will finish before the connector exits. Reload status to confirm." : "Connector: stopped"}</Status>
      <Actions>
        <button class="btn primary" onClick={() => setMsg("Configuration saved privately. Start the connector, then refresh Phones below.")}>Save configuration</button>
        <button class="btn" disabled={state() !== "stopped"} onClick={() => setState("running")}>Start connector</button>
        <button class="btn" disabled={state() !== "running"} onClick={() => setState("stopping")}>Stop connector</button>
        <button class="btn" onClick={() => { if (state() === "stopping") setState("stopped"); }}>Reload status</button>
      </Actions>
      <Show when={msg()}><Status kind="ok">{msg()}</Status></Show>
      <p class="set-group-hint">Start the connector after each Windows login. It uses a private receipt journal. Stop waits for its current phone request; reload status to confirm it has stopped before changing configuration.</p>
      <p class="set-group-hint">This Windows gateway controls existing local sessions. Mobile session creation, folder/worktree operations, peer routing, DeepSeek, OpenCode and voice are unavailable.</p>
    </Group>
  );
}

function PhoneTiming() {
  const [status, setStatus] = createSignal("");
  const change = (v: string) => {
    setPrefs("delivery", v);
    setStatus("Saving…");
    setTimeout(() => setStatus("Saved. Your other computers and the phone receive it with the next sync."), 700);
  };
  return (
    <Group title="Phone notifications">
      <Row label="When I'm not at a computer, send notifications to my phone" for="pn-timing"
        hint="You count as away after two minutes without keyboard or mouse input on every computer running Zerus, or while their screens are locked. Your computers and the phone share this choice.">
        <Select id="pn-timing" value={prefs.delivery} onChange={change} options={DELIVERY_OPTIONS.map((o) => ({ value: o.id, label: o.label }))} />
      </Row>
      <Show when={demoState() !== "empty"}>
        <div class="notice warn"><Icon name="triangle-alert" size={14} /><span>Update Zerus on win-lab, a computer this one synchronizes with directly. Until then it keeps syncing projects but does not receive this choice.</span></div>
      </Show>
      <Show when={status()}><Status kind={status() === "Saving…" ? "busy" : "ok"}>{status()}</Status></Show>
    </Group>
  );
}

export function MobileSection() {
  const [legacyOpen, setLegacyOpen] = createSignal(false);
  // The legacy section is hidden while the workspace is encrypted (SettingsPage.h:126-135).
  const encrypted = () => demoState() === "normal";
  return (
    <>
      <SectionHead title="Workspace" intro="Connect your computers and phones to one workspace. Session content is encrypted between your devices; the relay keeps routing and delivery metadata." />
      <WorkspacePanel />
      <button class="btn ghost set-disclosure" disabled={encrypted()} aria-expanded={legacyOpen() && !encrypted()} onClick={() => setLegacyOpen(!legacyOpen())}
        title={encrypted() ? "Unavailable while this computer uses an encrypted workspace" : undefined}>
        <Icon name={legacyOpen() && !encrypted() ? "chevron-down" : "chevron-right"} size={14} />Advanced: legacy connector configuration
      </button>
      <Show when={legacyOpen() && !encrypted()}>
        {platform === "windows" ? <WindowsMobile /> : <LegacyRelay />}
      </Show>
      <Group><PhonesPanel /></Group>
      <PhoneTiming />
    </>
  );
}
