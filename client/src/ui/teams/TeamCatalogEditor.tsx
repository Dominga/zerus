// Adapted from WhiteWh's TeamCatalogEditor: header, tabs, item rail and nested
// schema fields. Only the phase-one team-plan catalog fields are offered.
import { For, Index, Show, createMemo, createSignal, onMount, type JSX } from "solid-js";
import { tr, trNoop } from "../../i18n";
import { catalogEditor, type CatalogEditorStore, type CatalogList, type FieldSpec, type Item } from "../../store/catalogEditor";
import { TeamConfirmation } from "./TeamConfirmDialog";
import { TextInput, TextArea } from "../lib/TextField";
import "../styles/team-catalog.css";
const TABS = [{ id: "roles", label: trNoop("Roles") }, { id: "pipelines", label: trNoop("Pipelines") }, { id: "templates", label: trNoop("Templates") }] as const;
// Labels from the core's closed phase-one schema. Never translate catalog data.
const LABELS: Record<string, string> = Object.fromEntries([
  trNoop("ID"), trNoop("Name"), trNoop("Prompt"), trNoop("Agent"), trNoop("Account"), trNoop("Account choice"), trNoop("Account ID"), trNoop("Model"), trNoop("Effort"), trNoop("Pipeline"), trNoop("Context"), trNoop("Implementer"), trNoop("Stages"), trNoop("Kind"), trNoop("Role"), trNoop("Maximum rounds"), trNoop("Final acceptance"), trNoop("Description"), trNoop("Lead"), trNoop("Lead rules"), trNoop("Roster"), trNoop("Count"), trNoop("Optional"), trNoop("Default pipeline"), trNoop("Settings"), trNoop("Delegations"), trNoop("Accept results"), trNoop("Answer members"), trNoop("Confirm folder trust"), trNoop("Resume exited members"), trNoop("Merge accepted work"), trNoop("Limits"), trNoop("Maximum members"), trNoop("Maximum phase tasks"), trNoop("Maximum open tasks"), trNoop("Maximum open questions"), trNoop("Timers"), trNoop("Report timeout (seconds)"), trNoop("Start timeout (seconds)"), trNoop("Inbox interval (seconds)"), trNoop("Lead reminder (minutes)"), trNoop("Person reminder (minutes)"), trNoop("Lead stall (minutes)"), trNoop("Answer escalation (minutes)"), trNoop("Resumes per hour"), trNoop("Queue"), trNoop("Routing"), trNoop("Plan approval"), trNoop("Phase acceptance"), trNoop("Integration"), trNoop("Allow landing"),
].map((label) => [label, label]));
const CHOICES: Record<string, string> = { freest: trNoop("Freest account"), named: trNoop("Named account"), keep: trNoop("Keep context"), fresh: trNoop("Fresh context"), review: trNoop("Review"), test: trNoop("Test"), land: trNoop("Land"), lead: trNoop("Lead"), person: trNoop("Person"), pipeline: trNoop("Pipeline"), direct: trNoop("Direct"), auto: trNoop("Automatic"), branch: trNoop("Branch"), none: trNoop("None") };
const NOTICES: Record<string, string> = { "The catalog changed. Revert to read it again before saving.": trNoop("The catalog changed. Revert to read it again before saving."), "Catalog saved.": trNoop("Catalog saved."), "The save result is not known.": trNoop("The save result is not known."), "The catalog exceeds the editor size limit.": trNoop("The catalog exceeds the editor size limit.") };
const label = (spec: FieldSpec) => LABELS[spec.label] ? tr(LABELS[spec.label]) : spec.name;
const blank = (specs: FieldSpec[]): Item => Object.fromEntries(specs.map((spec) => [spec.name, spec.name === "account" && spec.type === "group" ? { mode: "freest" } : spec.type === "group" ? blank(spec.fields ?? []) : spec.default ?? ""]));
const str = (value: unknown) => typeof value === "string" ? value : "";

