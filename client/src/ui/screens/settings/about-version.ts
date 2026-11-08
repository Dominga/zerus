// UpdateController::installedVersion: what About says this build is, from
// the same facts `--build-info` prints (the app's `build_info`).

import { tr } from "../../../i18n";
import type { BuildInfo } from "../../../store/buildInfo";

/** A numbered or preview build, or one without its source, is its version
 *  name; any other names its source commit, and local changes to it. */
export function installedVersion(info: BuildInfo): string {
  const version = info.versionName ?? info.version ?? "";
  const commit = info.sourceCommit ?? "";
  if ((info.devBuildNumber ?? 0) > 0 || info.previewLabel !== undefined || !commit) return version;
  const source = commit.slice(0, 7);
  return info.sourceDirty ? tr("%1 (local changes %2)", version, source) : `${version} (${source})`;
}

export function installedAppName(info?: BuildInfo): string {
  const version = info ? installedVersion(info) : "";
  return version ? `Zerus ${version}` : "Zerus";
}

/** Keep full Stable versions; only a numbered native Dev identity gets Dev.
 * Build info currently has no packaged Nightly marker (zerus-dsk6q). */
export function railVersion(info?: BuildInfo): string {
  const version = info?.versionName ?? info?.version ?? "";
  return version && (info?.devBuildNumber ?? 0) > 0
    ? `${version.replace(/-dev\.\d+$/, "")}\nDev` : version;
}
