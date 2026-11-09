import type { Session } from "./types";
import { type MenuEntry, hasCompactRows, expandCompactRows, setSelectedKey, setSubagentOpen, setDetailTab } from "../../store/ui";
import { setParentDialog } from "../../store/parentLinks";
import { runningCommandsEntry } from "../../store/processes";
import { computerFresh } from "../../store/catalog";
import { flag } from "../../store/settings";
import { tr } from "../../i18n";

/** The original confirmed parent-link actions, now in Session actions. */
export function parentEntries(s: Session): MenuEntry[] {
  if (s.state === "archived") return [];
  const linked = ["claude", "codex", "kimi"].includes(s.cmd) && !!s.sessionId;
  const why = linked ? undefined : tr("Parent sessions are recorded for Claude, Codex and Kimi sessions that have started.");
  const entries: MenuEntry[] = [{ label: tr("Set parent…"), icon: "git-branch", disabled: !linked, hint: why, action: () => setParentDialog({ kind: "set", child: s }) }];
  if (s.parent) entries.push({ label: tr("Detach from %1…", s.parent.name.split("/").pop() || s.parent.name), icon: "git-branch", disabled: !linked, hint: why,
    action: () => setParentDialog({ kind: "detach", child: s }) });
  return entries;
}

export function expandedEntries(s: Session): MenuEntry[] {
  const commands = runningCommandsEntry(s, () => computerFresh(s.computer), () => flag("processes/enabled"), () => { setSelectedKey(s.key); setSubagentOpen(undefined); setDetailTab("processes"); });
  return [
    { label: tr("Expand all collapsed"), icon: "expand-row", disabled: !hasCompactRows(), action: expandCompactRows },
    ...(commands ? [commands] : []),
    ...parentEntries(s),
  ];
}