export function TeamCatalogEditor(props: { editor?: CatalogEditorStore }): JSX.Element {
  const editor = props.editor ?? catalogEditor;
  const [tab, setTab] = createSignal<CatalogList>("roles");
  const [selection, setSelection] = createSignal("");
  const [filter, setFilter] = createSignal("");
  const items = createMemo(() => editor.items(tab()).filter((item) => `${item.name} ${item.id}`.toLowerCase().includes(filter().toLowerCase())));
  const chosen = createMemo(() => items().find((item) => item.id === selection()) ?? items()[0]);
  const specs = () => editor.snapshot()?.schema_fields[tab()] ?? [];
  onMount(() => { void editor.load(); });
  const add = () => {
    const rows = editor.items(tab()); let i = 1; while (rows.some((item) => item.id === `new-${i}`)) i++;
    const id = `new-${i}`; setFilter(""); editor.setItem(tab(), id, { ...blank(specs()), id, name: tr("New item") }); setSelection(id);
  };
  const own = () => (editor.working()[tab()] ?? []).some((row) => row.id === chosen()?.id);
  const builtIn = () => (editor.snapshot()?.builtin[tab()] ?? []).some((row) => row.id === chosen()?.id);
  return <section class="cat cat-editor" aria-label={tr("Team catalog")}>
    <header class="cat-head"><h2>{tr("Team catalog")}</h2><Show when={editor.dirty()}><span class="cat-unsaved">{tr("Unsaved changes")}</span></Show><span class="grow" />
      <button class="btn ghost small" disabled={editor.locked() || !editor.snapshot()} onClick={() => void editor.importFile()}>{tr("Import…")}</button>
      <button class="btn ghost small" disabled={editor.busy() || !editor.snapshot()} onClick={() => void editor.exportFile()}>{tr("Export…")}</button>
      <button class="btn ghost small" disabled={editor.locked()} onClick={() => void editor.revert()}>{tr("Revert")}</button>
      <button class="btn primary small" disabled={editor.locked() || !editor.dirty()} onClick={() => void editor.save()}>{tr("Save catalog")}</button>
    </header>
    <Show when={editor.pending()}><div class="cat-confirm" role="alert"><p>{tr("The save result is not known. Check its receipt before making another change.")}</p><button class="btn small" disabled={editor.busy()} onClick={() => void editor.check()}>{tr("Check result")}</button><button class="btn ghost small" disabled={editor.busy()} onClick={editor.discardUncertain}>{tr("Discard draft and read saved catalog")}</button></div></Show>
    <Show when={editor.notice()}><p class="cat-notice" role="status">{NOTICES[editor.notice()] ? tr(NOTICES[editor.notice()]) : editor.notice()}</p></Show>
    <Show when={editor.confirmation()}><TeamConfirmation name="catalog-change" title={tr("Confirm catalog change")} text={tr("Replace these catalog edits? Running teams keep their snapshots.")} action={tr("Confirm")} onCancel={editor.cancel} onConfirm={editor.confirm} /></Show>
    <nav class="cat-tabs" role="tablist" aria-label={tr("Catalog sections")}><For each={TABS}>{(entry) => <button class="cat-tab" role="tab" aria-selected={tab() === entry.id} onClick={() => { setTab(entry.id); setSelection(""); }}>{tr(entry.label)}</button>}</For></nav>
    <Show when={editor.snapshot()} fallback={<p class="cat-empty">{editor.busy() ? tr("Reading the catalog…") : tr("The catalog is not read yet.")}</p>}>
      <div class="cat-body"><aside class="cat-list"><TextInput aria-label={tr("Search catalog")} value={filter()} onInput={(e) => setFilter(e.currentTarget.value)} /><For each={items()}>{(item) => <button class="cat-row" classList={{ on: chosen()?.id === item.id }} onClick={() => setSelection(str(item.id))}><span class="cat-row-name">{str(item.name) || str(item.id)}</span><span class="cat-row-meta">{str(item.id)}</span></button>}</For><button class="btn ghost small cat-add" disabled={editor.locked()} onClick={add}>{tr("+ Add")}</button></aside>
        <fieldset class="cat-form" disabled={editor.locked()}><Show when={chosen()} fallback={<p>{tr("Nothing here yet.")}</p>}>{(item) => <div class="cat-item"><div class="cat-item-head"><Show when={tab() === "templates"}><button class="btn ghost small" disabled={editor.dirty()} onClick={() => void editor.exportTemplate(str(item().id))}>{tr("Export template…")}</button></Show><b>{str(item().name)}</b><span class="cat-id">{str(item().id)}</span><span class="grow" /><Show when={own()}><button class="btn ghost small" onClick={() => editor.remove(tab(), str(item().id))}>{builtIn() ? tr("Remove override") : tr("Delete")}</button></Show></div><For each={specs()}>{(spec) => <Field spec={spec} value={item()[spec.name]} readonly={spec.name === "id"} onChange={(value) => editor.setItem(tab(),str(item().id),{ ...item(), [spec.name]: value })} />}</For></div>}</Show></fieldset>
      </div>
    </Show>
  </section>;
}

