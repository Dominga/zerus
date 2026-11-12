import { Dropdown } from "../lib/Dropdown";
import { TextInput } from "../lib/TextField";
import { For, Match, Show, Suspense, Switch, createEffect, createMemo, createSignal, lazy, onCleanup, onMount } from "solid-js";
import { listen } from "@tauri-apps/api/event";
import { Icon } from "../lib/icons";
import { MachineLabel } from "../components/MachineLabel";
import { ProjectLabel } from "../components/ProjectLabel";
import { inTauri } from "../../bridge";
import { setPage } from "../../store/ui";
import type { Project } from "../../types";
import { createSwarm, createAdoption, syncText, seedMappings, initialAlias } from "./project-swarm";
import "../styles/project-swarm.css";
import { tr } from "../../i18n";
const SwarmConflicts = lazy(() => import("./SwarmConflicts"));

/** setup-swarm Computers / Connect computer; the native worker owns every call. */
export default function ProjectSwarmDialog(props: { projects: Project[]; connect?: boolean; preferred?: string; origin: HTMLElement; close(): void }) {
  const state = createSwarm(), view = state.frame;
  const adoption = createAdoption(), adoptionView = adoption.frame;
  const [adoptionAlias, setAdoptionAlias] = createSignal("");
  const [tab, setTab] = createSignal<"computers" | "connect" | "conflicts" | "adopt">(props.connect ? "connect" : "computers");
  const [reviewError, setReviewError] = createSignal("");
  const [filter, setFilter] = createSignal("");
  const [sort, setSort] = createSignal({ column: 0, ascending: true });
  const [alias, setAlias] = createSignal("");
  const [requested, setRequested] = createSignal<{ alias: string; previous: string | undefined }>();
  const [validPreview, setValidPreview] = createSignal<string>();
  const [mapping, setMapping] = createSignal<Map<string, string>>(new Map());
  const [keep, setKeep] = createSignal(true);
  const busy = () => state.working() || !!view()?.busy;
  const adoptionBusy = () => adoption.working() || !!adoptionView()?.busy;
  // A running adoption never holds the dialog open: closing only hides it, and reopening shows the same original.
  const changing = () => state.working() || !!view()?.changing;
  const preview = () => { const p = view()?.preview; return p?.id === validPreview() && p?.alias === alias() ? p : undefined; };
  const differs = () => !!preview()?.initialized && !preview()?.same_swarm;
  const loading = () => !!requested() && busy() && view()?.mode === "preview";
  const computers = createMemo(() => view()?.catalog?.computers ?? []);
  const computerById = createMemo(() => new Map(computers().map((row) => [row.id, row])));
  const localById = createMemo(() => new Map((view()?.projects ?? []).map((p) => [p.id, p])));
  const rows = createMemo(() => {
    const f = filter().trim().toLowerCase(), s = sort();
    const key = (row: (ReturnType<typeof computers>)[number]) => [row.name, row.access, syncText(row)][s.column];
    return computers().filter((r) => !f || `${r.name} ${r.access} ${syncText(r)}`.toLowerCase().includes(f))
      .toSorted((a, b) => (key(a).localeCompare(key(b)) || a.id.localeCompare(b.id)) * (s.ascending ? 1 : -1));
  });
  let dialog!: HTMLDialogElement, gone = false, aliasInitialized = false, off: (() => void) | undefined, adoptionOff: (() => void) | undefined;
  const openAdoption = () => { setTab("adopt"); void adoption.open(); };
  const close = async () => {
    const [swarmClosed, adoptionClosed] = await Promise.all([state.close(), adoption.close()]);
    if (swarmClosed && adoptionClosed) props.close();
  };
  createEffect(() => {
    const aliases = adoptionView()?.aliases ?? [];
    if (!aliases.includes(adoptionAlias())) setAdoptionAlias(aliases[0] ?? "");
  });
  createEffect(() => {
    const aliases = view()?.aliases ?? [];
    if (!aliasInitialized && aliases.length) {
      aliasInitialized = true; setAlias(initialAlias(aliases, props.preferred));
    } else if (!aliases.includes(alias())) { setAlias(aliases[0] ?? ""); setRequested(undefined); setValidPreview(undefined); }
  });
  createEffect(() => {
    const p = view()?.preview, req = requested();
    if (p && req && p.id !== req.previous && p.alias === req.alias && p.alias === alias() && p.id !== validPreview()) {
      setMapping(seedMappings(view()?.projects ?? [], p.projects)); setKeep(true); setValidPreview(p.id);
    }
  });
  const chooseAlias = (next: string) => { setAlias(next); setRequested(undefined); setValidPreview(undefined); setMapping(new Map()); };
  const runPreview = async () => {
    if (busy() || !alias()) return;
    setValidPreview(undefined); setMapping(new Map());
    setRequested({ alias: alias(), previous: view()?.preview?.id });
    await state.preview(alias());
  };
  const join = () => {
    if (!preview()?.initialized || !view()?.can_join || busy()) return;
    const pairs: [string, string][] = differs() ? (view()?.projects ?? []).flatMap((p) => mapping().get(p.id) ? [[p.id, mapping().get(p.id)!] as [string, string]] : []) : [];
    void state.join(pairs, keep());
  };
  const manage = async () => { if (await state.close()) { props.close(); setPage("machines"); } };
  onMount(() => {
    dialog.showModal();
    void (async () => {
      const stop = inTauri() ? await listen("project-swarm-changed", state.changed) : () => {};
      if (gone) { stop(); return; }
      off = stop;
      adoptionOff = inTauri() ? await listen("project-swarm-changed", adoption.changed) : () => {};
      await state.open();
      if (!gone) dialog.querySelector<HTMLElement>(tab() === "computers" ? "input[type=search]" : '[role="combobox"]')?.focus();
    })();
  });
  onCleanup(() => { gone = true; off?.(); adoptionOff?.(); state.dispose(); adoption.dispose(); dialog.close(); queueMicrotask(() => { if (props.origin.isConnected) props.origin.focus(); }); });
  const header = (column: number, label: string) => <th aria-sort={sort().column === column ? (sort().ascending ? "ascending" : "descending") : "none"}><button class="su-th" onClick={() => setSort((s) => ({ column, ascending: s.column === column ? !s.ascending : true }))}>{label}</button></th>;
  const label = (id: string): Project => props.projects.find((p) => p.id === id) ?? { id, name: localById().get(id)?.name ?? id, color: null, accessible: false, is_default: false, palette: 0, folders: [] };
  return <dialog ref={dialog} class="dlg su-dlg project-swarm" style={{ width: "780px", height: "min(680px, calc(100vh - 48px))", position: "fixed", inset: "0", margin: "auto" }} aria-labelledby="project-swarm-title" onCancel={(e) => { if (e.target !== e.currentTarget) return; e.preventDefault(); void close(); }}>
    <div class="dlg-head"><Icon name="network" size={18} class="accent" /><h2 id="project-swarm-title">{tr("Project swarm")}</h2><span class="grow" /><button class="icon-btn" aria-label={tr("Close")} disabled={changing()} onClick={() => void close()}><Icon name="x" size={16} /></button></div>
    <div class="dlg-body sw-body">
      <p class="muted" title={view()?.catalog ? `Swarm ID: ${view()!.catalog!.swarm_id}` : undefined}>{tr("Projects are shared between connected computers. SSH settings and credentials stay on this computer. Joining through one member brings the whole project catalog.")}</p>
      <div class="tabs sw-tabs" role="tablist" aria-label={tr("Project swarm")}>
        <button id="swarm-computers-tab" class="tab" role="tab" classList={{ on: tab() === "computers" }} aria-selected={tab() === "computers"} aria-controls="swarm-computers" onClick={() => setTab("computers")}>{tr("Computers")}</button>
        <button id="swarm-connect-tab" class="tab" role="tab" classList={{ on: tab() === "connect" }} aria-selected={tab() === "connect"} aria-controls="swarm-connect" onClick={() => setTab("connect")}>{tr("Connect computer")}</button>
        <button id="swarm-adopt-tab" class="tab" role="tab" classList={{ on: tab() === "adopt" }} aria-selected={tab() === "adopt"} aria-controls="swarm-adopt" onClick={openAdoption}>{tr("Adopt catalog")}</button>
        <button id="swarm-conflicts-tab" class="tab" role="tab" classList={{ on: tab() === "conflicts" }} aria-selected={tab() === "conflicts"} aria-controls="swarm-conflicts" onClick={() => setTab("conflicts")}>{tr("Conflicts (%1)", view()?.catalog?.conflicts ?? 0)}</button>
      </div>
      <div class="sw-tabbody">
        <Show when={tab() === "adopt"}>
          <div id="swarm-adopt" class="sw-page" role="tabpanel" aria-labelledby="swarm-adopt-tab">
            <div class="row">
              <Dropdown class="field grow" aria-label={tr("Computer to adopt this catalog")} value={adoptionAlias()} disabled={adoptionBusy()} onChange={setAdoptionAlias} options={(adoptionView()?.aliases ?? []).map((value) => ({ value, label: value }))} />
              <button class="btn" disabled={adoptionBusy() || !adoptionView()?.can_preview || !adoptionAlias()} onClick={() => void adoption.preview(adoptionAlias())}>{tr("Check adoption")}</button>
            </div>
            <Show when={adoptionView()?.preview}>
              <p class="sw-msg">{adoptionView()!.preview!.reason}</p>
              <p class="muted">{tr("Drive folders stay uninitialized without a real reverse peer. Project history and session membership still copy.")}</p>
            </Show>
            <div class="row">
              <button class="btn primary" disabled={adoptionBusy() || !adoptionView()?.can_adopt || adoptionView()?.preview?.alias !== adoptionAlias()} onClick={() => void adoption.adopt()}>{tr("Adopt this catalog")}</button>
              <button class="btn" disabled={adoptionBusy() || !adoptionView()?.can_resume} onClick={() => void adoption.status()}>{tr("Review original status")}</button>
              <button class="btn" disabled={adoptionBusy() || !adoptionView()?.can_resume} onClick={() => void adoption.status()}>{tr("Review original outcome")}</button>
            </div>
          </div>
        </Show>
        <Show when={tab() === "conflicts"}><div id="swarm-conflicts" class="sw-page" role="tabpanel" aria-labelledby="swarm-conflicts-tab"><Suspense><SwarmConflicts view={view} working={state.working} changed={state.changed} resolve={state.resolve} report={setReviewError} /></Suspense></div></Show>
        <Show when={tab() === "computers"}>
          <div id="swarm-computers" class="sw-page" role="tabpanel" aria-labelledby="swarm-computers-tab">
            <div class="row"><TextInput class="field grow" type="search" maxLength={4096} placeholder={tr("Find a computer or connection…")} value={filter()} onInput={(e) => setFilter(e.currentTarget.value)} /><span class="hint tabular">{filter() ? `${rows().length} of ${computers().length} computers` : `${computers().length} computers`}</span></div>
            <div class="su-tablewrap grow"><table class="su-table sw-table"><thead><tr>{header(0, "Computer")}{header(1, tr("Local access"))}{header(2, tr("Sync connection"))}</tr></thead><tbody>
              <For each={rows().map((r) => r.id)}>{(id) => { const row = () => computerById().get(id)!; return <tr><td title={`${row().name}\n${row().id}`}><MachineLabel alias={row().name} class="mbadge" /></td><td title={row().access} classList={{ dim: row().access === "Not configured" }}>{row().access}</td><td title={syncText(row())} classList={{ danger: row().sync.state === "error", warn: row().tasks_outdated }}>{syncText(row())}</td></tr>; }}</For>
            </tbody></table></div>
            <div class="row"><button class="btn" disabled={changing()} onClick={() => void manage()}>{tr("Manage connections…")}</button><span class="grow" /><button class="btn" disabled={busy() || !view()?.can_sync} onClick={() => void state.sync()}><Icon name="refresh-cw" size={13} />{tr("Sync now")}</button></div>
          </div>
        </Show>
        <Show when={tab() === "connect"}>
          <div id="swarm-connect" class="sw-page" role="tabpanel" aria-labelledby="swarm-connect-tab">
            <div class="row"><Dropdown class="field grow" aria-label={tr("Computer to connect")} value={alias()} disabled={busy()} onChange={(value) => chooseAlias(value)} options={[...((view()?.aliases ?? []) ?? []).map((a) => ({ value: a, label: a }))]} /><button class="btn" disabled={busy() || !view()?.can_preview || !alias()} onClick={() => void runPreview()}>{tr("Preview connection")}</button></div>
            <div class="sw-preview grow"><Switch>
              <Match when={!busy() && !view()?.aliases.length}><p class="sw-msg">{tr("Add a computer in Manage connections first.")}</p></Match>
              <Match when={loading()}><div class="sw-msg"><p>{tr("Reading the other project catalog…")}</p><div class="su-progress su-indet" role="progressbar" aria-label={tr("Reading the other project catalog")}><i /></div></div></Match>
              <Match when={preview()?.initialized && preview()?.same_swarm}><p class="sw-msg su-pre">{tr("%1 already shares this project catalog.\nConnect directly to synchronize through this computer.", alias())}</p></Match>
              <Match when={differs()}><p class="muted">{tr("Choose which projects are the same. Projects stay separate unless you match them below.")}</p><div class="su-tablewrap"><table class="su-table sw-map"><thead><tr><th>{tr("Project on this computer")}</th><th>{tr("Project in the other catalog")}</th></tr></thead><tbody><For each={(view()?.projects ?? []).map((p) => p.id)}>{(id) => <tr><td><ProjectLabel project={label(id)} small /></td><td><Dropdown class="field" aria-label={`Match ${localById().get(id)?.name ?? id}`} disabled={busy()} value={mapping().get(id) ?? ""} onChange={(value) => setMapping((m) => new Map(m).set(id, value))} options={[{ value: "", label: tr("Keep as a separate project") }, ...((preview()?.projects ?? []) ?? []).map((p) => ({ value: p.id, label: `${p.name} (${p.id.slice(0, 8)})` }))]} /></td></tr>}</For></tbody></table></div></Match>
              <Match when={view()?.status === `Connected to ${alias()}.`}><p class="sw-msg">{tr("Connected to %1.", alias())}</p></Match>
              <Match when={true}><p class="sw-msg">{tr("Choose a computer and preview its project catalog before connecting.")}</p></Match>
            </Switch></div>
            <label class="check" style={{ visibility: differs() ? "visible" : "hidden" }}><input type="checkbox" checked={keep()} disabled={busy()} onChange={(e) => setKeep(e.currentTarget.checked)} />{tr("Keep swarm placement for sessions already in its catalog")}</label>
            <div class="row"><span class="grow" /><button class="btn primary" disabled={busy() || !preview()?.initialized || !view()?.can_join} onClick={join}>{differs() ? tr("Join and merge projects") : tr("Connect this member")}</button></div>
          </div>
        </Show>
      </div>
    </div>
    <div class="dlg-foot"><span class="hint grow" classList={{ danger: !!reviewError() || !!state.error() || !!view()?.error || !!adoptionView()?.error }} title={tab() === "adopt" ? adoptionView()?.status : reviewError() || state.error() || view()?.status} role="status">{tab() === "adopt" ? adoptionView()?.status : reviewError() || state.error() || view()?.status}</span><button class="btn" disabled={changing()} onClick={() => void close()}>{tr("Close")}</button></div>
  </dialog>;
}
