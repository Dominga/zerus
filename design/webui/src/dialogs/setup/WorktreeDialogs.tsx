import { For, Show, createMemo, createSignal, onCleanup, onMount, untrack } from "solid-js";
import { createStore } from "solid-js/store";
import { Icon } from "../../lib/icons";
import { machines } from "../../mock/data";
import { cleanupRows, type CleanupRow, type Outcome, type OverviewRow } from "../../mock/setup-extra";
import { demoState, toast } from "../../state";
import { MachineBadge } from "../../screens/sessions/SessionCard";
import { InspectionPanel } from "../../screens/setup/InspectionPanel";
import {
  org, overview, setOverview, batchTargets, setBatchTargets, batchExit, setBatchExit, cleanupFocus, inspectTarget, setInspectTarget, projectById,
} from "../../screens/setup/store";
import { MockSwitch, SDialog, closeSetup, leaf, openSetup, Progress } from "../../screens/setup/ui";

// Worktrees overview, batch removal, single-repository cleanup and the
// read-only inspection dialog (WorktreesDialog.cpp, WorktreeBatchCleanupDialog.h,
// WorktreeCleanupDialog.h). Cleanup keeps branches unless asked, keeps every
// refusal for review, and never replays an unknown result.

const CATEGORY = ["Cleanup candidates", "Review work", "In use", "Not checked", "Main folders"];

const rowSuffix = (r: OverviewRow) => (!r.fresh ? "Offline snapshot" : r.category === 0 ? "Will recheck before cleanup" : "");

