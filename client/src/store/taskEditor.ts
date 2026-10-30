import { createSignal } from "solid-js";
import * as bridge from "../bridge";
import { notify } from "./actions";
import { exitFlush } from "./exitFlush";
import type { Fetched } from "../types";
export type TaskEditorMode = "note" | "description";
export interface TaskMessageDelivery { mode: "note" | "queue" | "send" | "reopen"; stage_at: number; dispatch_id: string }
export interface TaskRecoveryRecord { id: string; snapshot: string }
export interface TaskEditorTarget<M extends string = TaskEditorMode> { project: string; task: string; mode: M }
export interface TaskEditorView<M extends string = TaskEditorMode> extends TaskEditorTarget<M> {
  handle: number; rev: number; text: string; position: number; anchor: number;
  files: (Fetched & { reference: string })[]; error: string; recovery: boolean; recovery_record?: TaskRecoveryRecord | null; blocked: boolean; saved: boolean; unsaved?: boolean; read_only?: boolean; created?: string | null;
}
export interface TaskEditorPort<M extends string = TaskEditorMode> {
  open(target: TaskEditorTarget<M>): Promise<TaskEditorView<M>>;
  edit(input: Pick<TaskEditorView<M>, "handle" | "rev" | "text" | "position" | "anchor">): Promise<TaskEditorView<M>>;
  pick(handle: number, start: number, end: number): Promise<TaskEditorView<M>>;
  remove(handle: number, index: number): Promise<TaskEditorView<M>>;
  submit(handle: number, rev: number, recover: boolean, delivery?: TaskMessageDelivery): Promise<TaskEditorView<M>>;
  discard?(handle: number, rev: number): Promise<TaskEditorView<M>>;
  release?(handle: number, rev: number, record: TaskRecoveryRecord): Promise<TaskEditorView<M>>;
}
export function createTaskEditor<M extends string = TaskEditorMode>(port: TaskEditorPort<M>, report: (message: string) => void = () => {}) {
  const [view, setView] = createSignal<TaskEditorView<M>>(), [opening, setOpening] = createSignal(false), [busy, setBusy] = createSignal(false), [error, setError] = createSignal("");
  let generation = 0, identity = "", dirty = false, timer: ReturnType<typeof setTimeout> | undefined;
  let selecting = Promise.resolve();
  let saving: Promise<void> | undefined, operation: Promise<boolean> | undefined;
  const accept = (next: TaskEditorView<M>) => { setView(next); setError(next.error); };
  const flush = async (): Promise<void> => {
    clearTimeout(timer); timer = undefined;
    if (saving) return saving;
    saving = (async () => {
      while (dirty) {
        const current = view(); if (!current) return;
        const rev = current.rev;
        try {
          const answer = await port.edit({ handle: current.handle, rev, text: current.text, position: current.position, anchor: current.anchor });
          if (view()?.handle === current.handle && view()?.rev === rev) { accept(answer); dirty = false; }
        } catch (e) { setError(String(e)); throw e; }
      }
    })();
    try { await saving; } finally { saving = undefined; }
  };
  const follow = (target?: TaskEditorTarget<M>) => {
    const key = target ? JSON.stringify([target.project, target.task, target.mode]) : "";
    if (key === identity) return;
    identity = key; const mine = ++generation; setOpening(!!target); setError("");
    selecting = selecting.catch(() => {}).then(async () => {
      if (mine !== generation) return;
      try {
        if (operation) await operation;
        await flush(); if (mine !== generation) return;
        setView(undefined); dirty = false;
        if (!target) return;
        const answer = await port.open(target);
        if (mine === generation) accept(answer);
      } catch (e) { if (mine === generation) setError(String(e)); }
      finally { if (mine === generation) setOpening(false); }
    });
  };
  const locked = () => opening() || busy() || !view() || !!view()?.blocked;
  const edit = (text: string, position: number, anchor: number) => {
    if (locked()) return;
    setView((v) => v ? { ...v, text, position, anchor, rev: v.rev + 1, saved: false } : v); dirty = true;
    clearTimeout(timer); timer = setTimeout(() => { void flush().catch(() => {}); }, 250);
  };
  const run = (call: (v: TaskEditorView<M>) => Promise<TaskEditorView<M>>): Promise<boolean> => {
    if (locked()) return Promise.resolve(false);
    const mine = generation; setBusy(true);
    operation = (async () => {
      try {
        await flush(); const current = view(); if (!current || mine !== generation) return false;
        const answer = await call(current);
        if (mine !== generation) { if (answer.error) report(answer.error); return false; }
        accept(answer); dirty = !!answer.unsaved; return answer.saved;
      } catch (e) { if (mine === generation) setError(String(e)); else report(String(e)); return false; }
      finally { setBusy(false); }
    })();
    return operation.finally(() => { operation = undefined; });
  };
  return { view, opening, busy, error, locked, follow, edit,
    flushForExit: async () => { if (operation) await operation; await flush(); },
    flush, pick: () => run((v) => port.pick(v.handle, Math.min(v.position, v.anchor), Math.max(v.position, v.anchor))),
    remove: (index: number) => run((v) => port.remove(v.handle, index)),
    discard: () => run((v) => port.discard ? port.discard(v.handle, v.rev) : Promise.reject(new Error("This draft cannot be discarded here."))),
    release: (handle: number, record: TaskRecoveryRecord) => run((v) => v.handle === handle && port.release ? port.release(v.handle, v.rev, record) : Promise.reject(new Error("The pending message changed; refresh this task."))),
    submit: (recover = false, delivery?: TaskMessageDelivery) => run((v) => delivery && !recover ? port.submit(v.handle, v.rev, false, delivery) : port.submit(v.handle, v.rev, recover)),
  };
}
export const taskEditor = createTaskEditor({
  open: (target) => bridge.taskEditorOpen(target), edit: (input) => bridge.taskEditorEdit(input),
  pick: (handle, start, end) => bridge.taskEditorPick(handle, start, end), remove: (handle, index) => bridge.taskEditorRemove(handle, index),
  submit: (handle, rev, recover, delivery) => bridge.taskEditorSubmit(handle, rev, recover, delivery),
  release: (handle, rev, record) => bridge.taskEditorRelease(handle, rev, record),
}, (message) => notify("error", message));
exitFlush.register(taskEditor.flushForExit);
