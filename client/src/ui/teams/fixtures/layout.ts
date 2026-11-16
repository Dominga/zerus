import fixture from "./lanes-view.json";
import type { TeamView } from "../../../store/teams";

// Existing Rust operation/view-writer output is the seed. Extra cards/stages
// below are explicitly synthetic, bounded UI stress rows, never task writes.
export function workspaceLayoutView(kind: "short" | "many" | "long-detail" | "empty" = "short"): TeamView {
  const view = structuredClone(fixture.before) as unknown as TeamView;
  // A sparse product-derived page must leave blank space below the cards.
  if (kind === "short" || kind === "long-detail") {
    view.groups = view.groups.map((group) => ({ ...group, cards: group.cards.filter((card) => [1, 5, 8].includes(card.node)) }));
  }
  if (kind === "empty") view.groups = view.groups.map((group) => ({ ...group, cards: [] }));
  if (kind === "many") {
    const pending = view.groups.find((group) => group.state === "pending")!;
    const seed = pending.cards[0];
    pending.cards = [...pending.cards, ...Array.from({ length: 60 }, (_, i) => ({
      ...structuredClone(seed), node: 1000 + i, title: `Synthetic bounded task ${i + 1}`,
      task: { project: "example-project", uuid: `00000000-0000-4000-8000-${(1000 + i).toString(16).padStart(12, "0")}` },
      rank: `z${String(i).padStart(3, "0")}`, stages: [],
    }))];
  }
  if (kind === "long-detail") {
    const task = view.groups.flatMap((group) => group.cards).find((card) => card.node === 1)!;
    const stage = task.stages[0];
    task.title = "Synthetic long task detail with multiple review stages and retained context";
    task.stages = Array.from({ length: 16 }, (_, i) => ({ ...structuredClone(stage), node: 6000 + i, index: i }));
  }
  const cards = view.groups.flatMap((group) => group.cards);
  for (const state of ["waiting", "pending", "in_progress", "reported", "accepted", "cancelled"] as const) {
    view.bar.counts.nodes[state] = cards.filter((card) => card.state === state).length;
  }
  return view;
}
