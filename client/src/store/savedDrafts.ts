import { batch, createSignal } from "solid-js";
import { createStore, produce } from "solid-js/store";
import type { ConflictsPage, DraftSource, SavedDraftDetail, SavedDraftItem, SavedDraftsPage } from "../types";
import type { Session } from "../ui/sessions/types";

// Saved drafts (Qt's MessageComposer::showSavedDrafts, MessageComposer.cpp
// 1230-1387) over src-tauri's composer/saved.rs. Three sections: this
// computer's drafts, Qt's ("From the previous Zerus app") and the copies the
// switch import left where they were. Each page is one bounded read of at
// most PAGE drafts; Show more reads the next on request only, and closing the
// dialog drops every row, so nothing loads a whole store or grows by itself.
// Qt's store and the dev app's are only read.

/** Drafts per page (Rust clamps to 1-200). */
export const PAGE = 100;

export const TEXTS = {
  title: "Saved drafts",
  hint: "Drafts stay on this desktop, including those from ended or unavailable sessions. Restoring never sends a message.",
  empty: "No saved drafts on this desktop.",
  loading: "Loading draft…",
  unreachable: "This session is not in the session list now.",
  replaceTitle: "Replace current draft?",
  replaceConfirm: "Replace draft",
  recovered: "Recovered to this computer.",
} as const;

/** "Replace the unsent draft in {key} with the selected saved draft?" (MessageComposer.cpp:1365), the key's lines joined by " / ". */
export const replaceText = (key: string) => `Replace the unsent draft in ${key.replaceAll("\n", " / ")} with the selected saved draft?`;

export type SectionId = "client" | "qt" | "conflicts";
export const SECTIONS: { id: SectionId; title: string; note?: string }[] = [
  { id: "client", title: "On this computer" },
  { id: "qt", title: "From the previous Zerus app" },
  { id: "conflicts", title: "Not imported at the switch", note: "A different draft was already here, so these copies stayed where they were." },
];

interface Section {
  items: SavedDraftItem[];
  /** The next page's token (client, qt) or offset (conflicts); null when none is left. */
  next: string | number | null;
  loading: boolean;
  /** Its first page answered. */
  loaded: boolean;
}

export interface SavedDraftsPort {
  page(source: "client" | "qt", limit: number, before: string | null): Promise<SavedDraftsPage>;
  conflicts(limit: number, after: number): Promise<ConflictsPage>;
  detail(source: DraftSource, key: string): Promise<SavedDraftDetail>;
  recover(key: string): Promise<void>;
  restore(handle: number, source: DraftSource, key: string): Promise<void>;
}

/** What Restore needs of the open composer (store/composer.ts). */
export interface RestoreTarget {
  handle: number;
  key: string;
  /** The editor holds text or files (Qt's current draft is not empty). */
  hasDraft: boolean;
  /** Loaded and not sending, waiting, attaching or otherwise locked. */
  idle: boolean;
}

const errorText = (error: unknown) => (typeof error === "string" ? error : error instanceof Error ? error.message : String(error));
const emptySection = (): Section => ({ items: [], next: null, loading: false, loaded: false });

/**
 * The catalog row a draft's session is (Qt's SessionsWindow::draftOwner):
 * none for an unknown or archived owner. The row of the key's host alias
 * wins; a subagent key names its machine instead.
 */
export function draftOwnerRow(item: SavedDraftItem, rows: Iterable<Session>): Session | undefined {
  if (item.kind === "unknown" || item.archived || !item.session) return undefined;
  let named: Session | undefined;
  for (const s of rows) {
    if (s.name !== item.session || s.state === "archived") continue;
    if (s.computer === item.computer) return s;
    if (item.computer && s.rawMachine === item.computer) named = s;
  }
  return named;
}

