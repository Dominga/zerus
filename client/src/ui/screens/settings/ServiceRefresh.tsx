import { Show, createEffect } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { desktopService, checkDesktopServiceStatus } from "../../../store/desktopService";
import { inTauri } from "../../../bridge";
import { tr } from "../../../i18n";
import { toast } from "../../../store/ui";
import { Actions, Group, Status } from "./ui";
import type { ServiceRefreshView } from "../../../types";
import { createServiceRefresh } from "./service-refresh";

/** Display only. Only a native dialog can create the refresh continuation. */
export function ServiceRefresh() {
  const manual = () => desktopService.state()?.executable_upgrade?.state === "manual_refresh_required";
  const { view, busy, check, submit } = createServiceRefresh(() => inTauri() && manual(),
    (command, args) => invoke<ServiceRefreshView>(command, args),
    () => toast(tr("Background service"), tr("The refresh result is unconfirmed. Check the original result before taking another action."), "error", true));
  let observed = "";
  // Observation only; Retry/reconnect/Resume cannot invoke the operation.
  createEffect(() => {
    const epoch = manual() ? desktopService.state()?.epoch : null;
    if (epoch && epoch !== observed) { observed = epoch; void check(); }
  });
  return <Show when={manual()}>
    <Group title={tr("Service refresh required")}>
      <Status kind={view()?.settled ? "ok" : "warn"}>{view()?.settled
        ? tr("The background service was refreshed.")
        : tr("The CLI was updated. The Windows background service needs a manual refresh.")}</Status>
      <Show when={!view()?.settled} fallback={<p class="set-group-hint">{tr("Close and reopen Zerus to connect to the refreshed service. Background work stays paused in this window.")}</p>}>
        <p class="set-group-hint">{tr("Refresh starts the updated CLI in the same service mode after the original service has drained. It does not grant or restore background access.")}</p>
        <Show when={view()?.request_id || view()?.code === "refresh_unconfirmed"} fallback={<Show when={!view()?.offer}>
          <p class="set-group-hint">{tr("Refresh is unavailable without current background access from this Zerus installation. Revoked or changed access needs separate recovery; this build cannot refresh that service.")}</p>
        </Show>}>
          <p class="set-group-hint">{tr("The refresh result is unconfirmed. Check the original result before taking another action.")}</p>
        </Show>
      </Show>
      <details>
        <summary>{tr("Technical details")}</summary>
        <dl class="selectable">
          <dt>{tr("Original service")}</dt><dd><code>{desktopService.state()?.epoch}</code></dd>
          <Show when={view()?.configuration ?? view()?.offer?.configuration}>{(configuration) => <>
            <dt>{tr("Configuration")}</dt><dd><code title={configuration()}>{configuration().slice(0, 12)}…</code></dd>
          </>}</Show>
          <Show when={view()?.request_id}>{(request) => <>
            <dt>{tr("Original request")}</dt><dd><code>{request()}</code></dd>
          </>}</Show>
          <Show when={view()?.code}><dt>{tr("Status")}</dt><dd><code>{view()?.code}</code></dd></Show>
        </dl>
      </details>
      <Actions>
        <Show when={view()?.offer && !view()?.request_id}>
          <button type="button" class="btn" disabled={busy() || view()?.running} onClick={() => void submit()}>{tr("Refresh background service…")}</button>
        </Show>
        <Show when={!view()?.settled}>
          <button type="button" class="btn" disabled={busy() || view()?.running} onClick={() => void check()}>{view()?.request_id || view()?.code === "refresh_unconfirmed" ? tr("Check result") : tr("Check refresh availability")}</button>
        </Show>
        <button type="button" class="btn" disabled={busy() || !desktopService.state()?.epoch} onClick={() => {
          const epoch = desktopService.state()?.epoch;
          if (epoch) void checkDesktopServiceStatus(epoch).catch(error => toast(tr("Background service"), String(error), "error", true));
        }}>{tr("Check service status")}</button>
      </Actions>
      <Show when={!view()?.settled && !view()?.offer}>
        <p class="set-group-hint">{tr("Keep the selected service mode and the original request. To inspect the registration with the same CLI and configuration:")}</p>
        <p class="selectable"><code>hgs desktop registration status --json</code></p>
      </Show>
    </Group>
  </Show>;
}
