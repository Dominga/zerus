import { createSignal } from "solid-js";
import { tr } from "../i18n";
import type { MemoryCall, MemoryHit, MemoryMoved, MemoryOp, MemoryOutcome, MemoryView } from "../bridge";
import type { Text } from "../text";

// Task 126 (owner, 2026-11-07): project memory, Qt's ProjectMemoryPanel.
// The owner turns RustCodeLore on for a whole project and chooses where its
// store lives: under Zerus's state folder by default, or a folder picked with
// Browse. A store that already holds records is moved, never repointed: the
// move copies it, verifies every file and only then switches, after the
// owner's confirmation; the old store stays where it was. Turning memory off
// keeps the store. hgs owns the settings (client/core/src/project_memory.rs
// builds each guarded call); a write that may have run reads them again.

interface Port {
  call(call: MemoryCall): Promise<MemoryOutcome>;
  /** The folder dialog; null when cancelled. */
  pickFolder(start: string): Promise<string | null>;
  /** Asks before a move; Cancel is the default. */
  confirmMove(from: string, to: string, records: number): Promise<boolean>;
}

const message = (error: unknown): Text => ({ source: "%1", args: [{ kind: "verbatim", value: String(error) }] });

/** A size in bytes, as the status shows it. */
export function sizeText(bytes: number): string {
  if (bytes < 1024) return tr("%1 B", bytes);
  if (bytes < 1024 * 1024) return tr("%1 KiB", (bytes / 1024).toFixed(1));
  if (bytes < 1024 * 1024 * 1024) return tr("%1 MiB", (bytes / (1024 * 1024)).toFixed(1));
  return tr("%1 GiB", (bytes / (1024 * 1024 * 1024)).toFixed(1));
}

/** The status line: records, size and the index state. */
export function statusText(view: MemoryView): string {
  if (!view.exists) return tr("No store yet: it is made when memory is turned on.");
  const index = view.index === "built" ? tr("index up to date") : view.index === "stale" ? tr("index out of date") : tr("no index yet");
  return tr("%1 records, %2, %3", view.records, sizeText(view.bytes), index);
}

/** What a finished move says. */
export function movedText(moved: MemoryMoved): string {
  return moved.old_kept
    ? tr("Moved %1 files (%2) and checked them. The old store is kept at %3.", moved.files, sizeText(moved.bytes), moved.from)
    : tr("The store is now at %1.", moved.to);
}

