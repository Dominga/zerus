import type { ComputerState } from "../../store/catalog";
import { connectionOf } from "../../store/overview";
import type { Effective, LocalHost, MachineMeta, MachineRow, MachinesView } from "../../types";
import type { MachineState } from "../components/MachineTile";
import { tr } from "../../i18n";
import type { MachineConnection } from "../../store/machineConnection";

// Pure texts and enablement of the Machines screen (MachinesPage.cpp).

/** The selection key of this computer. */
export const LOCAL = "@local";

/** The Machines selection of a fleet computer: this computer's ID is "", its key is LOCAL. */
export const machineKey = (computer: string): string => (computer === "" ? LOCAL : computer);

export interface PeerState { tile: MachineState; line: string; live: boolean }

/** A saved peer's state line (renderRows, MachinesPage.cpp:308-311) with the stream's states of the Decisions note. */
export function peerState(row: MachineRow, computer: ComputerState | undefined, sessions: number): PeerState {
  if (!row.enabled) return { tile: "disabled", line: "Disabled", live: false };
  const c = connectionOf(computer?.status);
  if (c.fresh) return { tile: "online", line: sessions === 1 ? "Connected, 1 session" : tr("Connected, %1 sessions", sessions), live: true };
  return { tile: c.tile, line: c.text === "Offline" ? tr("Not connected") : c.text, live: false };
}

export interface LocalState {
  name: string;
  console: boolean;
  line: string;
  origin: string;
  /** A Windows control console whose CLI reports a session host (LocalSessionHost::canEnable). */
  canEnable: boolean;
  sessionHost?: string;
  /** The connection form and the peer actions are hidden (updateControls, 400-405). */
  formHidden: boolean;
}

/** MachinesPage::updateControls, 408-419. */
export function sessionHostLine(host: LocalHost): string {
  const state = host.state ?? "";
  const facts = [state === "not-installed" ? "not installed" : state];
  if (host.pid !== null) facts.push(`PID ${host.pid}`);
  if (host.conpty) facts.push(`${host.conpty} ConPTY`);
  if (host.integrity) facts.push(`${host.integrity} integrity`);
  if (host.windows_session !== null) facts.push(tr("Windows session %1", host.windows_session));
  return tr("Session host: %1%2", facts.join(", "), host.error ? `: ${host.error}` : "");
}

/** This computer (loadSelection, MachinesPage.cpp:328-336); `listedName` is `machine ls .local`. */
export function localState(meta: MachineMeta | undefined, listedName: string): LocalState {
  const console = meta?.local_sessions === false;
  const windows = meta?.platform === "windows";
  const canEnable = console && windows && !!meta?.local_host;
  return {
    name: meta?.host || listedName || tr("This machine"),
    console,
    line: console ? tr("This computer: control console") : tr("This machine"),
    origin: console
      ? tr("This computer is a control console: agents run on the remote machines you add here, over SSH.%1", canEnable ? " Enable local sessions to run them on this computer too." : "")
      : tr("This device runs Zerus. Add a remote machine to manage its agents over SSH."),
    canEnable,
    sessionHost: windows && meta?.local_host ? sessionHostLine(meta.local_host) : undefined,
    formHidden: console || windows,
  };
}

/** "N connected, M machines": this computer counts unless it is a control console (renderRows, 302-321). */
export function summary(local: { console: boolean; live: boolean }, peers: { live: boolean }[]): string {
  const online = (local.live && !local.console ? 1 : 0) + peers.filter((p) => p.live).length;
  const saved = peers.length + (local.console ? 0 : 1);
  return saved === 1 ? `${online} connected, 1 machine` : `${online} connected, ${saved} machines`;
}

/** The field placeholders: inherited values once the selected peer's resolve answered (showEffectiveConnection). */
export function placeholders(effective: Effective | null | undefined, alias: string) {
  const known = !!effective && effective.alias === alias && !effective.pending && !effective.error;
  return {
    hostname: known ? `Inherited: ${effective!.hostname}` : tr("Address or existing SSH alias"),
    user: known ? `Inherited: ${effective!.user}` : "Use SSH configuration",
    port: known ? `Inherited: ${effective!.port}` : "SSH configuration (usually 22)",
    key: known && effective!.key ? `Inherited: ${effective!.key}` : "Use SSH agent / configuration",
  };
}

/** Test connection: a saved peer is selected and nothing of the page runs (no draft or edits exist in 3a). */
export const testEnabled = (selected: string, view: MachinesView) =>
  selected !== LOCAL && view.loaded && !view.busy && view.machines.some((m) => m.alias === selected);

/**
 * The selected computer's connection state from its own supervisor, and what
 * it does next: the time left of its actual restart wait (`seconds`), an
 * attempt that runs now, or a wait an older service does not report.
 */
export function reconnectLine(conn: MachineConnection | undefined, seconds: number): { state: string; detail?: string; busy: boolean } {
  switch (conn?.status) {
    case "live": case "polled": return { state: tr("Connected"), busy: false };
    case "probing": return { state: tr("Checking the connection…"), busy: true };
    case "unsupported": return { state: tr("No event stream: update hgs on this machine"), busy: false };
    case "reconnecting": {
      const state = tr("Not connected");
      if (conn.retry?.phase === "attempting" || (conn.retry?.phase === "waiting" && seconds <= 0)) return { state, detail: tr("Connecting…"), busy: true };
      if (conn.retry?.phase === "waiting") return { state, detail: tr("Next attempt in %1 s", seconds), busy: false };
      return { state, detail: tr("Waiting to reconnect"), busy: false };
    }
    default: return { state: tr("Not connected"), busy: false };
  }
}

/** Reconnect now applies while a supervisor waits to reconnect; none for a connected, ended or unsupported computer. */
export const reconnectShown = (conn: MachineConnection | undefined) => conn?.status === "reconnecting" || conn?.status === "probing";
export const reconnectReady = (conn: MachineConnection | undefined) => conn?.status === "reconnecting" && conn.retry?.phase !== "attempting";

/** What a Reconnect now reply means for the person; null when the state itself shows it. */
export function reconnectOutcome(code: string): string | null {
  switch (code) {
    case "requested": case "connected": return null;
    case "pending": return tr("A connection attempt is already running.");
    case "source_changed": return tr("The connection changed meanwhile. Check its state and try again.");
    case "not_configured": case "not_admitted": return tr("The background service does not connect this machine yet.");
    case "stopping": case "fleet_unavailable": return tr("The background service is not running its connections.");
    case "fleet_request_unsupported": return tr("Update the background service to reconnect a machine now.");
    default: return tr("Could not reconnect: %1", code);
  }
}

/** The four reconnect delays a person typed: whole seconds from 1 to 3600, each at least the one before. */
export function retryDelaysDraft(values: readonly string[]): { delays?: [number, number, number, number]; error?: string } {
  const numbers = values.map((value) => (/^\s*\d+\s*$/.test(value) ? Number(value) : NaN));
  if (numbers.length !== 4 || numbers.some((n) => !Number.isInteger(n) || n < 1 || n > 3600)) return { error: tr("Use whole seconds from 1 to 3600.") };
  if (numbers.some((n, i) => i > 0 && n < numbers[i - 1])) return { error: tr("Each delay must be at least the one before.") };
  return { delays: numbers as [number, number, number, number] };
}