function Overview() {
  const [query, setQuery] = createSignal("");
  const [project, setProject] = createSignal("");
  const [computer, setComputer] = createSignal("");
  const [category, setCategory] = createSignal(-1);
  const [showMain, setShowMain] = createSignal(false);
  const [sort, setSort] = createSignal<"size" | "activity" | "name">("size");
  const [loading, setLoading] = createSignal(false);
  const [anchor, setAnchor] = createSignal<string>();
  let filters!: HTMLDivElement;
  let filtersBtn!: HTMLButtonElement;
  const empty = () => demoState() === "empty";

  const visible = createMemo(() => {
    const q = query().toLowerCase();
    const rows = overview.rows.filter((r) =>
      (showMain() || r.category !== 4) && (!project() || r.project === project()) && (!computer() || r.machine === computer()) &&
      (category() < 0 || r.category === category()) &&
      (!q || r.path.toLowerCase().includes(q) || (r.branch ?? "").toLowerCase().includes(q) || (projectById(r.project)?.name ?? "").toLowerCase().includes(q) || r.sessions.some((s) => s.toLowerCase().includes(q))));
    const by = sort() === "size" ? (a: OverviewRow, b: OverviewRow) => b.bytes - a.bytes : sort() === "activity" ? (a: OverviewRow, b: OverviewRow) => b.activity - a.activity : (a: OverviewRow, b: OverviewRow) => leaf(a.path).localeCompare(leaf(b.path));
    return CATEGORY.map((name, i) => ({ name, i, rows: rows.filter((r) => r.category === i).sort(by) })).filter((g) => g.rows.length);
  });
  const flat = () => visible().flatMap((g) => g.rows);
  const selected = () => overview.selected;
  const isSel = (id: string) => selected().includes(id);
  const candidates = () => overview.rows.filter((r) => r.category === 0 && r.fresh).length;
  const estimate = () => overview.rows.filter((r) => r.category === 0).reduce((n, r) => n + r.bytes, 0) / 1e9;

  const click = (e: MouseEvent, r: OverviewRow) => {
    setOverview("focus", r.id);
    if (r.category === 4) { setOverview("selected", []); return; }
    if (e.shiftKey && anchor()) {
      const ids = flat().map((x) => x.id);
      const [a, b] = [ids.indexOf(anchor()!), ids.indexOf(r.id)].sort((x, y) => x - y);
      setOverview("selected", ids.slice(a, b + 1).filter((id) => overview.rows.find((x) => x.id === id)!.category !== 4));
    } else if (e.ctrlKey || e.metaKey) setOverview("selected", (s) => (s.includes(r.id) ? s.filter((x) => x !== r.id) : [...s, r.id]));
    else setOverview("selected", [r.id]);
    setAnchor(r.id);
  };
  const toggle = (r: OverviewRow) => setOverview("selected", (s) => (s.includes(r.id) ? s.filter((x) => x !== r.id) : [...s, r.id]));
  const refresh = () => { setLoading(true); setTimeout(() => setLoading(false), 900); };
  const openFilters = () => {
    const rect = filtersBtn.getBoundingClientRect();
    filters.style.left = `${rect.left}px`;
    filters.style.top = `${rect.bottom + 4}px`;
    filters.togglePopover();
  };
  const focusRow = () => overview.rows.find((r) => r.id === overview.focus);

  return (
    <>
      <div class="dlg-body wto-body">
        <div class="row wto-toolbar">
          <input class="field grow" type="search" placeholder="Search worktrees…" aria-label="Search folder, branch, project or session" value={query()} onInput={(e) => setQuery(e.currentTarget.value)} />
          <button class="btn" ref={filtersBtn} aria-haspopup="dialog" classList={{ on: !!(project() || computer() || category() >= 0 || showMain()) }} onClick={openFilters}>
            <Icon name="list-filter" size={14} />Filters
          </button>
          <div class="su-popover wto-filters" popover="auto" ref={filters} role="dialog" aria-label="Filters">
            <div class="form-row"><label for="wf-p">Project</label>
              <select id="wf-p" class="field" value={project()} onChange={(e) => setProject(e.currentTarget.value)}>
                <option value="">All projects</option><For each={org.projects}>{(p) => <option value={p.id}>{p.name}</option>}</For>
              </select></div>
            <div class="form-row"><label for="wf-c">Computer</label>
              <select id="wf-c" class="field" value={computer()} onChange={(e) => setComputer(e.currentTarget.value)}>
                <option value="">All computers</option><For each={machines}>{(m) => <option value={m.alias}>{m.alias}</option>}</For>
              </select></div>
            <div class="form-row"><label for="wf-s">State</label>
              <select id="wf-s" class="field" aria-label="Worktree state" value={category()} onChange={(e) => setCategory(+e.currentTarget.value)}>
                <option value="-1">All states</option><For each={CATEGORY.slice(0, 4)}>{(c, i) => <option value={i()}>{c}</option>}</For>
              </select></div>
            <label class="check"><input type="checkbox" checked={showMain()} onChange={(e) => setShowMain(e.currentTarget.checked)} />Show main checkouts and ordinary folders</label>
          </div>
          <select class="field wto-sort" aria-label="Sort worktrees" value={sort()} onChange={(e) => setSort(e.currentTarget.value as never)}>
            <option value="size">Largest first</option><option value="activity">Oldest activity first</option><option value="name">Folder name</option>
          </select>
          <button class="btn" onClick={refresh} disabled={loading()}><Icon name="refresh-cw" size={13} class={loading() ? "su-spin" : undefined} />Refresh</button>
        </div>
        <div class="hint wto-status" title="Browsing data is cached for 1 hour. Cleanup checks expire after five minutes.">
          {loading() ? <span class="parts"><span>Processed 4 of 10 folders</span><span>3 repositories</span><span>6 pending…</span></span>
            : empty() ? "Add a project folder to see its worktrees."
            : <span class="parts"><span>Processed 10 of 10 folders</span><span>6 repositories</span><span>1 incomplete</span><span>{candidates()} cleanup candidates</span><span>~{estimate().toFixed(1)} GB estimated</span><Show when={demoState() === "error"}><span class="warn">Catalog is incomplete: build-02 is offline.</span></Show><span>Updated 11:42</span></span>}
        </div>
        <div class="wto-split">
          <div class="wto-tree" role="grid" aria-label="Worktrees" aria-multiselectable="true">
            <div class="wto-row wto-headrow" role="row">
              <span role="columnheader" />
              <span role="columnheader">Worktree / branch</span>
              <span role="columnheader">What remains</span>
              <span role="columnheader">Size</span>
              <span role="columnheader">Computer</span>
            </div>
            <Show when={!empty()} fallback={<div class="empty"><Icon name="folder-git-2" size={22} /><p>Add a project folder to see its worktrees.</p></div>}>
              <For each={visible()}>
                {(g) => (
                  <>
                    <div class="wto-group" role="row">{g.name} ({g.rows.length})</div>
                    <For each={g.rows}>
                      {(r) => (
                        <div class="wto-row" role="row" aria-selected={isSel(r.id)} classList={{ on: isSel(r.id), focus: overview.focus === r.id, stale: !r.fresh }}
                          onClick={(e) => click(e, r)}>
                          <span role="gridcell">
                            <input type="checkbox" aria-label={`Select ${leaf(r.path)}`} checked={isSel(r.id)} disabled={r.category === 4}
                              onClick={(e) => { e.stopPropagation(); toggle(r); setOverview("focus", r.id); }} />
                          </span>
                          <span role="gridcell" class="col">
                            <span class="ell"><b>{leaf(r.path)}</b></span>
                            <span class="mono dim ell">{r.branch ?? "—"}</span>
                          </span>
                          <span role="gridcell" class="ell" classList={{ warn: r.category === 1 || r.category === 3, muted: r.category === 4 }} title={rowSuffix(r) ? `${r.remains}\n${rowSuffix(r)}` : r.remains}>
                            <Show when={r.state}><span class="wto-state">{r.state}</span> </Show>{r.remains}<Show when={rowSuffix(r)}><span class="wto-suffix">{rowSuffix(r)}</span></Show>
                          </span>
                          <span role="gridcell" class="tabular">{r.size}</span>
                          <span role="gridcell"><MachineBadge alias={r.machine} /></span>
                        </div>
                      )}
                    </For>
                  </>
                )}
              </For>
            </Show>
          </div>
          <InspectionPanel row={focusRow()} />
        </div>
      </div>
      <div class="dlg-foot">
        <button class="btn" disabled={!focusRow()} onClick={() => toast("Copied", focusRow()!.path)}><Icon name="copy" size={13} />Copy path</button>
        <button class="btn" onClick={() => setOverview("selected", overview.rows.filter((r) => r.category === 0 && r.fresh).map((r) => r.id))}>Select candidates</button>
        <span class="grow" />
        <button class="btn primary" disabled={!selected().length} onClick={() => { setBatchExit(false); setBatchTargets([...selected()]); openSetup("setup-batch-cleanup"); }}>
          {selected().length ? `Review cleanup (${selected().length})…` : "Review cleanup…"}
        </button>
        <button class="btn" onClick={() => closeSetup("setup-worktrees")}>Close</button>
      </div>
    </>
  );
}

