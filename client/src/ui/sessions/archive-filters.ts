import type { ArchivedSession, PagingComputer } from "../../store/archive";
import type { GroupingOut } from "../../types";

/** Qt's visible archive loop counts before each filter applies (SessionsWindowRows.cpp:266-284). */
export function archiveFilterCounts(rows: readonly Pick<ArchivedSession, "computer" | "machine" | "group" | "archivedAt">[], includes: (at: number) => boolean,
                                    machines: readonly string[], projects: readonly string[]) {
  const machine = new Map<string, number>(), project = new Map<string, number>();
  for (const s of rows) {
    if (!includes(s.archivedAt)) continue;
    if (!machines.length || machines.includes(s.machine)) project.set(s.group, (project.get(s.group) ?? 0) + 1);
    if (!projects.length || projects.includes(s.group)) machine.set(s.computer, (machine.get(s.computer) ?? 0) + 1);
  }
  return { machine, project };
}

/** Machines come from the fleet, including those with no loaded rows (MachineFilter.cpp:90). */
export const archiveMachineChoices = (fleet: readonly PagingComputer[], counts: ReadonlyMap<string, number>) =>
  fleet.map((h) => ({ id: h.machine, label: h.computer === "" ? `${h.machine} (this machine)` : h.machine,
                     count: counts.get(h.computer) ?? 0, computer: h.computer }));

/** Accessible, counted or selected projects, in organization order (ProjectFilter.cpp:80-84). */
export function archiveProjectChoices(g: Pick<GroupingOut, "loaded" | "headers">, counts: ReadonlyMap<string, number>, selected: readonly string[]) {
  if (!g.loaded) return [];
  const choices = g.headers.filter((h) => h.accessible || counts.has(h.id) || selected.includes(h.id))
    .map((h) => ({ id: h.id, label: h.name, count: counts.get(h.id) ?? 0 }));
  for (const id of [...selected].sort()) if (!choices.some((c) => c.id === id)) choices.push({ id, label: id, count: counts.get(id) ?? 0 });
  return choices;
}
