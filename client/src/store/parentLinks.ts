import { createSignal } from "solid-js";
import { setParent, type ParentChange } from "../bridge";
import { orderedRows } from "./catalog";
import { machineCards } from "./machines";
import { toast } from "./notices";
import { parentIndex } from "../ui/sessions/model";
import type { Session } from "../ui/sessions/types";
import { tr } from "../i18n";

// Desktop changes of parent links (zerus-sa9qn.9): Detach, Set parent… and
// drag to nest. Every change is confirmed in a dialog first; the session's
// computer checks the exact session, the chosen parent and cycles.

export type ParentDialog =
  | { kind: "detach"; child: Session }
  | { kind: "set"; child: Session; parent?: Session };
export const [parentDialog, setParentDialog] = createSignal<ParentDialog>();
const [busy, setBusy] = createSignal(false);
export const parentChangeBusy = busy;

/** Agents whose sessions carry parent links (the host refuses others). */
const LINKED = new Set(["claude", "codex", "kimi"]);
const identified = (s: Session) => !!s.sessionId && !!s.runId && !!s.conversationId && s.state !== "archived" && LINKED.has(s.cmd);

/** Whether `child` may be made a child of `parent`: both identified, on one computer, not the same session. */
export const nestable = (child: Session, parent: Session) =>
  child.key !== parent.key && child.computer === parent.computer && identified(child) && identified(parent);

/**
 * The sessions Set parent… offers for `child` among `rows`: same computer,
 * identified, never the child's own descendants (a bounded walk up each
 * row's parents; the host refuses any cycle it still finds).
 */
export function candidatesAmong(child: Session, rows: readonly Session[], machineOf: (computer: string) => string | undefined): Session[] {
  const keyOf = parentIndex(rows, machineOf);
  const byKey = new Map(rows.map((row) => [row.key, row] as const));
  const below = (row: Session) => {
    let at: Session | undefined = row;
    for (let depth = 0; at && depth < 8; depth++) {
      const up = keyOf(at);
      if (!up) return false;
      if (up === child.key) return true;
      at = byKey.get(up);
    }
    return false;
  };
  return rows.filter((s) => nestable(child, s) && !below(s));
}

export const parentCandidates = (child: Session) => candidatesAmong(child, orderedRows(), (computer) => machineCards[computer]?.machine_id);

const target = (s: Session) => ({ computer: s.computer, name: s.name, run_id: s.runId ?? "", conversation_id: s.conversationId ?? "" });
const short = (name: string) => name.split("/").pop() || name;

async function change(child: Session, request: ParentChange, done: string) {
  if (busy()) return false;
  setBusy(true);
  try {
    await setParent(target(child), request);
    toast(tr("Parent session"), done);
    return true;
  } catch (error) {
    toast(tr("Parent session"), String(error instanceof Error ? error.message : error), "error", true);
    return false;
  } finally {
    setBusy(false);
  }
}

/** Removes the child's link after its Detach confirmation. */
export const detachParent = (child: Session) =>
  change(child, { kind: "clear" }, tr("%1 is no longer a child session.", short(child.title)));

/** Makes `parent` the child's parent after its confirmation; replacing another parent was part of that confirmation. */
export const chooseParent = (child: Session, parent: Session) =>
  change(child, {
    kind: "set", session_id: parent.sessionId ?? "", name: parent.name, conversation_id: parent.conversationId ?? "",
    run_id: parent.runId ?? "", replace: !!child.parent,
  }, tr("%1 is now a child of %2.", short(child.title), short(parent.title)));
