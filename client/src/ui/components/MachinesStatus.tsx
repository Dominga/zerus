import { Show, createMemo } from "solid-js";
import { fleetRefresh, type BridgeMode } from "../../bridge";
import { catalog } from "../../store/catalog";
import { desktopService, firstChoicePending, servicePaused, serviceStatus } from "../../store/desktopService";
import { setPage, toast } from "../../store/ui";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";

function stateText(status: string) {
  switch (status) {
    case "probing": return tr("checking");
    case "unsupported": return tr("no event stream (update hgs)");
    case "polled": return tr("polled every 60 s (update hgs for live updates)");
    case "reconnecting": return tr("reconnecting");
    case "ended": return tr("stream ended");
    default: return status;
  }
}

/** The computers the connection line reads: the service's roster when it has one. */
function connectionComputers() {
  // The catalog has no row for a peer that has never greeted. Use the native
  // checked display roster when available; older/native clients keep theirs.
  const service = desktopService.state();
  return service?.connected && service.fleet_computers
    ? service.fleet_computers : Object.values(catalog.computers);
}
const connectedStatus = (status: string) => status === "live" || status === "polled";
const connectionLabel = (id: string) => id || tr("this computer");

/**
 * Task 409 (owner, 2026-11-08): the connection line the client log records and
 * the title bar's log line shows, as Qt writes each change of its status line:
 * the offline computers by name, else the calm count.
 */
export function connectionLine(): string {
  const computers = connectionComputers();
  if (!computers.length) return "";
  const problems = computers.filter((c) => !connectedStatus(c.status));
  if (!problems.length) return tr("%1 of %2 machines connected", computers.length, computers.length);
  return problems.map((c) => `${connectionLabel(c.id)}: ${stateText(c.status)}${c.status === "reconnecting" && c.attempt ? ` (${tr("attempt %1", c.attempt)})` : ""}`).join("; ");
}

/** Connection presentation only; opening Machines never retries a connection. */
export function MachinesStatus(props: { mode: () => BridgeMode | "starting" }) {
  const computers = createMemo(connectionComputers);
  const connected = connectedStatus;
  const live = createMemo(() => computers().filter((c) => connected(c.status)).length);
  const problems = createMemo(() => computers().filter((c) => !connected(c.status)));
  const offline = createMemo(() => problems().filter((c) => c.status !== "probing"));
  const label = connectionLabel;
  const connectionError = () => offline().length > 0;
  const details = createMemo(() => [
    tr("%1 of %2 machines connected", live(), computers().length)
      + (offline().length ? tr(", %1 offline", offline().map((c) => label(c.id)).join(", ")) : ""),
    ...problems().map((c) => `${label(c.id)}: ${stateText(c.status)}${c.status === "reconnecting" && c.attempt ? ` (${tr("attempt %1", c.attempt)})` : ""}`),
  ].join("\n"));
  const accessible = () => `${tr("Open Machines")}\n${details()}`;
  return <Show when={computers().length} fallback={<Show when={!servicePaused(desktopService.state()) && !firstChoicePending(desktopService.state())}><span>{props.mode() === "starting" ? tr("Starting…") : tr("Connecting…")}</span></Show>}>
    <button type="button" class="machines-status" classList={{ warn: connectionError() }}
      title={accessible()} aria-label={accessible()} onClick={() => setPage("machines")}>
      <Icon name="monitor" size={14} /><span class="tabular" aria-hidden="true">{live()}/{computers().length}</span>
    </button>
    <Show when={connectionError() && !serviceStatus(desktopService.state())?.retry}>
      <button type="button" class="btn small ghost" onClick={() => { void fleetRefresh().catch((error) => toast(tr("Connection"), String(error), "error", true)); }}>{tr("Retry now")}</button>
    </Show>
  </Show>;
}

/** Configuration is captured by the service process; this notice has no effect action. */
export function PeerConfigurationNotice() {
  return <Show when={desktopService.enabled()}><p class="muted">{tr("Restart the background service after changing HGS_PEERS. Existing sessions keep running.")}</p></Show>;
}
