import type { TeamProfileView } from "../../types";

// Task #226: Qt's TeamProfileView::exportJson, adapted to the browser: the
// profile mode's Export JSON saves the same object
// (`{team, scope, profile, spans?, compare?}`) as an indented JSON file. The
// webview cannot choose a path, so the file goes to the browser's downloads
// under the name Qt's save dialog proposes.

export interface ProfileExport {
  team: string;
  scope: string;
  profile?: TeamProfileView;
  spans?: unknown;
  compare?: unknown;
}

/** What the export holds: the team, the scope it was read at, and its profile
 * (with the time view's spans and a comparison when the view has them). */
export function profileExport(team: string, scope: string, profile: TeamProfileView | null | undefined, extra: { spans?: unknown; compare?: unknown } = {}): ProfileExport {
  const out: ProfileExport = { team, scope };
  if (profile) out.profile = profile;
  if (extra.spans !== undefined) out.spans = extra.spans;
  if (extra.compare !== undefined) out.compare = extra.compare;
  return out;
}

/** Qt's proposed file name: profile-<team8>-<scope>.json. */
export function profileFileName(team: string, scope: string): string {
  const short = team.slice(0, 8);
  return `profile-${short}-${scope.replace(/[^A-Za-z0-9_-]+/g, "-")}.json`;
}

/** Save `value` as indented JSON under `name` (the browser's downloads). */
export function downloadJson(name: string, value: unknown): void {
  const blob = new Blob([JSON.stringify(value, null, 2) + "\n"], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  try {
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = name;
    anchor.rel = "noopener";
    document.body.appendChild(anchor);
    anchor.click();
    anchor.remove();
  } finally {
    URL.revokeObjectURL(url);
  }
}
