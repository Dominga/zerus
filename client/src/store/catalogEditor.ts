import { createSignal } from "solid-js";
import * as bridge from "../bridge";
import { tr } from "../i18n";
import { teamConfirm } from "./teamConfirm";
import type { CatalogEdit, CatalogEditorView, CatalogKind, CatalogList, CatalogRemoveImpact, CatalogSchema, ComputerId, TeamConfirmation, TeamOutcome } from "../types";

// Task #44: Qt's catalog editor (TeamCatalogEditor) for the Tauri client.
// Core (client/core/src/teams/catalog_edit.rs) keeps the working copy, checks
// every edit and writes it through the guarded `hgs team catalog set`; this
// store only asks and shows. Revert, deletions, Reset planning rules, Apply
// and Discard draft ask the owner first, Cancel focused (TeamConfirmDialog).

export const LISTS: Record<CatalogKind, CatalogList> = { role: "roles", template: "templates", rule: "routing", pipeline: "pipelines" };

/** The words of a delete confirmation, as Qt's editor asks them. */
export function removeConfirmation(kind: CatalogKind, impact: CatalogRemoveImpact): TeamConfirmation {
  switch (kind) {
    case "role": {
      let text = impact.templates.length
        ? tr("Delete the role %1? These templates lose it: %2.", impact.name, impact.templates.join(", "))
        : tr("Delete the role %1 from the catalog?", impact.name);
      if (impact.pipelines.length) {
        const names = impact.pipelines.map((name) => (impact.emptied.includes(name) ? tr("%1 (left with no stage)", name) : name));
        text += " " + tr("These pipelines lose its stages: %1.", names.join("; "));
      }
      text += " " + tr("Running teams keep their members.");
      return { name: "teamRoleDeleteConfirm", title: tr("Delete role"), text, action: tr("Delete") };
    }
    case "template":
      return { name: "teamTemplateDeleteConfirm", title: tr("Delete template"), text: tr("Delete the template %1 and its routing rules?", impact.name), action: tr("Delete") };
    case "pipeline":
      return {
        name: "teamPipelineDeleteConfirm",
        title: tr("Delete pipeline"),
        text: impact.templates.length
          ? tr("Delete the pipeline %1? These lose it as their default: %2. Running tasks keep their snapshot.", impact.name, impact.templates.join(", "))
          : tr("Delete the pipeline %1? Running tasks keep their snapshot.", impact.name),
        action: tr("Delete"),
      };
    case "rule":
      return { name: "teamRuleDeleteConfirm", title: tr("Delete rule"), text: tr("Delete the routing rule %1?", impact.name), action: tr("Delete") };
  }
}

export interface CatalogEditorPort {
  schema: typeof bridge.catalogEditorSchema;
  load: typeof bridge.catalogEditorLoad;
  edit: typeof bridge.catalogEditorEdit;
  revert: typeof bridge.catalogEditorRevert;
  save: typeof bridge.catalogEditorSave;
  resetRules: typeof bridge.catalogEditorResetRules;
  impact: typeof bridge.catalogEditorImpact;
  exportFile: typeof bridge.catalogEditorExport;
  importFile: typeof bridge.catalogEditorImport;
  route: typeof bridge.catalogRoute;
  ask: (confirmation: TeamConfirmation) => Promise<boolean>;
}

// The bridge is read when a call is made, not when this module loads: a test
// that mocks the bridge without these functions can still import it.
export const desktopCatalogEditor: CatalogEditorPort = {
  schema: (...args) => bridge.catalogEditorSchema(...args),
  load: (...args) => bridge.catalogEditorLoad(...args),
  edit: (...args) => bridge.catalogEditorEdit(...args),
  revert: (...args) => bridge.catalogEditorRevert(...args),
  save: (...args) => bridge.catalogEditorSave(...args),
  resetRules: (...args) => bridge.catalogEditorResetRules(...args),
  impact: (...args) => bridge.catalogEditorImpact(...args),
  exportFile: (...args) => bridge.catalogEditorExport(...args),
  importFile: (...args) => bridge.catalogEditorImport(...args),
  route: (...args) => bridge.catalogRoute(...args),
  ask: (confirmation) => teamConfirm.ask(confirmation),
};

const message = (error: unknown) => (error instanceof Error ? error.message : String(error));

