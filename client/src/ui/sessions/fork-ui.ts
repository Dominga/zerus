import type { Details } from "../../types";
import { renameParts, tagProblem, type ActionRow } from "./actions-ui";
import { trNoop, tr } from "../../i18n";

export const FORK_HINT = trNoop("Create an independent conversation with the same context and working folder");
export const FORK_CHECKING = trNoop("Checking whether this session can be forked…");
export const FORK_CHANGED = trNoop("The session changed. Open its actions again.");

/** Qt's fork_available, with the same identity gate as the selected inspection. */
export function forkAvailable(row: ActionRow, details: Details | undefined, online: boolean, busy: boolean): boolean {
  return online && !busy && !!details?.fork_supported && !!details.conversation_id
    && (!row.runId || row.runId === details.run_id) && (!row.conversationId || row.conversationId === details.conversation_id);
}

export function sameForkSource(a: ActionRow, b: ActionRow): boolean {
  return a.key === b.key && a.computer === b.computer && a.name === b.name && a.state === b.state
    && a.runId === b.runId && a.conversationId === b.conversationId && a.created === b.created && a.archiveId === b.archiveId;
}

export function forkName(row: ActionRow, tag: string): string { return renameParts(row.name).prefix + tag; }
/** Names a fork cannot take on this computer: live and saved sessions. An
 * archived session's name is free, as `hgs fork` treats it (names are labels). */
function occupied(row: ActionRow, rows: readonly ActionRow[]): Set<string> {
  return new Set(rows.filter((r) => r.computer === row.computer && r.state !== "archived").map((r) => r.name));
}
/** The tag the fork gets: `tag`, or `tag-2`, `tag-3`… while a session uses it, as `hgs fork` chooses. */
export function freeForkTag(row: ActionRow, tag: string, rows: readonly ActionRow[]): string {
  const names = occupied(row, rows);
  let value = tag;
  for (let number = 2; names.has(forkName(row, value)); number++) value = `${tag}-${number}`;
  return value;
}
/** Only an invalid tag stops the fork; a name in use gets a free suffix instead. */
export function forkProblem(_row: ActionRow, tag: string): string | undefined { return tagProblem(tag); }
export function forkTag(row: ActionRow, rows: readonly ActionRow[]): string {
  const tag = renameParts(row.name).tag;
  return freeForkTag(row, tag ? `${tag}-fork` : "fork", rows);
}
