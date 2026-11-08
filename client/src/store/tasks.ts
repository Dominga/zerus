import { batch, createSignal } from "solid-js";
import { followTasks, readTaskPage, type TaskScope } from "../bridge";
import type { TasksOut } from "../types";
import { STAGES, TASK_LIMIT, object, taskItem, text, type Stage, type Task } from "../ui/screens/tasks-ui";
import { tr } from "../i18n";
import { createSessionView } from "./sessionTasks";

/** Rows of each stage the board follows (the core's window) and asks for per further page. */
export const WINDOW = 50;
/** The most rows one further page reads (`task list --limit`). */
export const PAGE_LIMIT = 200;
/** Tasks read on their own that the store keeps for the write guards. */
export const ADOPTED = 16;
/** The most pages one re-read of an extended stage requests; Show more continues past it. */
const MAX_REREAD_PAGES = 64;

export type Page = { tasks: unknown[]; page: { next: unknown } };
type ReadPage = (project: string, stage: Stage, limit: number, after: string | null, scope?: TaskScope) => Promise<unknown>;

const none = <T,>(value: T) => Object.fromEntries(STAGES.map((s) => [s, value])) as Record<Stage, T>;

/**
 * One project's tasks, replaced atomically by the event stream. A server that
 * windows the list sends the first rows of each stage with each stage's total
 * and next cursor; Show more reads further pages, and a stage the user
 * extended is read again to that extent when the window changes, keeping its
 * rows meanwhile. An older server sends the whole list, as before.
 */
