import type { Session } from "../ui/sessions/types";
import type { GitOut } from "../types";

export const GIT_ROWS = 48;
export const gitPath = (s: Session) => s.state === "archived" || s.gitMetadataState === "not_repo"
  ? "" : s.gitRoot || s.cwdCanonical || s.cwdPath;
const folderKey = (s: Session) => JSON.stringify([s.computer, gitPath(s)]);
type Facts = { value: Record<string, unknown>; at: number };

/** Only the currently visible/selected rows retain Git display facts. */
export function createGitProjection(port: {
  row(id: string): Session | undefined;
  online(computer: string): boolean;
  patch(id: string, git: Session["git"]): void;
}, clock = () => Date.now()) {
  let wanted: string[] = [];
  let wantedSet = new Set<string>();
  const facts = new Map<string, Facts>();
  const project = (s: Session): Session["git"] => {
    if (!wantedSet.has(s.key) || !port.online(s.computer)) return undefined;
    const fact = facts.get(folderKey(s));
    if (!fact || fact.value.state === "not_repo") return undefined;
    const v = fact.value, number = (key: string) => typeof v[key] === "number" && Number.isSafeInteger(v[key]) && (v[key] as number) >= 0 ? v[key] as number : undefined;
    const changed = number("changed_files"), ahead = number("ahead"), behind = number("behind");
    const scope = (!s.gitRoot || v.root === s.gitRoot) && (!s.branch || v.branch === s.branch || (s.gitDetached && v.detached === true));
    const local = v.state === "ok" && changed !== undefined && scope;
    const age = typeof v.remote_age_seconds === "number" && v.remote_age_seconds >= 0 ? v.remote_age_seconds : undefined;
    const upstream = typeof v.upstream === "string" ? v.upstream : "";
    const remote = local && !!upstream.trim() && v.remote_state === "verified" && age !== undefined && age < 60;
    return { changed: changed ?? 0, ahead: ahead ?? 0, behind: behind ?? 0, conflicts: number("conflicts"),
      state: remote ? changed ? "diff" : ahead ? "push" : behind ? "pull" : "synced" : "unknown",
      streamed: true, localFresh: local, remoteVerified: remote, receivedAt: fact.at, remoteAge: age,
      changedKnown: changed !== undefined, aheadKnown: ahead !== undefined, behindKnown: behind !== undefined,
      countsKnown: changed !== undefined && ahead !== undefined && behind !== undefined,
      root: typeof v.root === "string" ? v.root : gitPath(s),
      upstream,
      detached: v.detached === true, unborn: v.unborn === true,
    };
  };
  const refresh = () => {
    const paths = new Set(wanted.map((id) => port.row(id)).filter((s): s is Session => !!s && port.online(s.computer)).map(folderKey));
    for (const key of facts.keys()) if (!paths.has(key)) facts.delete(key);
    for (const id of wanted) { const s = port.row(id); if (s) port.patch(id, project(s)); }
  };
  return {
    project,
    interests(ids: readonly string[]) {
      const next = [...new Set(ids)].filter((id) => !!port.row(id)).slice(0, GIT_ROWS);
      for (const id of wanted) if (!next.includes(id) && port.row(id)) port.patch(id, undefined);
      wanted = next; wantedSet = new Set(next);
      refresh();
      return next.map((id) => port.row(id)!).map((s) => ({ computer: s.computer, key: s.catalogKey }));
    },
    apply(updates: readonly GitOut[]) {
      const paths = new Set(wanted.map((id) => port.row(id)).filter((s): s is Session => !!s && port.online(s.computer)).map(folderKey));
      for (const update of updates.slice(0, 129)) {
        if (update.key === "" && update.kind === "unavailable" && update.payload?.code === "git_batch_overflow") {
          facts.clear(); continue;
        }
        const key = JSON.stringify([update.computer, update.key]);
        if (!paths.has(key)) continue;
        if (update.kind === "state" && update.payload?.value && typeof update.payload.value === "object")
          facts.set(key, { value: update.payload.value, at: clock() });
        else facts.delete(key);
      }
      refresh();
    },
    refresh,
    retained: () => facts.size,
  };
}