// ---------------------------------------------------------------------------

type BatchStatus = "checking" | "ready" | "blocked" | "removing" | "done" | "pending";
interface BatchEntry { id: string; status: BatchStatus; reason: string; outcome?: Outcome; reviewed: number }

const OUTCOME_LABEL: Record<Outcome, string> = {
  removed: "Folder removed", forgotten: "Record forgotten", unknown: "Result unknown", kept: "Folder kept", programs: "Programs not checked", unsaved: "Unsaved work",
};

function preReview(r: OverviewRow): { status: BatchStatus; reason: string } {
  if (!r.fresh || machines.find((m) => m.alias === r.machine)?.connection === "offline") return { status: "blocked", reason: "Reconnect this computer to check the folder." };
  if (r.category === 2) return { status: "blocked", reason: r.detail ?? "Another session uses the folder." };
  if (r.processesUnchecked) return { status: "ready", reason: "Some running programs could not be checked." };
  if (r.discard?.length) return { status: "ready", reason: "Uncommitted changes need review." };
  if (r.localData?.length) return { status: "ready", reason: "Local files need review." };
  if (r.state === "Missing") return { status: "ready", reason: "Missing: only Git's record is removed." };
  return { status: "ready", reason: "Ready" };
}

function BatchCleanup() {
  const ids = untrack(batchTargets).filter((id) => overview.rows.some((r) => r.id === id));
  // One exact snapshot per target; removed rows leave the overview but stay listed here.
  const snaps = new Map(ids.map((id) => [id, { ...overview.rows.find((r) => r.id === id)! }]));
  const rowsById = (id: string) => snaps.get(id)!;
  const [entries, setEntries] = createStore<BatchEntry[]>(ids.map((id) => ({ id, status: "checking", reason: "Checking…", reviewed: 0 })));
  const [current, setCurrent] = createSignal(ids[0]);
  const [details, setDetails] = createSignal(false);
  const [local, setLocal] = createSignal(false);
  const [discard, setDiscard] = createSignal(false);
  const [processes, setProcesses] = createSignal(false);
  const [branches, setBranches] = createSignal(false);
  const [running, setRunning] = createSignal(false);
  const [stopping, setStopping] = createSignal(false);
  const [finished, setFinished] = createSignal(false);
  const exit = () => batchExit();
  const timers: ReturnType<typeof setTimeout>[] = [];
  onCleanup(() => timers.forEach(clearTimeout));

  const review = (which: string[]) => {
    which.forEach((id, i) => {
      setEntries((e) => e.id === id, { status: "checking", reason: "Checking…", outcome: undefined });
      timers.push(setTimeout(() => setEntries((e) => e.id === id, (e) => ({ ...preReview(rowsById(id)), reviewed: e.reviewed + 1 })), 500 + i * 350));
    });
  };
  onMount(() => review(ids));

  const targets = () => entries.filter((e) => e.status === "ready");
  const needs = (pred: (r: OverviewRow) => boolean) => targets().some((e) => pred(rowsById(e.id)));
  const needLocal = () => needs((r) => !!r.localData?.length);
  const needDiscard = () => needs((r) => !!r.discard?.length);
  const needProcesses = () => needs((r) => !!r.processesUnchecked);
  const consented = () => (!needLocal() || local()) && (!needDiscard() || discard()) && (!needProcesses() || processes());
  const reviewing = () => entries.some((e) => e.status === "checking");
  // Folders that were refused, stopped or never reachable are all kept.
  const count = (o: Outcome | "kept-all") => entries.filter((e) => (o === "kept-all" ? (e.outcome && !["removed", "forgotten", "unknown"].includes(e.outcome)) || e.status === "blocked" : e.outcome === o)).length;
  const summary = () => `${count("removed") + count("forgotten")} removed  ${count("kept-all")} kept  ${count("unknown")} unknown`;
  const canRecheck = () => !running() && entries.some((e) => e.status === "blocked" || (e.status === "done" && e.outcome && !["removed", "forgotten", "unknown"].includes(e.outcome)));

  const remove = () => {
    setRunning(true); setFinished(false);
    const queue = targets().map((e) => e.id);
    const step = (i: number) => {
      if (i >= queue.length || stopping()) {
        // Folders not reached stay in place.
        queue.slice(i).forEach((id) => setEntries((e) => e.id === id, { status: "done", outcome: "kept", reason: "Kept: removal stopped before this folder." }));
        setRunning(false); setStopping(false); setFinished(true);
        return;
      }
      const id = queue[i];
      setCurrent(id);
      setEntries((e) => e.id === id, { status: "removing", reason: "Checking again and removing…" });
      timers.push(setTimeout(() => {
        const r = rowsById(id);
        const o = r.outcome;
        // Each folder is rechecked before removal; a definite refusal needs a new review.
        const reason = o === "unknown" ? "The result is unknown. This operation will not be repeated. Inspect the folder before deciding what to do next."
          : o === "unsaved" ? "Uncommitted changes need review." : o === "programs" ? "Some running programs could not be checked."
          : o === "kept" ? r.detail ?? "Folder kept." : OUTCOME_LABEL[o];
        setEntries((e) => e.id === id, { status: "done", outcome: o, reason });
        if (o === "removed" || o === "forgotten") setOverview("rows", (list) => list.filter((x) => x.id !== id));
        step(i + 1);
      }, 1000));
    };
    step(0);
  };

  const cur = () => entries.find((e) => e.id === current());
  const detailText = () => {
    const e = cur(); if (!e) return "";
    const r = rowsById(e.id);
    return [
      `${r.path}`,
      e.reason,
      "After removal: conversations remain. Restoring an archived session may require recreating its working folder.",
      ...(r.discard ?? []).map((f) => `Discard: ${f}`),
      ...(r.localData ?? []).map((f) => `Delete local data: ${f.path} (${f.size})`),
    ].join("\n");
  };
  const removeLabel = () => (targets().length > 1 ? `Remove ${targets().length} folders` : "Remove folders");

  return (
    <>
      <div class="dlg-body bc-body">
        <h3 class="section-title">Review {ids.length} selected worktrees</h3>
        <p class="muted">{exit() ? "Conversations and branches are kept. Removing a working folder permanently deletes its local files."
          : "Conversations are kept. Branches stay unless you choose to delete them. Local files in removed folders are permanently deleted."}</p>
        <table class="su-table bc-table">
          <thead><tr><th>Folder / branch</th><th>Computer</th><th>Size</th><th>Result</th></tr></thead>
          <tbody>
            <For each={entries}>
              {(e) => {
                const snap = rowsById(e.id);
                return (
                  <tr classList={{ on: current() === e.id }} onClick={() => setCurrent(e.id)} title={snap.path}>
                    <td><div class="ell"><b>{leaf(snap.path)}</b></div><div class="mono dim ell">{snap.branch ?? "—"}</div></td>
                    <td><MachineBadge alias={snap.machine} /></td>
                    <td class="tabular">{snap.size}</td>
                    <td title={e.reason}>
                      <div class="bc-result" data-outcome={e.outcome ?? e.status}>
                        <Show when={e.status === "checking" || e.status === "removing"}><Icon name="loader-circle" size={12} class="su-spin" /></Show>
                        <Show when={e.outcome} fallback={<span class="ell">{e.reason}</span>}>
                          <b>{OUTCOME_LABEL[e.outcome!]}</b><Show when={e.outcome !== "removed" && e.outcome !== "forgotten"}><span class="hint ell wtc-reason">{e.reason}</span></Show>
                        </Show>
                      </div>
                    </td>
                  </tr>
                );
              }}
            </For>
          </tbody>
        </table>
        <div class="row">
          <button class="btn small" disabled={!canRecheck()} onClick={() => review(entries.filter((e) => e.status === "blocked" || (e.status === "done" && e.outcome && !["removed", "forgotten", "unknown"].includes(e.outcome))).map((e) => e.id))}>Check again</button>
          <button class="btn small" disabled={!cur()} onClick={() => { setInspectTarget(current()!); openSetup("setup-inspect"); }}>View files and commits…</button>
          <button class="btn small" classList={{ on: details() }} aria-pressed={details()} onClick={() => setDetails(!details())}>Details</button>
          <span class="grow" />
          <MockSwitch label="Opened from" value={exit() ? "exit" : "manual"} onChange={(v) => setBatchExit(v === "exit")}
            options={[{ value: "manual", label: "Worktrees" }, { value: "exit", label: "Terminate" }]} />
        </div>
        <Show when={details()}><pre class="bc-details mono">{detailText()}</pre></Show>
        <div class="bc-consent">
          <Show when={needLocal()}><label class="check"><input type="checkbox" checked={local()} disabled={running()} onChange={(e) => setLocal(e.currentTarget.checked)} />Delete the local files outside Git too</label></Show>
          <Show when={needDiscard()}><label class="check"><input type="checkbox" checked={discard()} disabled={running()} onChange={(e) => setDiscard(e.currentTarget.checked)} />Permanently discard uncommitted and untracked files</label></Show>
          <Show when={needProcesses()}>
            <label class="check" title="Accept this uncertainty for this folder for five minutes. Known programs and sessions using the folder still prevent removal.">
              <input type="checkbox" checked={processes()} disabled={running()} onChange={(e) => setProcesses(e.currentTarget.checked)} />Remove even if some running programs cannot be checked
            </label>
          </Show>
          <Show when={!exit()}><label class="check"><input type="checkbox" checked={branches()} disabled={running()} onChange={(e) => setBranches(e.currentTarget.checked)} />Also delete branches proven merged by Git</label></Show>
        </div>
        <Show when={running()}><Progress label="Removing working folders" /></Show>
      </div>
      <div class="dlg-foot">
        <span class="bc-summary tabular" role="status">
          {stopping() ? "Finishing the current folder; remaining folders will be kept." : finished() || running() ? summary() : reviewing() ? "Checking folders…" : ""}
          <Show when={finished() && count("unknown")}><span class="warn bc-note">An unknown result will not be retried.</span></Show>
        </span>
        <span class="grow" />
        <button class="btn" autofocus disabled={stopping()} onClick={() => { if (running()) setStopping(true); else closeSetup("setup-batch-cleanup"); }}>
          {running() ? "Stop after this folder" : finished() ? "Close" : "Keep folders"}
        </button>
        <Show when={!finished()}>
          <button class="btn solid-danger" disabled={running() || reviewing() || !targets().length || !consented()} onClick={remove}>{removeLabel()}</button>
        </Show>
      </div>
    </>
  );
}

