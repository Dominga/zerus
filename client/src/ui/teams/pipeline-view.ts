import type { MemberView, StageKind, StageView, TaskView, TeamView } from "./types";

// Presentation of the bounded page the core sent. These functions never read
// another page or turn a role name, account or idle session into pipeline facts.
export const closedStage = (stage: Pick<StageView, "state">) => ["accepted", "cancelled"].includes(stage.state);
export const orderedStages = (card: TaskView) => [...card.stages].sort((a, b) => a.index - b.index || a.node - b.node);
/** A nested stage whose own target names another node contradicts the card it
 * is shown under: it never places the card or makes an assignment. */
export const targetDisagrees = (card: Pick<TaskView, "node" | "kind">, stage: Pick<StageView, "node" | "target">) =>
  card.kind !== "stage" && stage.target !== undefined && stage.target !== null && stage.target !== card.node;
/** A round above the pinned pipeline's rounds is not a round the chip can name. */
export const roundDisagrees = (stage: Pick<StageView, "round">, rounds?: number | null) =>
  !!rounds && rounds > 0 && !!stage.round && stage.round > rounds;
/** The stage facts of one card agree with each other and with the card. */
export const stageAgrees = (card: TaskView, stage: StageView) => !targetDisagrees(card, stage) && !roundDisagrees(stage, card.rounds);
export function currentStage(card: TaskView): StageView | undefined {
  if (card.kind === "stage") return orphanStage(card);
  const stages = orderedStages(card).filter((stage) => !targetDisagrees(card, stage));
  return stages.find((stage) => stage.state === "in_progress") ?? stages.find((stage) => !closedStage(stage));
}
/** A stage row whose target is not on this page: its own stage facts as the
 * core kept them, or the row's own fields from an older core. */
export const orphanStage = (card: TaskView): StageView => card.stages.find((stage) => stage.node === card.node)
  ?? { node: card.node, index: 0, role: card.role, state: card.state, assignee: card.assignee, implementer: card.implementer };
export const stageKey = (stage: Pick<StageView, "kind" | "role">) => stage.kind || `role:${stage.role}`;
export interface BoardColumn { key: string; kind?: StageKind; role?: string; cards: TaskView[] }
export function boardColumns(view: TeamView): BoardColumn[] {
  const cards = view.groups.flatMap((group) => group.cards);
  const stageColumns = new Map<string, BoardColumn>();
  const ensure = (stage: StageView) => {
    const key = stageKey(stage);
    if (!stageColumns.has(key)) stageColumns.set(key, { key, kind: stage.kind ?? undefined, role: stage.kind ? undefined : stage.role, cards: [] });
  };
  for (const card of cards) {
    if (card.kind === "stage") ensure(orphanStage(card));
    for (const stage of card.stages) ensure(stage);
  }
  const columns: BoardColumn[] = ["queue", "work", "submitted"].map((key) => ({ key, cards: [] }));
  const kinds: string[] = ["review", "test", "land"];
  columns.push(...[...stageColumns.values()].sort((a, b) => {
    const rank = (key: string) => kinds.includes(key) ? kinds.indexOf(key) : kinds.length;
    return rank(a.key) - rank(b.key);
  }), { key: "accepted", cards: [] });
  for (const card of cards) {
    const stage = currentStage(card);
    const key = card.state === "accepted" ? "accepted" : card.state === "cancelled" ? "cancelled"
      : card.kind === "stage" && stage ? stageKey(stage)
      : ["waiting", "pending", "draft"].includes(card.state) ? "queue"
      : stage && ["in_progress", "reported"].includes(stage.state) ? stageKey(stage) : card.state === "in_progress" ? "work" : card.state === "reported" ? "submitted" : card.state;
    let column = columns.find((column) => column.key === key);
    if (!column) { column = { key, cards: [] }; columns.push(column); }
    column.cards.push(card);
  }
  return columns;
}
/** Who does a card's work: a stage's own assignee, never the implementer of
 * the task it checks; a task's implementer once handed out, else its assignee. */
export const workerOf = (card: TaskView) => card.kind === "stage" ? card.assignee : card.implementer || card.assignee;
export const cardWorker = (card: TaskView, members: readonly MemberView[]) => {
  const id = workerOf(card);
  return id ? members.find((member) => member.id === id) : undefined;
};
export interface MemberWork { card: TaskView; stage?: StageView }
export function memberWork(member: MemberView, cards: readonly TaskView[]): MemberWork[] {
  const work: MemberWork[] = [];
  for (const card of cards) {
    if (["accepted", "cancelled"].includes(card.state)) continue;
    if (card.kind === "stage") {
      const stage = orphanStage(card);
      if ((stage.holder || card.holder || stage.assignee) === member.id && !roundDisagrees(stage, card.rounds)) work.push({ card, stage });
      continue;
    }
    if ((card.holder || card.assignee) === member.id) work.push({ card });
    // Contradictory stage facts are marked on the card and assert no assignment.
    for (const stage of card.stages) if (!closedStage(stage) && (stage.holder || stage.assignee) === member.id && stageAgrees(card, stage)) work.push({ card, stage });
  }
  return work;
}
