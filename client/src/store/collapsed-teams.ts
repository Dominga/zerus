/** Qt's workspace/collapsedTeams list after folding or unfolding one team. */
export function collapseTeams(ids: readonly string[], id: string, collapsed: boolean): string[] {
  const next = new Set(ids);
  if (collapsed) next.add(id); else next.delete(id);
  return [...next].sort();
}
