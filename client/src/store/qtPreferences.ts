import { invoke } from "@tauri-apps/api/core";
import { tr } from "../i18n";
import { inTauri } from "../bridge";

/** One Qt preference family as the Settings import recorded it: key names
 * only, never a value (`settings_qt_preferences`, zerus-5wmh.66.5.3). */
export interface QtPreferenceFamily {
  family: string;
  state: "imported" | "waiting";
  keys: string[];
  /** Absent for a family handled before the split was recorded. */
  imported?: string[] | null;
  kept?: string[] | null;
  reason?: string;
}
export interface QtPreferenceReport { families: QtPreferenceFamily[]; unused: string[] }

/** One line of the Migration report's Qt preferences section. */
export interface QtPreferenceRow { label: string; status: string; keys: string[]; reason?: string }

/** The local, read-only import record; undefined outside the desktop app. */
export function loadQtPreferences(): Promise<QtPreferenceReport | undefined> {
  return inTauri() ? invoke<QtPreferenceReport>("settings_qt_preferences") : Promise.resolve(undefined);
}

const FAMILY_LABELS: Record<string, () => string> = {
  "machine-appearance": () => tr("Computer colors"),
  "project-view": () => tr("All swarm projects"),
  "row-density": () => tr("Session row density"),
  "task-preferences": () => tr("Tasks of this session only"),
  "termination-default": () => tr("Remove the worktree after Terminate"),
  "project-vivid": () => tr("Project colors"),
  "window-geometry": () => tr("Window size and position"),
  "sessions-width": () => tr("Sessions list width"),
  "session-splitter": () => tr("Sessions list width"),
  "project-folder-splitter": () => tr("Project folders width"),
  "inspector-panel": () => tr("Side panel"),
  "cache-keep-warm": () => tr("Keep cache warm"),
};

/** The section's lines: imported keys, keys that kept a Zerus choice, families
 * waiting to retry with their reason, and Qt keys with no Zerus equivalent. */
export function qtPreferenceRows(report: QtPreferenceReport | undefined): QtPreferenceRow[] {
  if (!report) return [];
  const rows: QtPreferenceRow[] = [];
  for (const family of report.families ?? []) {
    const label = FAMILY_LABELS[family.family]?.() ?? family.family;
    if (family.state === "waiting") {
      rows.push({ label, status: tr("Waiting to retry"), keys: family.keys ?? [], reason: family.reason || undefined });
    } else if (family.imported || family.kept) {
      if (family.imported?.length) rows.push({ label, status: tr("Imported"), keys: family.imported });
      if (family.kept?.length) rows.push({ label, status: tr("Kept your Zerus choice"), keys: family.kept });
    } else if (family.keys?.length) {
      rows.push({ label, status: tr("Imported earlier"), keys: family.keys });
    }
  }
  if (report.unused?.length) rows.push({ label: tr("Other preferences"), status: tr("Not imported: no Zerus equivalent"), keys: report.unused });
  return rows;
}
