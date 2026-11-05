import { For, Index, Show, createEffect, createMemo, createSignal, on, onMount } from "solid-js";
import type { JSX } from "solid-js";
import { tr, trNoop } from "../../i18n";
import { Icon } from "../lib/icons";
import { LISTS, createCatalogEditor, type CatalogEditorStore } from "../../store/catalogEditor";
import type { CatalogFieldSpec, CatalogKind, CatalogList } from "../../types";
import "../styles/catalog.css";

// Task #44: Qt's TeamCatalogEditor. The header holds Open Team designer,
// Import…, Export…, Revert, Save catalog, Discard draft and Apply
// (TeamCatalogEditor.cpp:517-523); the tabs edit roles, templates, routing
// rules, the lead's planning rules and pipelines. Every field comes from
// core's schema and every edit is checked there: nothing here reads or builds
// a catalog. Confirmations go through TeamConfirmDialog, Cancel focused.

type Tab = "roles" | "templates" | "routing" | "rules" | "pipelines";
type Item = Record<string, unknown>;

const TABS: { id: Tab; label: string }[] = [
  { id: "roles", label: trNoop("Roles") },
  { id: "templates", label: trNoop("Templates") },
  { id: "routing", label: trNoop("Routing") },
  { id: "rules", label: trNoop("Rules") },
  { id: "pipelines", label: trNoop("Pipelines") },
];
const KIND: Partial<Record<Tab, CatalogKind>> = { roles: "role", templates: "template", routing: "rule", pipelines: "pipeline" };
const ADD: Partial<Record<Tab, string>> = { roles: trNoop("+ Role"), templates: trNoop("+ Template"), routing: trNoop("+ Rule"), pipelines: trNoop("+ Pipeline") };

export interface CatalogDraftControls {
  kept: number;
  undecided: number;
  apply: () => void;
  discard: () => void;
}

export interface TeamCatalogEditorProps {
  /** The editor's store; one of its own when absent (this computer). */
  editor?: CatalogEditorStore;
  /** The followed catalog's version: a newer one is read again, unsaved edits rebased. */
  savedVersion?: number;
  /** The designer's draft, when there is one: Discard draft and Apply. */
  draft?: CatalogDraftControls;
  onOpenDesigner?: () => void;
}

const str = (value: unknown) => (typeof value === "string" ? value : value == null ? "" : String(value));
const words = (value: unknown) => (Array.isArray(value) ? value.map(str).join(", ") : str(value));
const splitWords = (text: string) => text.split(",").map((word) => word.trim()).filter(Boolean);

