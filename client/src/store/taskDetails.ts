import { createSignal } from "solid-js";
import { readTaskDetails } from "../bridge";
import type { Fetched } from "../types";
import { object, taskItem, text, type Task } from "../ui/screens/tasks-ui";

export interface TaskEntry {
  kind: string; text: string; at: number; actor: string; session: string; requestId: string; files: Fetched[];
  delivery: { mode: string; state: string; error: string };
}
export interface TaskDetail extends Task { files: Fetched[]; log: TaskEntry[]; total: number; next?: string; started: boolean; dispatch: string }
const actor = (v: unknown) => { const a = object(v); return a.kind === "agent" ? text(a.session) + (a.host ? ` @${text(a.host)}` : "") : "You"; };
const files = (v: unknown): Fetched[] => Array.isArray(v) ? v.slice(0, 8) as Fetched[] : [];
export function taskDetail(raw: unknown, project: string, uuid: string): TaskDetail {
  const d = object(raw);
  if (d.uuid !== uuid || d.project !== project) throw new Error("The task reply did not match the selected task.");
  if (d.history_schema !== 1 || !Array.isArray(d.log) || d.log.length > 80) throw new Error("Update hgs to read bounded task history.");
  return { ...taskItem(d), files: files(d.attachments), total: typeof d.log_total === "number" ? d.log_total : d.log.length,
    next: text(d.history_next) || undefined, started: d.start_confirmed === true, dispatch: text(d.dispatch_id),
    log: d.log.map((value) => {
      const e = object(value), delivery = object(e.delivery);
      return { kind: text(e.kind), text: text(e.text), at: typeof e.at === "number" ? e.at : 0,
        actor: actor(e.actor), session: text(object(e.session).session), requestId: text(e.request_id), files: files(e.attachments),
        delivery: { mode: text(delivery.mode), state: text(delivery.state), error: text(delivery.error) } };
    }) };
}

/** One page and one running read. Board updates coalesce into one follow-up;
 * a same-selection reply is published even while another refresh is pending. */
export function createTaskDetails(read: (project: string, uuid: string, before: string | null) => Promise<unknown>) {
  const [data, setData] = createSignal<TaskDetail>();
  const [loading, setLoading] = createSignal(false), [error, setError] = createSignal("");
  const [before, setBefore] = createSignal<string>();
  let selection: { project: string; uuid: string } | undefined, generation = 0, running = false, pending = false;
  const drain = async () => {
    if (running || !pending || !selection) return;
    pending = false; running = true; setLoading(true);
    const target = selection, mine = generation, cursor = before();
    try {
      const reply = await read(target.project, target.uuid, cursor ?? null);
      if (mine === generation) { setData(taskDetail(reply, target.project, target.uuid)); setError(""); }
    } catch (e) { if (mine === generation) setError(String(e)); }
    finally { running = false; if (mine === generation) setLoading(false); void drain(); }
  };
  const refresh = () => { if (selection) { pending = true; void drain(); } };
  return {
    data, loading, error, before, refresh,
    select(project?: string, uuid?: string) {
      if (selection?.project === project && selection?.uuid === uuid) return;
      generation++; selection = project && uuid ? { project, uuid } : undefined; pending = false;
      setData(undefined); setError(""); setBefore(undefined); setLoading(!!selection); refresh();
    },
    older() { if (!data()?.next || loading()) return; generation++; setBefore(data()!.next); refresh(); },
    latest() { generation++; setBefore(undefined); refresh(); },
  };
}
export const taskDetails = createTaskDetails(readTaskDetails);
