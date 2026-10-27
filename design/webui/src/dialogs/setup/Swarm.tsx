import { For, Match, Show, Switch, createMemo, createSignal, untrack } from "solid-js";
import { Icon } from "../../lib/icons";
import { machines } from "../../mock/data";
import { swarmComputers, swarmConflicts, swarmPeerCatalog, type Conflict, type ConflictChoice } from "../../mock/setup-extra";
import { setPage } from "../../state";
import { MachineBadge } from "../../screens/sessions/SessionCard";
import { org, swarmTab, setSwarmTab, swarmPeer, setSwarmPeer, resolvedConflicts, setResolvedConflicts } from "../../screens/setup/store";
import { ProjectLabel, SDialog, closeSetup, openSetup, Progress } from "../../screens/setup/ui";

// Project swarm (SwarmDialog.cpp, SwarmConflictReview.cpp). Joining happens
// only after a preview, project mapping defaults to separate projects, and a
// removal choice in conflict review needs its own confirmation.

const SEPARATE = "__separate__";
const [confirmChoice, setConfirmChoice] = createSignal<{ conflict: Conflict; choice: ConflictChoice; apply: () => void }>();

function Computers(props: { say: (t: string) => void }) {
  const [filter, setFilter] = createSignal("");
  const [sort, setSort] = createSignal<{ col: 0 | 1 | 2; asc: boolean }>({ col: 0, asc: true });
  const [synced, setSynced] = createSignal<string>();
  const rows = createMemo(() => {
    const f = filter().toLowerCase();
    const list = swarmComputers
      .map((r) => ({ ...r, sync: r.computer === "macbook" && synced() ? `Synced ${synced()}` : r.sync }))
      .filter((r) => !f || `${r.computer} ${r.access} ${r.sync}`.toLowerCase().includes(f));
    const key = (r: (typeof list)[number]) => [r.computer, r.access, r.sync][sort().col];
    return list.sort((a, b) => key(a).localeCompare(key(b)) * (sort().asc ? 1 : -1));
  });
  const th = (col: 0 | 1 | 2, label: string) => (
    <th aria-sort={sort().col === col ? (sort().asc ? "ascending" : "descending") : "none"}>
      <button class="su-th" onClick={() => setSort((s) => ({ col, asc: s.col === col ? !s.asc : true }))}>{label}</button>
    </th>
  );
  return (
    <div class="sw-page">
      <div class="row">
        <input class="field grow" type="search" placeholder="Find a computer or connection…" value={filter()} onInput={(e) => setFilter(e.currentTarget.value)} />
        <span class="hint tabular">{filter() ? `${rows().length} of ${swarmComputers.length} computers` : `${swarmComputers.length} computers`}</span>
      </div>
      <div class="su-tablewrap grow">
        <table class="su-table sw-table">
          <thead><tr>{th(0, "Computer")}{th(1, "Local access")}{th(2, "Sync connection")}</tr></thead>
          <tbody>
            <For each={rows()}>
              {(r) => (
                <tr>
                  <td><MachineBadge alias={r.computer} /></td>
                  <td classList={{ dim: r.access === "Not configured" }}>{r.access}</td>
                  <td classList={{ danger: r.sync.includes("timed out"), warn: r.sync.includes("Update Zerus") }}>{r.sync}</td>
                </tr>
              )}
            </For>
          </tbody>
        </table>
      </div>
      <div class="row">
        <button class="btn" onClick={() => { closeSetup("setup-swarm"); setPage("machines"); }}>Manage connections…</button>
        <span class="grow" />
        <button class="btn" onClick={() => { props.say("Synchronizing projects…"); setTimeout(() => { setSynced(new Date().toTimeString().slice(0, 8)); props.say("Synchronization finished. Connection details are on the Computers tab."); }, 900); }}>
          <Icon name="refresh-cw" size={13} />Sync now
        </button>
      </div>
    </div>
  );
}