export function TeamCatalogEditor(props: TeamCatalogEditorProps): JSX.Element {
  const editor = props.editor ?? createCatalogEditor();
  const [tab, setTab] = createSignal<Tab>("roles");
  const [current, setCurrent] = createSignal<Partial<Record<Tab, string>>>({});
  onMount(() => void editor.load());
  createEffect(on(() => props.savedVersion, (version, previous) => { if (version !== undefined && previous !== undefined && version !== previous) void editor.load(); }));

  const list = (which: CatalogList): Item[] => (editor.view()?.[which] as Item[] | undefined) ?? [];
  const items = createMemo(() => { const kind = KIND[tab()]; return kind ? list(LISTS[kind]) : []; });
  const chosen = createMemo(() => {
    const all = items(), id = current()[tab()];
    return all.find((item) => item.id === id) ?? all[0];
  });
  const choose = (id: string) => setCurrent({ ...current(), [tab()]: id });
  const unsaved = () => editor.view()?.unsaved ?? 0;
  const edited = (which: CatalogList, id: unknown) => !!editor.view()?.edited[which]?.includes(str(id));

  const add = async () => {
    const kind = KIND[tab()];
    if (!kind) return;
    const before = new Set(items().map((item) => str(item.id)));
    if (await editor.edit({ op: "add", kind })) {
      const fresh = items().find((item) => !before.has(str(item.id)));
      if (fresh) choose(str(fresh.id));
    }
  };
  const set = (kind: CatalogKind, id: string, field: string, value: unknown) => editor.edit({ op: "set", kind, id, field, value });

  return (
    <section class="cat cat-editor" aria-label={tr("Team catalog")}>
      <header class="cat-head">
        <h2>{tr("Team catalog")}</h2>
        <span class="cat-version">{editor.view()?.builtin ? tr("Built-in catalog") : tr("v%1", editor.view()?.version ?? 0)}</span>
        <Show when={unsaved() > 0}>
          <span class="cat-unsaved">{unsaved() === 1 ? tr("%1 unsaved change", unsaved()) : tr("%1 unsaved changes", unsaved())}</span>
        </Show>
        <Show when={editor.notice()}>{(notice) => <span class="cat-notice" classList={{ error: notice().error }} role={notice().error ? "alert" : "status"}>{notice().text}</span>}</Show>
        <span class="grow" />
        <Show when={props.onOpenDesigner}>
          <button type="button" class="btn ghost small" onClick={() => props.onOpenDesigner?.()}><Icon name="users" size={13} />{tr("Open Team designer")}</button>
        </Show>
        <button type="button" class="btn ghost small" disabled={editor.busy()} onClick={() => void editor.importFile()}><Icon name="folder-open" size={13} />{tr("Import…")}</button>
        <button type="button" class="btn ghost small" disabled={editor.busy() || !editor.view()} onClick={() => void editor.exportFile()}><Icon name="download" size={13} />{tr("Export…")}</button>
        <button type="button" class="btn ghost small" disabled={editor.busy() || unsaved() === 0} onClick={() => void editor.revert()}>{tr("Revert")}</button>
        <button type="button" class="btn small primary" disabled={editor.busy() || unsaved() === 0} onClick={() => void editor.save()}>{tr("Save catalog")}</button>
        <Show when={props.draft}>
          {(draft) => (
            <>
              <button type="button" class="btn ghost small" disabled={editor.busy()} onClick={() => draft().discard()}>{tr("Discard draft")}</button>
              <button type="button" class="btn small" disabled={editor.busy() || draft().kept === 0 || unsaved() > 0} onClick={() => draft().apply()}
                title={unsaved() > 0 ? tr("Save or revert your edits first") : tr("Write the kept changes as the next catalog version")}>{tr("Apply")}</button>
            </>
          )}
        </Show>
      </header>

      <nav class="cat-tabs" role="tablist" aria-label={tr("Catalog sections")}>
        <For each={TABS}>{(entry) => (
          <button type="button" role="tab" class="cat-tab" aria-selected={tab() === entry.id} classList={{ on: tab() === entry.id }} onClick={() => setTab(entry.id)}>{tr(entry.label)}</button>
        )}</For>
      </nav>

      <Show when={editor.view() && editor.schema()} fallback={<p class="cat-empty">{editor.busy() ? tr("Reading the catalog…") : tr("The catalog is not read yet.")}</p>}>
        <Show when={tab() === "rules"} fallback={
          <Show when={tab() === "routing"} fallback={
            <div class="cat-body">
              <aside class="cat-list" aria-label={tr(TABS.find((entry) => entry.id === tab())!.label)}>
                <For each={items()}>{(item) => (
                  <button type="button" class="cat-row" classList={{ on: chosen()?.id === item.id, edited: edited(LISTS[KIND[tab()]!], item.id) }} onClick={() => choose(str(item.id))}>
                    <span class="cat-row-name">{str(item.name) || str(item.id)}</span>
                    <span class="cat-row-meta">{str(item.about ?? item.description ?? "")}</span>
                  </button>
                )}</For>
                <button type="button" class="btn ghost small cat-add" disabled={editor.busy()} onClick={() => void add()}>{tr(ADD[tab()]!)}</button>
              </aside>
              <div class="cat-form">
                <Show when={chosen()} fallback={<p class="cat-empty">{tr("Nothing here yet.")}</p>}>
                  {(item) => <ItemForm editor={editor} kind={KIND[tab()]!} item={item()} set={set} />}
                </Show>
              </div>
            </div>
          }>
            <Routing editor={editor} set={set} add={add} />
          </Show>
        }>
          <PlanningRules editor={editor} />
        </Show>
      </Show>
    </section>
  );
}

/** One item's fields, Duplicate and Delete. */
function ItemForm(props: { editor: CatalogEditorStore; kind: CatalogKind; item: Item; set: (kind: CatalogKind, id: string, field: string, value: unknown) => Promise<boolean> }) {
  const specs = () => props.editor.schema()?.[LISTS[props.kind]] ?? [];
  const id = () => str(props.item.id);
  return (
    <div class="cat-item">
      <div class="cat-item-head">
        <b>{str(props.item.name) || id()}</b>
        <span class="cat-id">{id()}</span>
        <span class="grow" />
        <button type="button" class="btn ghost small" disabled={props.editor.busy()} onClick={() => void props.editor.edit({ op: "duplicate", kind: props.kind, id: id() })}><Icon name="copy" size={12} />{tr("Duplicate")}</button>
        <button type="button" class="btn ghost small danger" disabled={props.editor.busy()} onClick={() => void props.editor.remove(props.kind, id())}><Icon name="x" size={12} />{tr("Delete")}</button>
      </div>
      <For each={specs()}>{(spec) => (
        <Field editor={props.editor} spec={spec} value={props.item[spec.name]} onChange={(value) => props.set(props.kind, id(), spec.name, value)} />
      )}</For>
    </div>
  );
}

