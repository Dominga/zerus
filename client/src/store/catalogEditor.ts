// Adapted from WhiteWh's catalogEditor store. Our catalog is person-owned;
// an uncertain save is checked by its point receipt, never submitted again.
import { createRoot, createSignal } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { newRequestId, teams } from "./teams";

export type Item = Record<string, unknown>;
export type CatalogList = "roles" | "pipelines" | "templates";
export interface Catalog extends Item { roles?: Item[]; pipelines?: Item[]; templates?: Item[] }
export interface FieldSpec { name: string; label: string; type: string; default?: unknown; fields?: FieldSpec[]; options?: string[] }
export interface CatalogSnapshot { revision: string; person: Catalog; catalog: Catalog; builtin: Catalog; schema_fields: Record<CatalogList, FieldSpec[]> }
export interface CatalogSave { schema: 1; request_id: string; expected_revision: string; catalog: Catalog }
export interface CatalogOutcome { outcome: "done" | "failed" | "uncertain"; request_id: string; revision?: string; detail?: string; code?: string }
export interface CatalogPort {
  load(): Promise<CatalogSnapshot>;
  validate(value: Catalog, base: Catalog): Promise<Catalog>;
  importFile(): Promise<unknown | null>;
  import(value: unknown, current: Catalog, base: Catalog): Promise<Catalog>;
  exportFile(value: Catalog): Promise<boolean>;
  exportTemplate?(id: string): Promise<boolean>;
  save(edit: CatalogSave): Promise<CatalogOutcome>;
  receipt(request: string): Promise<CatalogOutcome>;
  changed?(): void;
}
export const desktopCatalog: CatalogPort = {
  load: () => invoke("catalog_editor_load"),
  validate: (value, base) => invoke("catalog_editor_validate", { value, base }),
  importFile: () => invoke("catalog_editor_import_file"),
  import: (value, current, base) => invoke("catalog_editor_import", { value, current, base }),
  exportFile: (value) => invoke("catalog_editor_export", { value }),
  exportTemplate: (id) => invoke("catalog_editor_export_template", { id }),
  save: (edit) => invoke("catalog_editor_save", { edit }),
  receipt: (request) => invoke("catalog_editor_receipt", { request }),
  changed: () => { void teams.reloadCatalog(); },
};
const copy = <T,>(value: T): T => JSON.parse(JSON.stringify(value));
const message = (error: unknown) => error instanceof Error ? error.message : String(error);

/** One bounded working copy, retained when the page closes. No polling, no
 * automatic save, no implicit rebase or retry of an uncertain write. */
export function createCatalogEditor(port: CatalogPort = desktopCatalog) {
  const [snapshot, setSnapshot] = createSignal<CatalogSnapshot>();
  const [working, setWorking] = createSignal<Catalog>({});
  const [busy, setBusy] = createSignal(false);
  const [notice, setNotice] = createSignal("");
  const [pending, setPending] = createSignal<CatalogSave>();
  const [confirmation, setConfirmation] = createSignal<(() => void) | undefined>();
  const dirty = () => !!snapshot() && JSON.stringify(working()) !== JSON.stringify(snapshot()!.person);
  const locked = () => busy() || !!pending() || !!confirmation();
  async function run<T>(work: () => Promise<T>): Promise<T | undefined> {
    if (busy()) return;
    setBusy(true);
    try { return await work(); } catch (error) { setNotice(message(error)); } finally { setBusy(false); }
  }
  async function load() {
    if (snapshot()) return;
    await run(async () => { const got = await port.load(); setSnapshot(got); setWorking(copy(got.person)); });
  }
  function replace(value: Catalog) { if (!locked()) {
    if (new TextEncoder().encode(JSON.stringify(value, null, 2) + "\n").length > 240_000 || [value.roles, value.pipelines, value.templates].some((rows) => rows && rows.length > 200)) { setNotice("The catalog exceeds the editor size limit."); return; }
    setWorking(copy(value)); setNotice("");
  } }
  function items(kind: CatalogList): Item[] {
    const own = working()[kind] ?? [], base = snapshot()?.builtin[kind] ?? [];
    return [...base.map((row) => own.find((over) => over.id === row.id) ?? row), ...own.filter((row) => !base.some((b) => b.id === row.id))];
  }
  function setItem(kind: CatalogList, id: string, row: Item) {
    if (locked()) return;
    const rows = [...(working()[kind] ?? [])], at = rows.findIndex((item) => item.id === id);
    if (at < 0) rows.push(copy(row)); else rows[at] = copy(row);
    replace({ ...working(), [kind]: rows });
  }
  function remove(kind: CatalogList, id: string) {
    if (locked()) return;
    setConfirmation(() => () => { replace({ ...working(), [kind]: (working()[kind] ?? []).filter((row) => row.id !== id) }); });
  }
  async function revert() {
    if (locked()) return;
    const reset = () => { void run(async () => { const got = await port.load(); setSnapshot(got); setWorking(copy(got.person)); setNotice(""); }); };
    if (dirty()) setConfirmation(() => reset); else reset();
  }
  function confirm() { const action = confirmation(); setConfirmation(undefined); action?.(); }
  function cancel() { setConfirmation(undefined); }
  function accept(outcome: CatalogOutcome, intent: CatalogSave) {
    if (outcome.request_id !== intent.request_id) { setNotice("The save result is not known."); return; }
    setNotice(outcome.detail ?? outcome.outcome);
    if (outcome.outcome === "done" && outcome.revision) {
      setSnapshot({ ...snapshot()!, person: copy(intent.catalog), revision: outcome.revision });
      setPending(undefined);
      port.changed?.();
    } else if (outcome.outcome === "failed") setPending(undefined);
  }
  async function save() {
    if (locked() || !snapshot() || !dirty()) return;
    await run(async () => {
      const catalog = await port.validate(copy(working()), snapshot()!.builtin);
      const intent: CatalogSave = { schema: 1, request_id: newRequestId(), expected_revision: snapshot()!.revision, catalog };
      setWorking(copy(catalog));
      setPending(intent); // reserve before the first await that can write
      try { accept(await port.save(intent), intent); } catch (error) { setNotice(message(error)); }
    });
  }
  async function check() {
    const intent = pending();
    if (!intent) return;
    await run(async () => {
      const outcome = await port.receipt(intent.request_id);
      if (outcome.outcome === "failed") { setNotice(outcome.detail ?? "The save result is not known."); return; }
      accept(outcome, intent);
    });
  }
  function discardUncertain() {
    if (busy() || !pending()) return;
    setConfirmation(() => () => { void run(async () => {
      const got = await port.load();
      setSnapshot(got); setWorking(copy(got.person)); setPending(undefined); setNotice("");
    }); });
  }
  async function importFile() {
    if (locked() || !snapshot()) return;
    await run(async () => {
      const value = await port.importFile();
      if (value === null) return;
      const next = await port.import(value, working(), snapshot()!.builtin);
      // Import is a draft until the person saves it. Confirm replacement.
      setConfirmation(() => () => replace(next));
    });
  }
  async function exportFile() {
    await run(async () => { await port.exportFile(copy(working())); });
  }
  async function exportTemplate(id: string) {
    if (dirty() || locked()) return;
    await run(async () => { await port.exportTemplate?.(id); });
  }
  return { snapshot, working, busy, notice, pending, dirty, locked, confirmation, confirm, cancel, load, replace, items, setItem, remove, revert, save, check, discardUncertain, importFile, exportFile, exportTemplate };
}
export type CatalogEditorStore = ReturnType<typeof createCatalogEditor>;
export const catalogEditor = createRoot(() => createCatalogEditor());