function Connect(props: { say: (t: string) => void }) {
  const peers = machines.filter((m) => !m.thisComputer).map((m) => m.alias);
  const [preview, setPreview] = createSignal<"none" | "loading" | "same" | "map" | "connected">("none");
  const [map, setMap] = createSignal<Record<string, string>>({});
  const [keep, setKeep] = createSignal(true);
  const [busy, setBusy] = createSignal(false);
  const peer = () => swarmPeer();
  const catalog = () => swarmPeerCatalog[peer()];
  const differs = () => preview() === "map";
  const run = () => {
    setPreview("loading");
    props.say("Reading the other project catalog…");
    setTimeout(() => {
      const c = catalog();
      setMap({});
      setPreview(c?.same ? "same" : "map");
      props.say(c?.same ? "" : `Review the project mapping, then connect to ${peer()}.`);
    }, 800);
  };
  const join = () => {
    setBusy(true);
    props.say(differs() ? "Connecting and merging catalogs…" : "Synchronizing projects…");
    setTimeout(() => { setBusy(false); setPreview("connected"); props.say(`Connected to ${peer()}.`); }, 1100);
  };
  return (
    <div class="sw-page">
      <div class="row">
        <select class="field grow" aria-label="Computer to connect" value={peer()} disabled={busy()} onChange={(e) => { setSwarmPeer(e.currentTarget.value); setPreview("none"); props.say(""); }}>
          <For each={peers}>{(p) => <option value={p}>{p}</option>}</For>
        </select>
        <button class="btn" disabled={!peers.length || busy() || preview() === "loading"} onClick={run}>Preview connection</button>
      </div>
      <div class="sw-preview grow">
        <Switch>
          <Match when={!peers.length}><p class="sw-msg">Add a computer in Manage connections first.</p></Match>
          <Match when={preview() === "none"}><p class="sw-msg">Choose a computer and preview its project catalog before connecting.</p></Match>
          <Match when={preview() === "loading"}><div class="sw-msg"><p>Reading the other project catalog…</p><Progress label="Reading the other project catalog" /></div></Match>
          <Match when={preview() === "same"}><p class="sw-msg su-pre">{`${peer()} already shares this project catalog.\nConnect directly to synchronize through this computer.`}</p></Match>
          <Match when={preview() === "connected"}><p class="sw-msg">Connected to {peer()}.</p></Match>
          <Match when={preview() === "map"}>
            <p class="muted">Choose which projects are the same. Projects stay separate unless you match them below.</p>
            <div class="su-tablewrap">
              <table class="su-table sw-map">
                <thead><tr><th>Project on this computer</th><th>Project in the other catalog</th></tr></thead>
                <tbody>
                  <For each={org.projects}>
                    {(p) => (
                      <tr>
                        <td><ProjectLabel project={p} small /></td>
                        <td>
                          <select class="field" aria-label={`Match ${p.name}`} disabled={busy()} value={map()[p.id] ?? SEPARATE}
                            onChange={(e) => setMap((m) => ({ ...m, [p.id]: e.currentTarget.value }))}>
                            <option value={SEPARATE}>Keep as a separate project</option>
                            <For each={catalog()?.projects ?? []}>{(o) => <option value={o}>{o}</option>}</For>
                          </select>
                        </td>
                      </tr>
                    )}
                  </For>
                </tbody>
              </table>
            </div>
          </Match>
        </Switch>
      </div>
      <label class="check" style={{ visibility: preview() === "map" ? "visible" : "hidden" }}>
        <input type="checkbox" checked={keep()} disabled={busy()} onChange={(e) => setKeep(e.currentTarget.checked)} />Keep swarm placement for sessions already in its catalog
      </label>
      <div class="row">
        <span class="grow" />
        <button class="btn primary" disabled={busy() || (preview() !== "map" && preview() !== "same")} onClick={join}>{differs() ? "Join and merge projects" : "Connect this member"}</button>
      </div>
    </div>
  );
}