/** One computer's catalog editor. */
export function createCatalogEditor(port: CatalogEditorPort = desktopCatalogEditor, computer: ComputerId = "") {
  const [view, setView] = createSignal<CatalogEditorView | undefined>();
  const [schema, setSchema] = createSignal<CatalogSchema | undefined>();
  const [busy, setBusy] = createSignal(false);
  /** Qt's header line: what the last call did, or why it failed. */
  const [notice, setNotice] = createSignal<{ text: string; error: boolean } | undefined>();
  // An unknown save outcome keeps its request id: sending it again is answered by the stored receipt.
  let pendingSave: string | null = null;
  let pendingReset: string | null = null;

  const say = (text: string, error = false) => setNotice({ text, error });
  const run = async <T,>(work: () => Promise<T>): Promise<T | undefined> => {
    setBusy(true);
    try {
      return await work();
    } catch (error) {
      say(message(error), true);
      return undefined;
    } finally {
      setBusy(false);
    }
  };

  async function load() {
    if (!schema()) {
      const loaded = await run(port.schema);
      if (loaded) setSchema(loaded);
    }
    const loaded = await run(() => port.load(computer));
    if (loaded) setView(loaded);
  }

  /** One edit; core refuses a wrong one and nothing changes. */
  async function edit(change: CatalogEdit): Promise<boolean> {
    try {
      setView(await port.edit(computer, change));
      setNotice(undefined);
      return true;
    } catch (error) {
      say(message(error), true);
      return false;
    }
  }

  async function remove(kind: CatalogKind, id: string) {
    const impact = await run(() => port.impact(computer, kind, id));
    if (!impact || !(await port.ask(removeConfirmation(kind, impact)))) return;
    await edit({ op: "remove", kind, id });
  }

  async function revert() {
    const unsaved = view()?.unsaved ?? 0;
    if (unsaved > 0 && !(await port.ask({ name: "teamCatalogRevertConfirm", title: tr("Revert catalog"), text: unsaved === 1 ? tr("Drop %1 unsaved change?", unsaved) : tr("Drop %1 unsaved changes?", unsaved), action: tr("Revert") }))) return;
    const reverted = await run(() => port.revert(computer));
    if (reverted) setView(reverted);
  }

  const settle = async (outcome: TeamOutcome | undefined, done: (version: unknown) => string, keep: (id: string | null) => void) => {
    if (!outcome) return;
    if (outcome.outcome === "done") {
      keep(null);
      say(done(outcome.receipt.version ?? ""));
    } else if (outcome.outcome === "uncertain") {
      keep(outcome.request_id);
      say(outcome.detail, true);
    } else if (outcome.outcome === "failed") {
      keep(null);
      say(outcome.detail, true);
    }
    // Saved, or refused by a newer version: core read it again (edits rebased).
    const loaded = await run(() => port.load(computer));
    if (loaded) setView(loaded);
  };

  async function save() {
    say(tr("Saving…"));
    const outcome = await run(() => port.save(computer, pendingSave));
    await settle(outcome, (version) => tr("Saved as v%1", String(version)), (id) => { pendingSave = id; });
  }

  async function resetRules() {
    const current = view();
    if (!current || current.unsaved > 0) return;
    if (!(await port.ask({ name: "teamPlanningRulesResetConfirm", title: tr("Reset planning rules"), text: tr("Replace the planning rules with the built-in ones and save the catalog as v%1?", current.version + 1), action: tr("Reset") }))) return;
    const outcome = await run(() => port.resetRules(computer, pendingReset));
    await settle(outcome, (version) => tr("Planning rules reset to built-in, saved as v%1", String(version)), (id) => { pendingReset = id; });
  }

  async function exportFile() {
    const path = await run(() => port.exportFile(computer));
    if (path) say(tr("Exported to %1", path));
  }

  async function importFile() {
    const imported = await run(() => port.importFile(computer));
    if (imported) {
      setView(imported);
      say(tr("Imported; Save catalog writes it as the next version."));
    }
  }

  return { view, schema, busy, notice, load, edit, remove, revert, save, resetRules, exportFile, importFile, route: (goal: string) => port.route(computer, goal) };
}

export type CatalogEditorStore = ReturnType<typeof createCatalogEditor>;
