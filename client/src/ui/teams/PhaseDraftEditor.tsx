import { For, Show, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { tr } from "../../i18n";
import { TextArea, TextInput } from "../lib/TextField";
import type { TeamMember, TeamOutcome } from "../../store/teams";
import { removeDraftNode, type DraftInput, type DraftNode, type DraftRoute } from "../../store/team-phases";

// The fork's EditPhaseDialog adapted to our draft input. Runtime task fields
// never enter this editor; the whole save retains its original revision.
export interface DraftSnapshot {
  phase: number; revision: number; generation: number | null; next_node: number; input: DraftInput;
}

export function PhaseDraftEditor(props: {
  snapshot: DraftSnapshot; routes: DraftRoute[]; members: TeamMember[];
  close: () => void; submit: (snapshot: DraftSnapshot, input: DraftInput) => Promise<TeamOutcome>;
}) {
  let dialog!: HTMLDialogElement;
  const frozen = props.snapshot;
  const [title, setTitle] = createSignal(frozen.input.title);
  const [barrier, setBarrier] = createSignal(frozen.input.barrier);
  const [nodes, setNodes] = createSignal<DraftNode[]>(structuredClone(frozen.input.nodes));
  const [selected, setSelected] = createSignal<number | null>(nodes()[0]?.id ?? null);
  const [removing, setRemoving] = createSignal<number | null>(null);
  const [pending, setPending] = createSignal(false), [locked, setLocked] = createSignal(false);
  const [notice, setNotice] = createSignal("");
  const current = createMemo(() => nodes().find((n) => n.id === selected()));
  const update = (id: number, patch: Partial<DraftNode>) => setNodes(nodes().map((n) => n.id === id ? { ...n, ...patch } : n));
  const valid = () => title().trim() && nodes().length > 0 && nodes().length <= 200
    && nodes().every((n) => n.title.trim() && n.criteria.length > 0 && n.criteria.every((c) => c.trim()) && n.role && n.pipeline && Number.isInteger(n.estimate_minutes) && n.estimate_minutes >= 0);
  onMount(() => dialog.showModal());
  onCleanup(() => { if (dialog.open) dialog.close(); });
  const add = () => {
    const id = Math.max(frozen.next_node - 1, ...nodes().map((n) => n.id)) + 1;
    const route = props.routes[0];
    setNodes([...nodes(), { id, title: "", text: "", criteria: [""], role: route?.role ?? "", pipeline: route?.pipeline ?? "", deps: [], assignee: null, writes: [], estimate_minutes: 0, kind: "task" }]);
    setSelected(id);
  };
  const remove = () => {
    const id = removing(); if (id === null) return;
    const next = removeDraftNode(nodes(), id, frozen.next_node);
    setNodes(next); setSelected(next[0]?.id ?? null); setRemoving(null);
  };
  const save = async () => {
    if (pending() || locked() || !valid()) return;
    setPending(true); setNotice("");
    try {
      const result = await props.submit(frozen, { title: title(), barrier: barrier(), nodes: structuredClone(nodes()) });
      if (result.outcome === "done") { props.close(); return; }
      if (result.outcome === "uncertain" || result.outcome === "pending") {
        setLocked(true); setNotice(tr("The change is unconfirmed. Close this editor and use Check in the team's unresolved actions."));
      } else if (result.outcome === "failed") {
        setLocked(result.code === "stale_revision");
        setNotice(result.code === "stale_revision" ? tr("The draft changed. Close this editor and read again; your text stays here to copy.") : result.detail);
      }
    } catch {
      setLocked(true); setNotice(tr("The change is unconfirmed. Close this editor and use Check in the team's unresolved actions."));
    } finally { setPending(false); }
  };
  return (
    <dialog ref={dialog} class="dlg phase-editor" aria-labelledby="phase-editor-title" onCancel={(e) => { e.preventDefault(); if (!pending()) props.close(); }}>
      <div class="dlg-head"><h2 id="phase-editor-title">{frozen.generation === null ? tr("New phase draft") : tr("Edit phase draft")}</h2></div>
      <div class="dlg-body">
        <p class="dim">{tr("Drafts create no tasks until the phase is approved.")}</p>
        <label class="phase-field">{tr("Phase title")}<TextInput class="field" value={title()} maxLength={200} disabled={pending()} data-phase-title onInput={(e) => setTitle(e.currentTarget.value)} /></label>
        <label><input type="checkbox" checked={barrier()} disabled={pending()} onChange={(e) => setBarrier(e.currentTarget.checked)} /> {tr("Waits for earlier phases (barrier)")}</label>
        <div class="phase-editor-grid">
          <nav class="phase-editor-list" aria-label={tr("Draft tasks")}>
            <For each={nodes()}>{(n) => <button type="button" class="btn ghost" aria-pressed={selected() === n.id} disabled={pending()} onClick={() => setSelected(n.id)}>{n.title || tr("Draft task #%1", n.id)}</button>}</For>
            <button type="button" class="btn" disabled={pending() || locked() || nodes().length >= 200} data-add-draft-task onClick={add}>{tr("Add draft task")}</button>
          </nav>
          <Show when={current()}>{(n) => <section class="phase-editor-task" data-draft-node={n().id}>
            <label class="phase-field">{tr("Task title")}<TextInput class="field" value={n().title} maxLength={200} disabled={pending()} data-node-title onInput={(e) => update(n().id, { title: e.currentTarget.value })} /></label>
            <label class="phase-field">{tr("Description")}<TextArea class="field" rows={4} value={n().text} disabled={pending()} onInput={(e) => update(n().id, { text: e.currentTarget.value })} /></label>
            <label class="phase-field">{tr("Acceptance criteria, one per line")}<TextArea class="field" rows={3} value={n().criteria.join("\n")} disabled={pending()} data-node-criteria onInput={(e) => update(n().id, { criteria: e.currentTarget.value.split("\n") })} /></label>
            <label class="phase-field">{tr("Pipeline")}<select class="field" value={n().pipeline} disabled={pending()} onChange={(e) => {
              const route = props.routes.find((r) => r.pipeline === e.currentTarget.value);
              if (route) update(n().id, { pipeline: route.pipeline, role: route.role, assignee: null });
            }}><For each={props.routes}>{(r) => <option value={r.pipeline}>{r.pipeline}</option>}</For></select></label>
            <label class="phase-field">{tr("Assigned to")}<select class="field" value={n().assignee ?? ""} disabled={pending()} onChange={(e) => update(n().id, { assignee: e.currentTarget.value || null })}>
              <option value="">{tr("A free member of the role")}</option>
              <For each={props.members.filter((m) => !m.lead && m.state !== "removed" && m.role === n().role)}>{(m) => <option value={m.id}>{m.id}</option>}</For>
            </select></label>
            <fieldset class="phase-dependencies"><legend>{tr("Dependencies")}</legend>
              <For each={nodes().filter((other) => other.id !== n().id).map((other) => other.id)}>{(id) => <div class="phase-dependency">
                <label><input type="checkbox" checked={n().deps.some((d) => d.node === id)} disabled={pending()} onChange={(e) => update(n().id, { deps: [...n().deps.filter((d) => d.node !== id), ...(e.currentTarget.checked ? [{ node: id, kind: "hard" as const }] : [])] })} /> {nodes().find((other) => other.id === id)?.title || tr("Draft task #%1", id)}</label>
                <Show when={n().deps.some((d) => d.node === id)}><select class="field" aria-label={tr("Dependency kind")} value={n().deps.find((d) => d.node === id)?.kind ?? "hard"} disabled={pending()} onChange={(e) => update(n().id, { deps: n().deps.map((d) => d.node === id ? { ...d, kind: e.currentTarget.value as "hard" | "soft" } : d) })}>
                  <option value="hard">{tr("Hard")}</option><option value="soft">{tr("Soft")}</option>
                </select></Show>
              </div>}</For>
              <For each={n().deps.filter((d) => !nodes().some((other) => other.id === d.node))}>{(dep) => <div class="phase-dependency"><span>{tr("Task #%1 in another phase", dep.node)}</span><button type="button" class="btn ghost small" disabled={pending()} onClick={() => update(n().id, { deps: n().deps.filter((d) => d.node !== dep.node) })}>{tr("Remove dependency")}</button></div>}</For>
            </fieldset>
            <label class="phase-field">{tr("Write paths, one per line")}<TextArea class="field" rows={2} value={n().writes.join("\n")} disabled={pending()} onInput={(e) => update(n().id, { writes: e.currentTarget.value ? e.currentTarget.value.split("\n") : [] })} /></label>
            <label class="phase-field">{tr("Estimate in minutes")}<TextInput class="field" type="number" min={0} max={525600} step={1} value={n().estimate_minutes} disabled={pending()} onInput={(e) => update(n().id, { estimate_minutes: Number(e.currentTarget.value) })} /></label>
            <button type="button" class="btn ghost danger" disabled={pending() || locked()} onClick={() => setRemoving(n().id)}>{tr("Remove draft task…")}</button>
          </section>}</Show>
        </div>
        <Show when={removing() !== null}><div class="notice" role="alertdialog" aria-label={tr("Remove draft task?")}>
          <p>{tr("Remove this draft task and its local dependency links? Save applies the change.")}</p>
          <button type="button" class="btn danger" onClick={remove}>{tr("Remove draft task")}</button><button type="button" class="btn" onClick={() => setRemoving(null)}>{tr("Keep task")}</button>
        </div></Show>
        <Show when={notice()}><p class="danger" role="status">{notice()}</p></Show>
      </div>
      <div class="dlg-foot"><button type="button" class="btn" disabled={pending()} onClick={props.close}>{tr("Close")}</button><span class="spacer" /><button type="button" class="btn primary" disabled={pending() || locked() || !valid()} data-save-phase onClick={() => void save()}>{tr("Save draft")}</button></div>
    </dialog>
  );
}