export function createProjectMemory(port: Port) {
  const [project, setProjectSignal] = createSignal("");
  const [view, setView] = createSignal<MemoryView>();
  const [loading, setLoading] = createSignal(false);
  const [failed, setFailed] = createSignal(false);
  const [error, setError] = createSignal<Text>();
  const [notice, setNotice] = createSignal("");
  // Task #397: the project's search and the notes lore reports with it (the
  // model download says there what it is doing); the notice can be dismissed.
  const [found, setFound] = createSignal<{ hits: MemoryHit[]; store: string; query: string }>();
  const [searching, setSearching] = createSignal(false);
  const [searchError, setSearchError] = createSignal<Text>();
  const [notes, setNotes] = createSignal<string[]>([]);
  const [notesDismissed, setNotesDismissed] = createSignal(false);
  let generation = 0;

  async function reload(keep?: Text) {
    const current = ++generation;
    setView(undefined); setError(undefined); setFailed(false);
    if (!project()) { setLoading(false); return; }
    setLoading(true);
    let outcome: MemoryOutcome;
    try { outcome = await port.call({ project: project(), op: { op: "read" } }); }
    catch (thrown) { outcome = { outcome: "failed", detail: message(thrown) }; }
    if (current !== generation) return;
    setLoading(false);
    if (outcome.outcome === "done") { setView(outcome.view); setError(keep); return; }
    if (outcome.outcome === "failed" || outcome.outcome === "unsupported") { setFailed(true); setError(outcome.detail); }
  }

  /** Searches the project's store and its team sub-stores; each hit names its store. */
  async function search(query: string): Promise<void> {
    const text = query.trim();
    if (!project() || !text) return;
    const current = generation;
    setSearching(true); setSearchError(undefined);
    let outcome: MemoryOutcome;
    try { outcome = await port.call({ project: project(), op: { op: "find", query: text, limit: 10 } }); }
    catch (thrown) { outcome = { outcome: "failed", detail: message(thrown) }; }
    if (current !== generation) return;
    setSearching(false);
    if (outcome.outcome === "found") {
      setFound({ hits: outcome.hits, store: outcome.store, query: outcome.query });
      setNotes(outcome.notes); setNotesDismissed(false);
    } else if (outcome.outcome === "failed" || outcome.outcome === "unsupported") {
      setFound(undefined); setSearchError(outcome.detail);
    }
  }
  /** The notes still worth showing: the owner dismissed the last ones. */
  const openNotes = () => (notesDismissed() ? [] : notes());
  const dismissNotes = () => setNotesDismissed(true);
  const clearSearch = () => { setFound(undefined); setSearchError(undefined); };

  async function write(op: MemoryOp): Promise<boolean> {
    if (!project() || loading() || failed() || !view()) return false;
    const current = generation;
    setLoading(true); setError(undefined); setNotice("");
    let outcome: MemoryOutcome;
    try { outcome = await port.call({ project: project(), op }); }
    catch (thrown) { outcome = { outcome: "failed", detail: message(thrown) }; }
    if (current !== generation) return false;
    setLoading(false);
    switch (outcome.outcome) {
      case "done":
        setView(outcome.view);
        if (outcome.moved) setNotice(movedText(outcome.moved));
        return true;
      case "uncertain": void reload(outcome.detail); return false;
      case "unsupported": setFailed(true); setError(outcome.detail); return false;
      case "failed": setError(outcome.detail); return false;
      // A search is not a write; it never reaches here.
      default: return false;
    }
  }

  const editable = () => !loading() && !failed() && !!view();
  /** Moves the store to `to` after the owner confirms; the old one stays. */
  async function moveTo(to: string): Promise<boolean> {
    const now = view();
    if (!now || !editable() || !to.trim() || to.trim() === now.store) return false;
    if (!(await port.confirmMove(now.store, to.trim(), now.records))) return false;
    if (view() !== now) return false;
    return write({ op: "move", to: to.trim(), expected_revision: now.revision });
  }
  return {
    project, view, loading, failed, error, notice, editable,
    setProject(next: string) {
      if (next === project()) return;
      setProjectSignal(next);
      setNotice("");
      setFound(undefined); setSearchError(undefined); setNotes([]);
      void reload();
    },
    reload: () => reload(),
    // Task #397: the search and the notes it brings; the owner dismisses them.
    found, searching, searchError, openNotes, dismissNotes, clearSearch, search,
    setEnabled(enabled: boolean) {
      const now = view();
      return now && now.enabled !== enabled ? write({ op: "enable", enabled, expected_revision: now.revision }) : Promise.resolve(false);
    },
    /** Browse: an empty store just takes the new folder; one with records moves. */
    async browse(): Promise<boolean> {
      const now = view();
      if (!now || !editable()) return false;
      const picked = await port.pickFolder(now.store);
      if (!picked || picked === now.store || view() !== now) return false;
      if (now.records > 0) return moveTo(picked);
      return write({ op: "store", store: picked, expected_revision: now.revision });
    },
    moveTo,
    /** Back to the default folder: only an empty store is repointed. */
    useDefault() {
      const now = view();
      if (!now || now.default || !editable()) return Promise.resolve(false);
      return now.records > 0 ? moveTo(now.default_store) : write({ op: "store", store: "", expected_revision: now.revision });
    },
  };
}
export type ProjectMemory = ReturnType<typeof createProjectMemory>;
