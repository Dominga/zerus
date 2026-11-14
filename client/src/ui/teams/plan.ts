// Adapted from WhiteWh/zerus plan.ts: dependency chains and per-phase plans.
// Layers are supplied by client/core's team-plan projection, never inferred here.
import type { TeamCard } from "../../store/teams";
import { placePhase, routeEdges } from "./planLayout";

export function ancestors(tasks: readonly TeamCard[], id: number): Set<number> {
  const found = new Set<number>([id]);
  const pending = [id];
  const byId = new Map(tasks.map((task) => [task.node, task]));
  while (pending.length) {
    const task = byId.get(pending.pop()!);
    for (const dep of task?.deps ?? []) {
      if (!found.has(dep.node)) { found.add(dep.node); pending.push(dep.node); }
    }
  }
  return found;
}

export function descendants(tasks: readonly TeamCard[], id: number): Set<number> {
  const found = new Set<number>([id]);
  const pending = [id];
  while (pending.length) {
    const current = pending.pop()!;
    for (const task of tasks) {
      if (!found.has(task.node) && task.deps?.some((dep) => dep.node === current)) {
        found.add(task.node); pending.push(task.node);
      }
    }
  }
  return found;
}

export function chain(tasks: readonly TeamCard[], id: number): Set<number> {
  return new Set([...ancestors(tasks, id), ...descendants(tasks, id)]);
}

export function phasePlans(tasks: readonly TeamCard[]) {
  const phases = [...new Set(tasks.map((task) => task.phase ?? 0))].sort((a, b) => a - b);
  return phases.map((phase) => {
    const cards = tasks.filter((task) => (task.phase ?? 0) === phase);
    const ids = new Set(cards.map((task) => task.node));
    const edges = cards.flatMap((task) => (task.deps ?? []).filter((dep) => ids.has(dep.node)).map((dep) => ({ from: dep.node, to: task.node, kind: dep.kind ?? "hard" })));
    const layout = placePhase(cards.map((task) => ({ id: task.node, layer: task.layer ?? 0 })), edges);
    return { phase, cards, edges, layout, routes: routeEdges(layout, edges) };
  });
}
