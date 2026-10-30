import { createSignal } from "solid-js";
import * as bridge from "../bridge";
import { notify } from "./actions";
import { exitFlush } from "./exitFlush";
import type { Fetched } from "../types";
export type TaskEditorMode = "note" | "description";
export interface TaskEditorTarget { project: string; task: string; mode: TaskEditorMode }
export interface TaskEditorView extends TaskEditorTarget {
  handle: number; rev: number; text: string; position: number; anchor: number;
  files: (Fetched & { reference: string })[]; error: string; recovery: boolean; blocked: boolean; saved: boolean; unsaved?: boolean;
}
export interface TaskEditorPort {
  open(target: TaskEditorTarget): Promise<TaskEditorView>;
  edit(input: Pick<TaskEditorView, "handle" | "rev" | "text" | "position" | "anchor">): Promise<TaskEditorView>;
  pick(handle: number, start: number, end: number): Promise<TaskEditorView>;
  remove(handle: number, index: number): Promise<TaskEditorView>;
  submit(handle: number, rev: number, recover: boolean): Promise<TaskEditorView>;
}
export function createTaskEditor(port: TaskEditorPort, report: (message: string) => void = () => {}) {
  const [view, setView] = createSignal<TaskEditorView>(), [opening, setOpening] = createSignal(false), [busy, setBusy] = createSignal(false), [error, setError] = createSignal("");
  let generation = 0, identity = "", dirty = false, timer: ReturnType<typeof setTimeout> | undefined;
  let selecting = Promise.resolve();
  let saving: Promise<void> | undefined, operation: Promise<boolean> | undefined;
  const accept = (next: TaskEditorView) => { setView(next); setError(next.error); };
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
  const follow = (target?: TaskEditorTarget) => {
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
  const run = (call: (v: TaskEditorView) => Promise<TaskEditorView>): Promise<boolean> => {
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
    submit: (recover = false) => run((v) => port.submit(v.handle, v.rev, recover)),
  };
}
export const taskEditor = createTaskEditor({
  open: (target) => bridge.taskEditorOpen(target), edit: (input) => bridge.taskEditorEdit(input),
  pick: (handle, start, end) => bridge.taskEditorPick(handle, start, end), remove: (handle, index) => bridge.taskEditorRemove(handle, index),
  submit: (handle, rev, recover) => bridge.taskEditorSubmit(handle, rev, recover),
}, (message) => notify("error", message));
exitFlush.register(taskEditor.flushForExit);