// ---------------------------------------------------------------------------

const [confirmCtx, setConfirmCtx] = createSignal<{ rows: CleanupRow[]; branches: boolean; machine: string; onConfirm: () => void }>();
const [cleanupBusy, setCleanupBusy] = createSignal(false);

function RepoCleanup() {
  const focus = untrack(cleanupFocus);
  const focusLeaf = focus ? leaf(focus) : undefined;
  const machine = "workstation";
  const base = focusLeaf ? cleanupRows.filter((r) => r.path === focusLeaf) : cleanupRows;
  const [reviewing, setReviewing] = createSignal(true);
  const [checked, setChecked] = createStore<Record<string, boolean>>(Object.fromEntries(base.filter((r) => r.verdict === "Ready" || r.verdict === "Missing" || !!focusLeaf).filter((r) => r.verdict !== "Blocked").map((r) => [r.path, true])));
  const [results, setResults] = createStore<Record<string, string>>({});
  const [branches, setBranches] = createSignal(false);
  const [done, setDone] = createSignal("");
  const start = () => { setReviewing(true); const t = setTimeout(() => setReviewing(false), 700); onCleanup(() => clearTimeout(t)); };
  onMount(start);
  const chosen = () => base.filter((r) => checked[r.path] && !results[r.path]);
  const counts = () => {
    const c = (v: string) => base.filter((r) => r.verdict === v || (v === "Review" && r.verdict === "Check running programs")).length;
    const parts = [c("Ready") && `${c("Ready")} ready`, c("Review") && `${c("Review")} to review`, c("Missing") && `${c("Missing")} missing`, c("Blocked") && `${c("Blocked")} blocked`].filter(Boolean);
    return parts.length ? `${parts.join(", ")}.` : focusLeaf ? "Git no longer lists this worktree." : "This repository has no linked worktrees.";
  };
  const askRemove = () => {
    setConfirmCtx({
      rows: chosen(), branches: branches(), machine, onConfirm: () => {
        setCleanupBusy(true);
        const list = chosen();
        list.forEach((r, i) => setTimeout(() => {
          setResults(r.path, r.result ?? "Removed");
          if (i === list.length - 1) {
            setCleanupBusy(false);
            const n = list.filter((x) => x.result !== "Not confirmed").length;
            setDone(n === 1 ? "Removed 1 worktree." : `Removed ${n} worktrees.`);
          }
        }, 700 * (i + 1)));
      },
    });
    openSetup("setup-cleanup-confirm");
  };
  return (
    <>
      <div class="dlg-body">
        <p class="muted su-pre">{focusLeaf ? `Computer: ${machine}\nWorktree: ${focus}` : `Computer: ${machine}\nRepository: /home/example/src/aurora`}</p>
        <p class="hint">Removal uses git worktree remove without force. Ignored files in a removed folder, such as build output, are deleted with it. Blocked worktrees explain what to finish first.</p>
        <table class="su-table wc-table">
          <thead><tr><th /><th>Worktree</th><th>State</th><th>Details</th></tr></thead>
          <tbody>
            <Show when={!reviewing()} fallback={<tr><td colspan="4" class="hint">Reviewing worktrees on {machine}…</td></tr>}>
              <For each={base}>
                {(r) => (
                  <tr classList={{ blocked: r.verdict === "Blocked" }}>
                    <td><input type="checkbox" aria-label={`Select ${r.path}`} checked={!!checked[r.path]} disabled={r.verdict === "Blocked" || !!results[r.path] || cleanupBusy()}
                      onChange={(e) => setChecked(r.path, e.currentTarget.checked)} /></td>
                    <td><div class="ell"><b>{r.path}</b></div><div class="mono dim ell">{r.branch}</div></td>
                    <td><span class="wc-verdict" data-v={r.verdict}>{results[r.path] ?? r.verdict}</span></td>
                    <td class="hint">{r.details}</td>
                  </tr>
                )}
              </For>
            </Show>
          </tbody>
        </table>
        <label class="check" title="Uses git branch -d after the worktree is removed. Branches with commits that are not merged are always kept.">
          <input type="checkbox" checked={branches()} disabled={cleanupBusy()} onChange={(e) => setBranches(e.currentTarget.checked)} />Also delete merged branches
        </label>
        <Show when={cleanupBusy()}><Progress label="Removing worktrees" /></Show>
      </div>
      <div class="dlg-foot">
        <span class="hint grow" role="status">{reviewing() ? `Reviewing worktrees on ${machine}…` : done() || counts()}</span>
        <button class="btn" disabled={reviewing() || cleanupBusy()} onClick={() => { setDone(""); start(); }}>Review again</button>
        <button class="btn" disabled={cleanupBusy()} onClick={() => closeSetup("worktree-cleanup")}>Close</button>
        <button class="btn danger" disabled={reviewing() || cleanupBusy() || !chosen().length} onClick={askRemove}>
          {chosen().length > 1 ? `Remove ${chosen().length} worktrees…` : "Remove selected…"}
        </button>
      </div>
    </>
  );
}