/** "d MMM HH:mm" in local time. */
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
export function modifiedText(seconds: number): string {
  if (!(seconds > 0)) return "";
  const d = new Date(seconds * 1000);
  return `${d.getDate()} ${MONTHS[d.getMonth()]} ${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

export function createSavedDrafts(port: SavedDraftsPort) {
  const [open, setOpen] = createSignal(false);
  const [sections, setSections] = createStore<Record<SectionId, Section>>({ client: emptySection(), qt: emptySection(), conflicts: emptySection() });
  const [selected, setSelected] = createSignal<SavedDraftItem>();
  /** The selected draft's preview; undefined while it loads ("Loading draft…"). */
  const [detail, setDetail] = createSignal<SavedDraftDetail>();
  /** Why Restore or Recover did not run, or that Recover ran. */
  const [message, setMessage] = createSignal<{ text: string; error: boolean }>();
  const [busy, setBusy] = createSignal(false);
  /** Counts opens and closes: an answer of an earlier opening is dropped. */
  let opening = 0;
  /** Counts selections (Qt's `selection`): a late preview of another draft is dropped. */
  let selection = 0;
  /** The composer the dialog opened for (Qt's `target`): Restore replaces only its draft. */
  let target: number | undefined;

  const key = (item: SavedDraftItem) => `${item.source}\n${item.key}`;

  function select(item: SavedDraftItem | undefined) {
    const mine = ++selection;
    const at = opening;
    batch(() => { setSelected(item); setDetail(undefined); setMessage(undefined); });
    if (!item) return;
    port.detail(item.source, item.key).then(
      (found) => { if (mine === selection && at === opening) setDetail(found); },
      (error) => { if (mine === selection && at === opening) setDetail({ text: "", attachments: 0, detail: errorText(error), restorable: false }); },
    );
  }

  /** The first draft listed, once nothing is selected (Qt selects row 0). */
  function selectFirst() {
    if (selected()) return;
    for (const { id } of SECTIONS) {
      const first = sections[id].items[0];
      if (first) { select(first); return; }
    }
  }

  async function load(id: SectionId) {
    const section = sections[id];
    if (section.loading || (section.loaded && section.next === null)) return;
    const at = opening;
    setSections(id, "loading", true);
    try {
      let items: SavedDraftItem[];
      let next: string | number | null;
      if (id === "conflicts") {
        const page = await port.conflicts(PAGE, typeof section.next === "number" ? section.next : 0);
        ({ items, next } = page);
      } else {
        const page = await port.page(id, PAGE, typeof section.next === "string" ? section.next : null);
        ({ items, next } = page);
      }
      if (at !== opening) return;
      setSections(id, produce((s) => {
        // A page never repeats a draft the section shows (a draft saved meanwhile moves to the front).
        const shown = new Set(s.items.map(key));
        s.items.push(...items.filter((item) => !shown.has(key(item))));
        s.next = next;
        s.loaded = true;
        s.loading = false;
      }));
      selectFirst();
    } catch {
      if (at === opening) setSections(id, produce((s) => { s.loading = false; s.loaded = true; s.next = null; }));
    }
  }

  function reset() {
    opening++;
    selection++;
    batch(() => {
      setSections({ client: emptySection(), qt: emptySection(), conflicts: emptySection() });
      setSelected(undefined);
      setDetail(undefined);
      setMessage(undefined);
      setBusy(false);
    });
  }

  return {
    open, sections, selected, detail, message, busy,
    /** Every section's first page answered and none lists a draft. */
    empty: () => SECTIONS.every(({ id }) => sections[id].loaded && sections[id].items.length === 0),
    /** Opens for the composer `handle` (undefined: Restore stays off). */
    show(handle: number | undefined) {
      reset();
      target = handle;
      setOpen(true);
      for (const { id } of SECTIONS) void load(id);
    },
    /** Closes and drops every row. */
    close() {
      reset();
      target = undefined;
      setOpen(false);
    },
    /** Show more: the section's next page, on request only. */
    more: load,
    select,
    /** Restore in current session is enabled (MessageComposer.cpp:1321-1323). */
    canRestore(composer: RestoreTarget | undefined): boolean {
      const d = detail();
      return !!composer && composer.handle === target && composer.idle && !!selected() && !!d && d.restorable && !busy();
    },
    /**
     * Whether Restore asks "Replace current draft?" first: the open draft is
     * not empty and the selection is not that very draft (1362-1364).
     */
    asks(composer: RestoreTarget): boolean {
      const item = selected();
      return composer.hasDraft && !(item?.source === "client" && item.key === composer.key);
    },
    /** Restores the selection into the composer; true when the dialog may close. */
    async restore(composer: RestoreTarget | undefined): Promise<boolean> {
      const item = selected();
      if (!item || !composer || composer.handle !== target || !composer.idle) return false;
      setBusy(true);
      try {
        await port.restore(composer.handle, item.source, item.key);
        return true;
      } catch (error) {
        setMessage({ text: errorText(error), error: true });
        return false;
      } finally {
        setBusy(false);
      }
    },
    /** Recover (Qt's drafts only): a copy in this computer's store, never over a draft here. */
    async recover() {
      const item = selected();
      if (!item || item.source !== "qt" || busy()) return;
      const at = opening;
      setBusy(true);
      try {
        await port.recover(item.key);
        if (at !== opening) return;
        setMessage({ text: TEXTS.recovered, error: false });
        // The recovered draft now heads this computer's section.
        setSections("client", emptySection());
        void load("client");
      } catch (error) {
        if (at === opening) setMessage({ text: errorText(error), error: true });
      } finally {
        if (at === opening) setBusy(false);
      }
    },
  };
}

export type SavedDrafts = ReturnType<typeof createSavedDrafts>;
