// When a page's own save has its answer (RecoverySettings' changed()): the
// service counts answered saves, so a read's answer during the save, which
// also leaves the service idle, is never taken for it.

export interface SaveView { busy: boolean; error: string; saves: number }

/** Whether the save sent with `before` answered saves has its answer, and the line to show. */
export function saveOutcome(before: number, view: SaveView): { done: false } | { done: true; message: string; saved: boolean } {
  if (view.saves <= before || view.busy) return { done: false };
  return view.error ? { done: true, message: view.error, saved: false } : { done: true, message: "Saved.", saved: true };
}