function CleanupConfirm() {
  const ctx = untrack(confirmCtx)!;
  const ignored = ctx.rows.filter((r) => r.ignored);
  const missing = ctx.rows.some((r) => r.verdict === "Missing");
  const processReview = ctx.rows.some((r) => r.processes);
  const needConsent = ignored.length > 0 || processReview;
  const [consent, setConsent] = createSignal(false);
  const merged = ctx.rows.filter((r) => r.merged).map((r) => r.branch);
  const n = ctx.rows.length;
  return (
    <>
      <div class="dlg-body">
        <div class="notice warn"><Icon name="triangle-alert" size={16} /><b>{n === 1 ? `Remove 1 worktree on ${ctx.machine}?` : `Remove ${n} worktrees on ${ctx.machine}?`}</b></div>
        <ul class="cc-list mono"><For each={ctx.rows}>{(r) => <li>{r.path}</li>}</For></ul>
        <ul class="cc-facts">
          <Show when={ignored.length}><li>Ignored files deleted with them: {ignored.map((r) => r.ignored).join(", ")}.</li></Show>
          <Show when={missing}><li>Missing folders: only Git's record is removed.</li></Show>
          <li>{ctx.branches && merged.length ? `Merged branches deleted: ${merged.join(", ")}.` : "All branches are kept."}</li>
          <Show when={processReview}><li>The OS hides some programs’ folder paths. Confirm only after checking that these folders are not in use.</li></Show>
          <li>Each worktree is checked again before removal. This cannot be undone.</li>
        </ul>
        <Show when={needConsent}>
          <label class="check"><input type="checkbox" checked={consent()} onChange={(e) => setConsent(e.currentTarget.checked)} />
            {ignored.length ? "I reviewed the local files outside Git and want to delete them too" : "I checked these folders are not in use; remove them anyway"}
          </label>
        </Show>
      </div>
      <div class="dlg-foot">
        <span class="grow" />
        <button class="btn" autofocus onClick={() => closeSetup("setup-cleanup-confirm")}>Cancel</button>
        <button class="btn solid-danger" disabled={needConsent && !consent()} onClick={() => { ctx.onConfirm(); closeSetup("setup-cleanup-confirm"); }}>
          {n === 1 ? "Remove worktree" : `Remove ${n} worktrees`}
        </button>
      </div>
    </>
  );
}

export function WorktreeDialogs() {
  return (
    <>
      <SDialog id="setup-worktrees" title="Worktrees" width="min(1180px, calc(100vw - 48px))" height="min(760px, calc(100vh - 48px))" icon="folder-git-2" bare>
        <Overview />
      </SDialog>
      <SDialog id="setup-batch-cleanup" title="Remove working folders" width="780px" icon="trash" bare><BatchCleanup /></SDialog>
      <SDialog id="worktree-cleanup" title={cleanupFocus() ? "Remove worktree" : "Clean up worktrees"} width="820px" icon="folder-git-2" busy={cleanupBusy} bare>
        <RepoCleanup />
      </SDialog>
      <SDialog id="setup-cleanup-confirm" title={cleanupFocus() ? "Remove worktree" : "Clean up worktrees"} width="520px" icon="triangle-alert" bare>
        <Show when={confirmCtx()}><CleanupConfirm /></Show>
      </SDialog>
      <SDialog id="setup-inspect" title="Inspect working folder" width="min(900px, calc(100vw - 48px))" height="min(680px, calc(100vh - 48px))" icon="eye">
        <InspectionPanel row={overview.rows.find((r) => r.id === inspectTarget())} />
      </SDialog>
    </>
  );
}