function Field(props: { spec: FieldSpec; value: unknown; onChange: (value: unknown) => void; readonly?: boolean }): JSX.Element {
  const spec = props.spec, id = `catalog-${crypto.randomUUID()}`;
  const object = () => (props.value && typeof props.value === "object" ? props.value : {}) as Item;
  const value = () => props.value ?? spec.default;
  const input = () => {
    switch (spec.type) {
      case "group": return <fieldset class="cat-group"><legend>{label(spec)}</legend><For each={spec.fields ?? []}>{(field) => <Show when={spec.name !== "account" || field.name !== "account" || object().mode === "named"}><Field spec={field} value={object()[field.name]} onChange={(next) => {
        const changed = { ...object(), [field.name]: next };
        if (spec.name === "account" && changed.mode === "freest") delete changed.account;
        props.onChange(changed);
      }} /></Show>}</For></fieldset>;
      case "list": return <div class="cat-rows"><Index each={(Array.isArray(value()) ? value() : []) as Item[]}>{(row, at) => <fieldset class="cat-rowedit"><legend>{at + 1}</legend><For each={spec.fields ?? []}>{(field) => <Field spec={field} value={row()[field.name]} onChange={(next) => props.onChange(((value() ?? []) as Item[]).map((r,i) => i === at ? { ...r,[field.name]:next } : r))} />}</For><button class="btn ghost small" disabled={at === 0} onClick={() => { const rows = [...(value() as Item[])]; [rows[at - 1], rows[at]] = [rows[at], rows[at - 1]]; props.onChange(rows); }}>{tr("Move up")}</button><button class="btn ghost small" disabled={at === (value() as Item[]).length - 1} onClick={() => { const rows = [...(value() as Item[])]; [rows[at + 1], rows[at]] = [rows[at], rows[at + 1]]; props.onChange(rows); }}>{tr("Move down")}</button><button class="btn ghost small" onClick={() => props.onChange(((value() ?? []) as Item[]).filter((_,i) => i !== at))}>{tr("Remove")}</button></fieldset>}</Index><button class="btn ghost small" onClick={() => props.onChange([...((value() ?? []) as Item[]),blank(spec.fields ?? [])])}>{tr("+ Add")}</button></div>;
      case "multiline": return <TextArea id={id} maxLength={20000} rows={6} value={str(value())} onChange={(e) => props.onChange(e.currentTarget.value)} />;
      case "number": return <TextInput id={id} type="number" min="0" max="4294967295" value={Number(value() ?? 0)} onChange={(e) => props.onChange(Number(e.currentTarget.value))} />;
      case "bool": return <input id={id} type="checkbox" checked={value() === true} onChange={(e) => props.onChange(e.currentTarget.checked)} />;
      case "choice": return <select id={id} value={str(value())} onChange={(e) => props.onChange(e.currentTarget.value)}><For each={spec.options ?? []}>{(option) => <option value={option}>{CHOICES[option] ? tr(CHOICES[option]) : option}</option>}</For></select>;
      default: return <TextInput id={id} maxLength={20000} readonly={props.readonly} value={str(value())} onChange={(e) => props.onChange(e.currentTarget.value)} />;
    }
  };
  return <div class="cat-field" classList={{ wide: ["list","multiline","group"].includes(spec.type) }}><Show when={spec.type !== "group"}><label for={id}>{label(spec)}</label></Show>{input()}</div>;
}
