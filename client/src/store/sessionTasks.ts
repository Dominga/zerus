import { batch, createSignal } from "solid-js";
import type { TaskScope } from "../bridge";
import { STAGES, object, taskItem, text, type Stage, type Task } from "../ui/screens/tasks-ui";

/** `hgs task list --session` of an older hgs (client/core `UPDATE_FOR_SESSION_PAGES`). */
export const UPDATE_FOR_SESSION_PAGES = "Update hgs to read one session's tasks.";

export type ScopedRead = (project: string, stage: Stage, limit: number, after: string | null, scope: TaskScope) => Promise<unknown>;
type Page = { tasks?: unknown; page?: { next?: unknown }; counts?: unknown };

const none = <T,>(value: T) => Object.fromEntries(STAGES.map((s) => [s, value])) as Record<Stage, T>;

/**
 * One session's view of the followed project (Only this session): each
 * stage's rows as hgs pages them for the session's identity (`task list
 * --session`, zerus-cg5h.26), never the loaded window filtered here. A stage
 * the board shows is read from its start when first shown, and again to the
 * extent the user loaded whenever the project's list changes. One read per
 * stage at a time: a change during a read reads that stage once more after
 * it. Totals are the view's own counts from the latest reply.
 */
export function createSessionView(read: ScopedRead, window: number, pageLimit: number, maxPages: number) {
  /** `project \n host \n session ID` of the view shown, "" when none. */
  const [key, setKey] = createSignal("");
  const [view, setView] = createSignal({ rows: none<Task[]>([]), next: none<string | null>(null), read: none(false) });
  const [counts, setCounts] = createSignal<Record<Stage, number> | null>(null);
  const [loading, setLoading] = createSignal(none(false));
  const [error, setError] = createSignal("");
  const [unsupported, setUnsupported] = createSignal(false);
  let project = "", scope: TaskScope | undefined, watched: readonly Stage[] = [], generation = 0;
  /** Changes of the list so far, and the change the shown totals were read at (zerus-0h8lg). */
  let change = 0, countsAt = 0;
  let extent = none(0), fresh = none(false), reads = none(0), again = none(false);

  const busy = (stage: Stage, on: boolean) => setLoading({ ...loading(), [stage]: on });
  const unique = (values: unknown, stage: Stage) => {
    const seen = new Set<string>();
    return (Array.isArray(values) ? values : []).flatMap((value) => {
      const t = taskItem(value); if (!t.uuid || seen.has(t.uuid) || t.stage !== stage) return [];
      seen.add(t.uuid); return [t];
    });
  };
  /** Totals of every stage from a reply read at change `seen`; never older than those shown. */
  const counted = (page: Page, seen: number) => {
    const c = object(page.counts);
    if (seen < countsAt || !STAGES.every((s) => typeof c[s] === "number")) return;
    countsAt = seen;
    setCounts(Object.fromEntries(STAGES.map((s) => [s, c[s] as number])) as Record<Stage, number>);
  };
  const failed = (e: unknown) => { if (String(e).includes(UPDATE_FOR_SESSION_PAGES)) setUnsupported(true); else setError(String(e)); };
  const placed = (stage: Stage, rows: Task[], next: string | null) =>
    setView({ rows: { ...view().rows, [stage]: rows }, next: { ...view().next, [stage]: next }, read: { ...view().read, [stage]: true } });
  /** After a read of `stage`: once more when the list changed meanwhile. */
  const settle = (stage: Stage) => {
    busy(stage, false);
    if (again[stage]) { again[stage] = false; refresh(stage); }
  };

  /** Reads `stage` from its start up to the rows the user loaded, replacing them when still current. */
  async function reread(stage: Stage) {
    const at = generation, mine = ++reads[stage], want = Math.max(window, extent[stage]), owner = project, as = scope!, seen = change;
    fresh[stage] = true; busy(stage, true);
    try {
      let found: Task[] = [], after: string | null = null, pages = 0;
      do {
        const page = await read(owner, stage, Math.min(pageLimit, want - found.length), after, as) as Page;
        if (at !== generation || mine !== reads[stage]) return;
        const cursor = text(object(page.page).next) || null;
        found = unique([...found, ...(Array.isArray(page.tasks) ? page.tasks : [])], stage);
        counted(page, seen);
        // A cursor that does not advance ends the read rather than looping on it.
        after = cursor && cursor !== after ? cursor : null;
      } while (after && found.length < want && ++pages < maxPages);
      batch(() => { placed(stage, found, after); setError(""); });
    } catch (e) {
      if (at === generation && mine === reads[stage]) failed(e);
    } finally {
      if (at === generation && mine === reads[stage]) settle(stage);
    }
  }
  function refresh(stage: Stage) {
    if (!key() || unsupported()) return;
    if (!watched.includes(stage)) { fresh[stage] = false; return; }
    if (loading()[stage]) { again[stage] = true; return; }
    void reread(stage);
  }

  return {
    /** Whether the board shows hgs's own view of a session. */
    active: () => !!key() && !unsupported(),
    /** An hgs without `--session`: the board filters its loaded rows instead. */
    unsupported, error, loading,
    rows: (stage: Stage) => view().rows[stage],
    /** Whether `stage` was read for this view (an empty one is then really empty). */
    read: (stage: Stage) => view().read[stage],
    total: (stage: Stage) => counts()?.[stage] ?? view().rows[stage].length,
    remaining: (stage: Stage) => view().next[stage] ? Math.max(1, (counts()?.[stage] ?? 0) - view().rows[stage].length) : 0,
    find: (uuid: string) => key() ? STAGES.map((s) => view().rows[s].find((t) => t.uuid === uuid)).find(Boolean) : undefined,
    /**
     * Shows `scope`'s view of `nextProject` (none: no view), reading the
     * `stages` the board shows that this view has not read since the list
     * last changed. Call it untracked.
     */
    show(nextProject: string | undefined, nextScope: TaskScope | undefined, stages: readonly Stage[]) {
      const next = nextProject && nextScope ? [nextProject, nextScope.host, nextScope.sessionId].join("\n") : "";
      if (next !== key()) {
        generation++;
        project = nextProject ?? ""; scope = nextScope; countsAt = change;
        extent = none(0); fresh = none(false); again = none(false);
        batch(() => {
          setKey(next); setView({ rows: none([]), next: none(null), read: none(false) }); setCounts(null);
          setLoading(none(false)); setError(""); setUnsupported(false);
        });
      }
      watched = stages;
      for (const s of stages) if (!fresh[s]) refresh(s);
    },
    /** The project's list changed: every stage is read again when shown. */
    changed() {
      change++;
      fresh = none(false);
      for (const s of watched) refresh(s);
    },
    /**
     * Nothing shows the view now (the Tasks panel closed, zerus-fgyiy): no
     * stage is read until `show` names stages again, and reads in flight are
     * dropped. Its rows stay for the write guards.
     */
    suspend() {
      watched = [];
      for (const s of STAGES) {
        if (!loading()[s]) continue;
        reads[s]++; fresh[s] = false; again[s] = false;
      }
      setLoading(none(false));
    },
    /** The next page of `stage` in this view. */
    async more(stage: Stage) {
      const after = view().next[stage];
      if (!key() || !after || loading()[stage]) return;
      const at = generation, mine = ++reads[stage], seen = change;
      busy(stage, true);
      try {
        const page = await read(project, stage, window, after, scope!) as Page;
        if (at !== generation || mine !== reads[stage]) return;
        const rows = unique([...view().rows[stage], ...(Array.isArray(page.tasks) ? page.tasks : [])], stage);
        extent = { ...extent, [stage]: rows.length };
        counted(page, seen);
        batch(() => { placed(stage, rows, text(object(page.page).next) || null); setError(""); });
      } catch (e) {
        if (at === generation && mine === reads[stage]) failed(e);
      } finally {
        if (at === generation && mine === reads[stage]) settle(stage);
      }
    },
  };
}