export function createTasks(follow: (project: string | null) => Promise<void>, readPage: ReadPage = readTaskPage) {
  const [project, setProject] = createSignal<string>();
  const [tasks, setTasks] = createSignal<Task[]>([]);
  const [loaded, setLoaded] = createSignal(false);
  const [error, setError] = createSignal("");
  const [truncated, setTruncated] = createSignal(false);
  const [damaged, setDamaged] = createSignal(0);
  /** Only the first five kept entries from the server's bounded damaged page. */
  const [keptDamaged, setKeptDamaged] = createSignal<{ uuid: string; error: string }[]>([]);
  const [windowed, setWindowed] = createSignal(false);
  const [totals, setTotals] = createSignal(none(0));
  /** The published rows of each stage: `total` and `remaining` track them through this signal. */
  const [shown, setShown] = createSignal(none(0), { equals: (a, b) => STAGES.every((s) => a[s] === b[s]) });
  const [loading, setLoading] = createSignal(none(false));
  const [pageError, setPageError] = createSignal("");
  /** Tasks read on their own (a selected task beyond the loaded rows), at most `ADOPTED`. */
  const [adopted, setAdopted] = createSignal<Map<string, Task>>(new Map());
  let rows = none<Task[]>([]), next = none<string | null>(null), extent = none(0), reads = none(0);
  /** A whole list, published in the order the server sent it. */
  let whole: Task[] | null = null;
  let sent = Promise.resolve(), generation = 0;
  /** Only this session: hgs's own pages of one session's view. */
  const session = createSessionView((p, s, limit, after, scope) => readPage(p, s, limit, after, scope), WINDOW, PAGE_LIMIT, MAX_REREAD_PAGES);

  /** The stages in board order, each task once, To do numbered by its position. */
  const publish = () => {
    let queue = 0;
    const seen = new Set<string>();
    const ordered = (whole ?? STAGES.flatMap((s) => rows[s])).filter((t) => !seen.has(t.uuid) && !!seen.add(t.uuid));
    const counts = none(0);
    for (const t of ordered) if (t.stage in counts) counts[t.stage as Stage]++;
    const published = ordered.map((t) => t.stage === "todo" ? { ...t, queue: ++queue } : t);
    batch(() => { setTasks(published); setShown(counts); });
  };
  /** `found` as the rows of `stage`: the freshest read of those tasks, so no other stage keeps them. */
  const place = (stage: Stage, found: Task[]) => {
    const ids = new Set(found.map((t) => t.uuid));
    rows = Object.fromEntries(STAGES.map((s) => [s, s === stage ? found : rows[s].filter((t) => !ids.has(t.uuid))])) as Record<Stage, Task[]>;
  };
  /** Ends every read in flight for `stage`: its page belongs to rows that were replaced. */
  const abandon = (stage: Stage) => { reads[stage]++; if (loading()[stage]) busy(stage, false); };
  const unique = (values: unknown[], stage?: Stage) => {
    const seen = new Set<string>();
    return values.flatMap((value) => {
      const t = taskItem(value); if (!t.uuid || seen.has(t.uuid) || (stage && t.stage !== stage)) return [];
      seen.add(t.uuid); return [t];
    });
  };
  const busy = (stage: Stage, on: boolean) => setLoading({ ...loading(), [stage]: on });

  /** Reads `stage` from its start up to `want` rows, replacing its rows when this is still the latest read. */
  async function reread(stage: Stage, want: number) {
    const mine = ++reads[stage], owner = project(), at = generation;
    busy(stage, true);
    try {
      let found: Task[] = [], after: string | null = null, pages = 0;
      do {
        const page = await readPage(owner!, stage, Math.min(PAGE_LIMIT, want - found.length), after) as Page;
        // A newer read or project supersedes this one: stop reading.
        if (at !== generation || mine !== reads[stage]) return;
        const cursor = text(page.page.next) || null;
        found = unique([...found, ...page.tasks], stage);
        // A cursor that does not advance ends the read rather than looping on it.
        after = cursor && cursor !== after ? cursor : null;
      } while (after && found.length < want && ++pages < MAX_REREAD_PAGES);
      place(stage, found); next = { ...next, [stage]: after }; publish(); setPageError("");
    } catch (e) {
      if (at === generation && mine === reads[stage]) setPageError(String(e));
    } finally {
      if (at === generation && mine === reads[stage]) busy(stage, false);
    }
  }

  return {
    project, tasks, loaded, error, truncated, damaged, keptDamaged, windowed, loading, pageError, session,
    /** A task by UUID: a loaded row, else a row of the session view, else one read on its own. */
    find: (uuid: string) => tasks().find((t) => t.uuid === uuid) ?? session.find(uuid) ?? adopted().get(uuid),
    /**
     * Keeps a task read on its own for the write guards; the oldest goes
     * first. Later windows do not refresh it: the guards compare it with an
     * equally old card, and hgs's own checks of each write are the real guard.
     */
    adopt(task: Task) {
      const next = new Map(adopted()); next.delete(task.uuid); next.set(task.uuid, task);
      while (next.size > ADOPTED) next.delete(next.keys().next().value!);
      setAdopted(next);
    },
    /** Every task of a stage, or only those loaded when the list is not windowed. */
    total: (stage: Stage) => windowed() ? totals()[stage] : shown()[stage],
    /** Rows of a stage not loaded yet. */
    remaining: (stage: Stage) => windowed() ? Math.max(0, totals()[stage] - shown()[stage]) : 0,
    follow(nextProject: string | undefined) {
      if (nextProject === project()) return;
      setKeptDamaged([]);
      const mine = ++generation;
      setProject(nextProject); setTasks([]); setShown(none(0)); setLoaded(false); setError(""); setTruncated(false); setDamaged(0);
      setWindowed(false); setTotals(none(0)); setLoading(none(false)); setPageError(""); setAdopted(new Map());
      rows = none([]); next = none(null); extent = none(0); whole = null;
      session.show(undefined, undefined, []);
      sent = sent.then(() => follow(nextProject ?? null)).catch((e) => { if (mine === generation) { setError(String(e)); setLoaded(true); } });
    },
    /** The next page of `stage`; one read at a time per stage. */
    async more(stage: Stage) {
      const after = next[stage], owner = project(), at = generation;
      if (!windowed() || !after || !owner || loading()[stage]) return;
      const mine = ++reads[stage];
      busy(stage, true);
      try {
        const page = await readPage(owner, stage, WINDOW, after) as Page;
        if (at !== generation || mine !== reads[stage]) return;
        place(stage, unique([...rows[stage], ...page.tasks], stage));
        next = { ...next, [stage]: text(page.page.next) || null };
        extent = { ...extent, [stage]: rows[stage].length };
        publish(); setPageError("");
      } catch (e) {
        if (at === generation && mine === reads[stage]) setPageError(String(e));
      } finally {
        if (at === generation && mine === reads[stage]) busy(stage, false);
      }
    },
    apply(updates: TasksOut[]) {
      for (const update of updates) {
        if (update.computer !== "" || update.key !== project()) continue;
        const p = object(update.payload);
        if (update.kind === "list") {
          if (text(object(p.project).id) !== project() || !Array.isArray(p.tasks)) continue;
          setKeptDamaged((Array.isArray(p.damaged) ? p.damaged : []).slice(0, 20)
            .map(object).filter((row) => row.kept === true && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(text(row.uuid)))
            .slice(0, 5).map((row) => ({ uuid: text(row.uuid), error: text(row.error).slice(0, 512) })));
          const summary = object(p.window), stages = object(object(summary.window).stages);
          if (Object.keys(stages).length) {
            // A window: each stage's first rows; extended stages keep their
            // rows and are read again to the extent the user loaded. The
            // window's rows win over any row kept from earlier reads.
            const first = unique(p.tasks), current = new Set(first.map((t) => t.uuid));
            whole = null; setWindowed(true);
            setTotals(Object.fromEntries(STAGES.map((s) => [s, Number(object(stages[s]).total) || 0])) as Record<Stage, number>);
            for (const s of STAGES) {
              const head = first.filter((t) => t.stage === s), cursor = text(object(stages[s]).next) || null;
              if (extent[s] > head.length && cursor) {
                const kept = rows[s].filter((t) => !current.has(t.uuid));
                rows = { ...rows, [s]: [...head, ...kept] };
                void reread(s, extent[s]);
              } else {
                abandon(s);
                rows = { ...rows, [s]: head }; next = { ...next, [s]: cursor }; extent = { ...extent, [s]: 0 };
              }
            }
            setTruncated(false);
            setDamaged(Number(summary.damaged_total) || (Array.isArray(p.damaged) ? p.damaged.length : 0));
          } else {
            setWindowed(false); STAGES.forEach(abandon);
            const all = unique(p.tasks.slice(0, TASK_LIMIT));
            whole = all;
            rows = Object.fromEntries(STAGES.map((s) => [s, all.filter((t) => t.stage === s)])) as Record<Stage, Task[]>;
            next = none(null); extent = none(0);
            setTruncated(p.truncated === true || p.tasks.length > TASK_LIMIT);
            setDamaged(Array.isArray(p.damaged) ? p.damaged.length : 0);
          }
          publish(); setError(""); setLoaded(true);
          session.changed();
        } else if (update.kind === "failed") { setError(text(p.message) || tr("Tasks could not be read.")); setLoaded(true); }
        else if (update.kind === "ended") { setError(tr("The task subscription ended. Reopen Tasks to reconnect.")); setLoaded(true); }
      }
    },
  };
}
export const tasks = createTasks((project) => followTasks(project));
