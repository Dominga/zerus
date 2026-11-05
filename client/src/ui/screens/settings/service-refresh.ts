import { createSignal } from "solid-js";
import type { ServiceRefreshView } from "../../../types";

type Command = "desktop_service_refresh_status" | "desktop_service_refresh";
/** Native state owns the original UUID. A failed or lost invoke closes the old
 * offer; only the read command can recover it. No automatic retry of effects. */
export function createServiceRefresh(
  available: () => boolean,
  send: (command: Command, args?: { display: NonNullable<ServiceRefreshView["offer"]> }) => Promise<ServiceRefreshView>,
  report: (error: unknown) => void,
) {
  const [view, setView] = createSignal<ServiceRefreshView>();
  const [busy, setBusy] = createSignal(false);
  const check = async () => {
    if (!available() || busy()) return;
    setBusy(true);
    try { setView(await send("desktop_service_refresh_status")); }
    catch { setView({ code: "refresh_unavailable", running: false, settled: false, offer: null, request_id: view()?.request_id }); }
    finally { setBusy(false); }
  };
  const submit = async () => {
    const display = view()?.offer;
    if (!available() || !display || busy() || view()?.request_id || view()?.running) return;
    setBusy(true);
    try { setView(await send("desktop_service_refresh", { display })); }
    catch (error) {
      setView({ code: "refresh_unconfirmed", running: false, settled: false, offer: null, request_id: view()?.request_id });
      report(error);
    } finally { setBusy(false); }
  };
  return { view, busy, check, submit };
}