function Conflicts(props: { say: (t: string) => void }) {
  const open = () => swarmConflicts.filter((c) => !resolvedConflicts[c.id]);
  const [current, setCurrent] = createSignal<string | undefined>(untrack(open)[0]?.id);
  const [choice, setChoice] = createSignal<number>();
  const [saving, setSaving] = createSignal(false);
  const conflict = () => open().find((c) => c.id === current()) ?? open()[0];
  const pick = (id: string) => { setCurrent(id); setChoice(undefined); };
  const apply = (c: Conflict) => {
    setSaving(true);
    props.say("Saving your choice to the shared catalog…");
    setTimeout(() => {
      setResolvedConflicts(c.id, true);
      setSaving(false);
      setChoice(undefined);
      setCurrent(open()[0]?.id);
      props.say(open().length ? "Choice saved. Review the remaining conflicts." : "Choice saved. No conflicts remain.");
    }, 700);
  };
  const resolve = () => {
    const c = conflict(); const i = choice();
    if (!c || i === undefined) return;
    const ch = c.choices[i];
    if (ch.removal) { setConfirmChoice({ conflict: c, choice: ch, apply: () => apply(c) }); openSetup("setup-swarm-confirm"); }
    else apply(c);
  };
  return (
    <Show when={open().length} fallback={<p class="sw-msg su-pre">{"No conflicting changes.\nChanges from connected computers are merged automatically."}</p>}>
      <div class="sw-page">
        <p class="muted">These changes need your decision. Review one item at a time; each choice is shared with the swarm.</p>
        <div class="sw-conflicts grow">
          <ul class="sw-clist" role="listbox" aria-label="Conflicts">
            <For each={open()}>
              {(c) => (
                <li role="option" aria-selected={conflict()?.id === c.id} classList={{ on: conflict()?.id === c.id }} onClick={() => pick(c.id)}>
                  <b>{c.kind}</b><span class="muted ell">{c.subject}</span>
                </li>
              )}
            </For>
          </ul>
          <Show when={conflict()}>
            {(c) => (
              <section class="sw-review">
                <h3 class="section-title">{c().kind}</h3>
                <div class="row"><span class="grow">{c().subject}</span><Show when={c().machine}><MachineBadge alias={c().machine!} /></Show></div>
                <p class="muted su-pre">{c().explanation}</p>
                <table class="su-table sw-choices">
                  <thead><tr><th>Value to keep</th><th>Changed on</th><th>Display</th></tr></thead>
                  <tbody>
                    <For each={c().choices}>
                      {(ch, i) => (
                        <tr classList={{ on: choice() === i() }} tabindex="0" aria-selected={choice() === i()} onClick={() => setChoice(i())}
                          onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); setChoice(i()); } }}>
                          <td class="su-pre" classList={{ danger: ch.removal }}>{ch.value}</td>
                          <td><MachineBadge alias={ch.machine} /><Show when={ch.local}><span class="hint"> this computer</span></Show></td>
                          <td class="hint">{ch.shown ? "Shown now" : ""}</td>
                        </tr>
                      )}
                    </For>
                  </tbody>
                </table>
                <p class="hint su-pre sw-outcome">{choice() === undefined ? "Select a row to see what will change. Closing this window leaves the conflict for later." : c().outcome(c().choices[choice()!])}</p>
                <div class="row"><span class="grow" />
                  <button class="btn primary" disabled={choice() === undefined || saving()} onClick={resolve}>{choice() === undefined ? "Choose a value above" : c().action}</button>
                </div>
              </section>
            )}
          </Show>
        </div>
      </div>
    </Show>
  );
}

function SwarmBody() {
  const [status, setStatus] = createSignal("");
  const conflicts = () => swarmConflicts.filter((c) => !resolvedConflicts[c.id]).length;
  return (
    <>
      <div class="dlg-body sw-body">
        <p class="muted" title="Swarm ID: 7d3c1f0e-5a2b-4c8e-9f61-0b4e2d9a6c15">Projects are shared between connected computers. SSH settings and credentials stay on this computer. Joining through one member brings the whole project catalog.</p>
        <div class="tabs sw-tabs" role="tablist">
          <button class="tab" role="tab" classList={{ on: swarmTab() === "computers" }} aria-selected={swarmTab() === "computers"} onClick={() => setSwarmTab("computers")}>Computers</button>
          <button class="tab" role="tab" classList={{ on: swarmTab() === "connect" }} aria-selected={swarmTab() === "connect"} onClick={() => setSwarmTab("connect")}>Connect computer</button>
          <button class="tab" role="tab" classList={{ on: swarmTab() === "conflicts" }} aria-selected={swarmTab() === "conflicts"} onClick={() => setSwarmTab("conflicts")}>
            Conflicts ({conflicts()})
          </button>
        </div>
        <div class="sw-tabbody">
          <Switch>
            <Match when={swarmTab() === "computers"}><Computers say={setStatus} /></Match>
            <Match when={swarmTab() === "connect"}><Connect say={setStatus} /></Match>
            <Match when={swarmTab() === "conflicts"}><Conflicts say={setStatus} /></Match>
          </Switch>
        </div>
      </div>
      <div class="dlg-foot">
        <span class="hint grow" role="status">{status()}</span>
        <button class="btn" onClick={() => closeSetup("setup-swarm")}>Close</button>
      </div>
    </>
  );
}

function ConfirmChange() {
  const ctx = untrack(confirmChoice)!;
  return (
    <>
      <div class="dlg-body">
        <p class="confirm-body su-pre">{`${ctx.conflict.subject}\n\n${ctx.conflict.outcome(ctx.choice)}`}</p>
      </div>
      <div class="dlg-foot">
        <span class="grow" />
        <button class="btn" autofocus onClick={() => closeSetup("setup-swarm-confirm")}>Cancel</button>
        <button class="btn solid-danger" onClick={() => { ctx.apply(); closeSetup("setup-swarm-confirm"); }}>Apply</button>
      </div>
    </>
  );
}

export function SwarmDialogs() {
  return (
    <>
      <SDialog id="setup-swarm" title="Project swarm" width="780px" height="min(680px, calc(100vh - 48px))" icon="network" bare><SwarmBody /></SDialog>
      <SDialog id="setup-swarm-confirm" title="Confirm catalog change" width="480px" icon="triangle-alert" bare>
        <Show when={confirmChoice()}><ConfirmChange /></Show>
      </SDialog>
    </>
  );
}
