import { gitRows } from "../bridge";
import { gitInterests } from "./catalog";

/** At most one IPC plus its latest replacement; never replay a refused route. */
export function createGitInterestSender(send: typeof gitRows) {
  let pending: Parameters<typeof gitRows>[0] | undefined;
  let running = false;
  const drain = async () => {
    running = true;
    while (pending) {
      const rows = pending; pending = undefined;
      try { await send(rows); }
      catch { console.warn("Zerus: visible Git subscriptions are unavailable."); }
    }
    running = false;
  };
  return (rows: Parameters<typeof gitRows>[0]) => {
    pending = rows;
    if (!running) void drain();
  };
}
const send = createGitInterestSender((rows) => gitRows(rows));
export const followGitRows = (ids: readonly string[]) => send(gitInterests(ids));
