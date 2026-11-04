import { invoke } from "@tauri-apps/api/core";
import { inTauri } from "../bridge";
import { tr } from "../i18n";

const REASONS = {
  legacy_projects_invalid: () => tr("The legacy project list could not be verified. The source is kept."),
  legacy_projects_capacity: () => tr("The project import exceeds its safety limit. Review the existing projects manually."),
  legacy_projects_changed: () => tr("The computer or project catalog changed. The original import is kept for review."),
  legacy_projects_history: () => tr("Earlier project changes prevent automatic import. Review the existing projects manually."),
  legacy_projects_invalid_folders: () => tr("Some legacy folders are invalid. Existing projects and the source are kept."),
  legacy_projects_windows_unsupported: () => tr("Legacy Windows folders require device-local import and were not shared."),
  legacy_projects_read_blocked: () => tr("The fresh project catalog or computer configuration could not be verified. Reads retry automatically; the original import will not be applied again."),
  legacy_projects_pending: () => tr("The original import has not been confirmed. It will not be applied again."),
};
export interface LegacyProjectRow { status: string; keys: string[]; reasons: { code: string; text: string }[] }

/** Read-only, bounded native report. Source values never cross this boundary. */
export function loadLegacyProjects(): Promise<unknown> {
  return inTauri() ? invoke("settings_legacy_projects") : Promise.resolve(undefined);
}

/** Accept the closed report vocabulary only. Never display arbitrary source
 * names, paths, error text or future fields as report labels or translations. */
export function legacyProjectRow(value: unknown): LegacyProjectRow | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) return;
  const report = value as Record<string, unknown>;
  if (report.family !== "legacy-projects" || !["empty", "waiting", "blocked", "imported"].includes(String(report.state))) return;
  if (!Array.isArray(report.keys) || report.keys.length > 2 || report.keys.some(k => k !== "projects" && k !== "folders") || new Set(report.keys).size !== report.keys.length) return;
  if (!Array.isArray(report.reasons) || report.reasons.length > Object.keys(REASONS).length || report.reasons.some(c => typeof c !== "string" || !Object.hasOwn(REASONS, c)) || new Set(report.reasons).size !== report.reasons.length) return;
  if (report.state === "empty") return;
  if (report.keys.length !== 2) return;
  const status = report.state === "imported" ? tr("Imported") : report.state === "waiting" ? tr("Waiting for confirmation") : tr("Needs review");
  return { status, keys: report.keys as string[], reasons: (report.reasons as (keyof typeof REASONS)[]).map(code => ({ code, text: REASONS[code]() })) };
}

/** Translate the one fixed native recovery status, preserving other statuses. */
export function legacyProjectStatus(status: string): string {
  return status === "Project import verification is blocked. Retrying reads automatically."
    ? tr("Project import verification is blocked. Retrying reads automatically.") : status;
}