/** The ids a field names, for a choice instead of free text. */
function references(editor: CatalogEditorStore, spec: CatalogFieldSpec): [string, string][] | undefined {
  const view = editor.view();
  if (!view) return undefined;
  const named = (entries: Record<string, unknown>[]) => entries.map((entry) => [str(entry.id), str(entry.name) || str(entry.id)] as [string, string]);
  if (spec.name === "role") return [["lead", tr("Lead")], ...named(view.roles)];
  if (spec.name === "template") return [["", tr("No template")], ...named(view.templates)];
  if (spec.name === "pipeline") return [["", tr("No pipeline")], ...named(view.pipelines)];
  return undefined;
}

/** One field, drawn from its schema type; each change goes to core at once. */
function Field(props: { editor: CatalogEditorStore; spec: CatalogFieldSpec; value: unknown; onChange: (value: unknown) => Promise<boolean> | void; compact?: boolean }) {
  const label = () => tr(props.spec.label);
  const fieldId = `cat-field-${Math.random().toString(36).slice(2)}`;
  const input = (): JSX.Element => {
    const spec = props.spec;
    const refs = spec.type === "text" ? references(props.editor, spec) : undefined;
    if (refs) {
      const value = str(props.value);
      const options = refs.some(([id]) => id === value) ? refs : [...refs, [value, value] as [string, string]];
      return <select id={fieldId} value={value} onChange={(event) => void props.onChange(event.currentTarget.value)}>
        <For each={options}>{([id, name]) => <option value={id}>{name}</option>}</For>
      </select>;
    }
    switch (spec.type) {
      case "multiline":
        return <textarea id={fieldId} rows={5} value={str(props.value)} onChange={(event) => void props.onChange(event.currentTarget.value)} />;
      case "number":
        return <input id={fieldId} type="number" min={spec.min} max={spec.max} value={str(props.value ?? spec.min ?? 0)}
          onChange={(event) => void props.onChange(Number.parseInt(event.currentTarget.value, 10) || 0)} />;
      case "bool":
        return <input id={fieldId} type="checkbox" checked={props.value === true} onChange={(event) => void props.onChange(event.currentTarget.checked)} />;
      case "choice":
        return <select id={fieldId} value={str(props.value)} onChange={(event) => void props.onChange(event.currentTarget.value)}>
          <For each={spec.options ?? []}>{([value, name]) => <option value={value}>{tr(name)}</option>}</For>
        </select>;
      case "words":
        return <input id={fieldId} value={words(props.value)} placeholder={tr("Comma-separated")} onChange={(event) => void props.onChange(splitWords(event.currentTarget.value))} />;
      case "list":
        return <Rows editor={props.editor} spec={spec} value={Array.isArray(props.value) ? (props.value as Item[]) : []} onChange={props.onChange} />;
      default:
        return <input id={fieldId} value={str(props.value)} onChange={(event) => void props.onChange(event.currentTarget.value)} />;
    }
  };
  return (
    <div class="cat-field" classList={{ compact: props.compact, wide: props.spec.type === "list" || props.spec.type === "multiline" }}>
      <label for={fieldId}>{label()}</label>
      {input()}
      <Show when={props.spec.hint && !props.compact}><span class="cat-hint">{tr(props.spec.hint)}</span></Show>
    </div>
  );
}

/** A list field's rows (roster, task skeleton, stages, models): add, remove, reorder. */
function Rows(props: { editor: CatalogEditorStore; spec: CatalogFieldSpec; value: Item[]; onChange: (value: unknown) => Promise<boolean> | void }) {
  const fields = () => props.spec.fields ?? [];
  const blank = (): Item => Object.fromEntries(fields().map((field) => [field.name, field.type === "number" ? field.min ?? 0 : field.type === "bool" ? false : field.type === "words" ? [] : field.type === "choice" ? field.options?.[0]?.[0] ?? "" : ""]));
  const replace = (index: number, row: Item | undefined) => {
    const rows = [...props.value];
    if (row) rows[index] = row; else rows.splice(index, 1);
    return props.onChange(rows);
  };
  const move = (index: number, by: number) => {
    const rows = [...props.value], target = index + by;
    if (target < 0 || target >= rows.length) return;
    [rows[index], rows[target]] = [rows[target], rows[index]];
    void props.onChange(rows);
  };
  return (
    <div class="cat-rows">
      <Index each={props.value}>{(row, index) => (
        <div class="cat-rowedit">
          <span class="cat-rownum">{index + 1}</span>
          <For each={fields()}>{(field) => (
            <Field editor={props.editor} spec={field} value={row()[field.name]} compact onChange={(value) => replace(index, { ...row(), [field.name]: value })} />
          )}</For>
          <button type="button" class="icon-btn small" aria-label={tr("Move up")} disabled={index === 0} onClick={() => move(index, -1)}><Icon name="arrow-up" size={12} /></button>
          <button type="button" class="icon-btn small" aria-label={tr("Move down")} disabled={index === props.value.length - 1} onClick={() => move(index, 1)}><Icon name="arrow-down" size={12} /></button>
          <button type="button" class="icon-btn small" aria-label={tr("Remove")} onClick={() => void replace(index, undefined)}><Icon name="x" size={12} /></button>
        </div>
      )}</Index>
      <button type="button" class="btn ghost small" onClick={() => void props.onChange([...props.value, blank()])}>{tr("+ Add")}</button>
    </div>
  );
}

