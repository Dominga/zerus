// Settings > Mobile connection: what automatic connector start says. The native
// side decides the line; this only words it.
import { tr } from "../../../i18n";

export type AutostartLine =
  | { kind: "running" | "running_not_armed" | "starting" | "will_start" | "not_armed" | "stopped_by_you" | "unavailable" }
  | { kind: "failed"; reason: string };

export interface AutostartStatus { kind: "ok" | "warn" | "muted" | "busy"; text: string }

export function autostartStatus(line?: AutostartLine | null): AutostartStatus | undefined {
  switch (line?.kind) {
    case "running": return { kind: "ok", text: tr("Running. Starts automatically when Zerus starts.") };
    case "running_not_armed": return { kind: "muted", text: tr("Running. It won't start automatically until you stop it and choose Start connector.") };
    case "starting": return { kind: "busy", text: tr("Starting automatically…") };
    case "will_start": return { kind: "muted", text: tr("Stopped. It starts the next time Zerus starts.") };
    case "not_armed": return { kind: "muted", text: tr("Stopped. Choose Start connector once and Zerus will start it automatically from then on.") };
    case "stopped_by_you": return { kind: "muted", text: tr("Stopped by you. It stays stopped until you choose Start connector.") };
    case "unavailable": return { kind: "warn", text: tr("Automatic start is unavailable: this Zerus has no packaged connector.") };
    case "failed": return { kind: "warn", text: tr("Automatic start failed: %1", line.reason) };
    default: return undefined;
  }
}

export function autostartSaveWarning(failed?: boolean): string | undefined {
  return failed ? tr("Zerus could not save this choice, so the connector may start again the next time Zerus starts.") : undefined;
}