/** Routing: the rules in order, each its own row, and Try it (`hgs team catalog route`). */
function Routing(props: { editor: CatalogEditorStore; set: (kind: CatalogKind, id: string, field: string, value: unknown) => Promise<boolean>; add: () => Promise<void> }) {
  const rules = () => (props.editor.view()?.routing ?? []) as Item[];
  const specs = () => props.editor.schema()?.routing ?? [];
  const [goal, setGoal] = createSignal("");
  const [answer, setAnswer] = createSignal<string>("");
  const tryIt = async () => {
    try {
      const routed = await props.editor.route(goal());
      const template = routed.template as Item | null | undefined;
      const added = Array.isArray(routed.added) ? (routed.added as unknown[]).map(str) : [];
      let text = template ? tr("Uses %1", str(template.name) || str(template.id)) : tr("No template matches; the lead plans.");
      if (routed.rule) text += " " + tr("(rule %1)", str(routed.rule));
      if (added.length) text += " " + tr("Adds %1", added.join(", "));
      setAnswer(text);
    } catch (error) {
      setAnswer(error instanceof Error ? error.message : String(error));
    }
  };
  return (
    <div class="cat-routing">
      <p class="cat-hint">{tr("Rules run top to bottom; the first that matches the goal picks the template.")}</p>
      <For each={rules()}>{(rule, index) => (
        <div class="cat-rule">
          <For each={specs()}>{(spec) => (
            <Field editor={props.editor} spec={spec} value={rule[spec.name]} compact onChange={(value) => props.set("rule", str(rule.id), spec.name, value)} />
          )}</For>
          <button type="button" class="icon-btn small" aria-label={tr("Move up")} disabled={index() === 0} onClick={() => void props.editor.edit({ op: "move", kind: "rule", id: str(rule.id), by: -1 })}><Icon name="arrow-up" size={12} /></button>
          <button type="button" class="icon-btn small" aria-label={tr("Move down")} disabled={index() === rules().length - 1} onClick={() => void props.editor.edit({ op: "move", kind: "rule", id: str(rule.id), by: 1 })}><Icon name="arrow-down" size={12} /></button>
          <button type="button" class="icon-btn small" aria-label={tr("Delete rule")} onClick={() => void props.editor.remove("rule", str(rule.id))}><Icon name="x" size={12} /></button>
        </div>
      )}</For>
      <button type="button" class="btn ghost small cat-add" disabled={props.editor.busy()} onClick={() => void props.add()}>{tr("+ Rule")}</button>
      <div class="cat-try">
        <label for="cat-try-goal">{tr("Try it")}</label>
        <input id="cat-try-goal" value={goal()} placeholder={tr("A goal or issue title…")} onInput={(event) => setGoal(event.currentTarget.value)}
          onKeyDown={(event) => { if (event.key === "Enter") void tryIt(); }} />
        <button type="button" class="btn small" disabled={!goal().trim()} onClick={() => void tryIt()}>{tr("Try")}</button>
        <Show when={answer()}><span class="cat-try-answer" role="status">{answer()}</span></Show>
      </div>
      <p class="cat-hint">{tr("Try it reads the saved catalog; save your edits first to try them.")}</p>
    </div>
  );
}

/** U28: the lead's planning rules as one text; Reset brings the built-in ones back. */
function PlanningRules(props: { editor: CatalogEditorStore }) {
  const dirty = () => (props.editor.view()?.unsaved ?? 0) > 0;
  return (
    <div class="cat-rules">
      <label for="cat-planning-rules">{tr("Planning rules")}</label>
      <span class="cat-hint">{tr("Every lead plans by these rules; they are sent with its brief.")}</span>
      <textarea id="cat-planning-rules" rows={16} value={props.editor.view()?.planning_rules ?? ""}
        onChange={(event) => void props.editor.edit({ op: "planning_rules", text: event.currentTarget.value })} />
      <div class="cat-rules-foot">
        <span class="grow" />
        <button type="button" class="btn ghost small" disabled={props.editor.busy() || dirty() || !!props.editor.view()?.builtin}
          title={dirty() ? tr("Save or revert your edits first") : tr("Replace the rules with Zerus's built-in ones and save the catalog")}
          onClick={() => void props.editor.resetRules()}>{tr("Reset planning rules")}</button>
      </div>
    </div>
  );
}
