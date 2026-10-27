import { For, Match, Show, Switch, createSignal } from "solid-js";
import { Icon } from "../../lib/icons";
import { inspection, type OverviewRow } from "../../mock/setup-extra";
import { setPage, setSelectedKey, setDetailTab, setDialog } from "../../state";
import { sessions } from "../../mock/data";
import { machineOf } from "../sessions/model";
import { leaf } from "./ui";

// Worktree inspection panel (WorktreeInspectionPanel.h): read-only view of a
// working folder's changed files, original commits, local files outside Git
// and related sessions. Reading never changes the folder.

type Tab = "changes" | "commits" | "local" | "sessions";

export function InspectionPanel(props: { row?: OverviewRow }) {
  const [tab, setTab] = createSignal<Tab>("changes");
  const [details, setDetails] = createSignal(false);
  const [mode, setMode] = createSignal<"uncommitted" | "base">("uncommitted");
  const [loading, setLoading] = createSignal(false);
  const [preview, setPreview] = createSignal<string>();
  const [file, setFile] = createSignal(0);
  const [session, setSession] = createSignal<string>();
  const offline = () => !props.row?.fresh || machineOf(props.row?.machine ?? "")?.connection === "offline";
  const hasChanges = () => !!props.row?.discard?.length;
  const refresh = () => { setLoading(true); setTimeout(() => setLoading(false), 600); };
  const openSession = () => {
    const name = session() ?? props.row?.sessions[0];
    const s = sessions.find((x) => name?.startsWith(x.name));
    if (!s) return;
    setDialog(undefined);
    setSelectedKey(s.key);
    setDetailTab("activity");
    setPage("sessions");
  };
  return (
    <section class="su-insp" aria-label="Worktree details">
      <Show when={props.row} fallback={<div class="empty"><Icon name="folder-git-2" size={22} /><p>Select a worktree</p></div>}>
        {(r) => (
          <>
            <header class="ip-head">
              <h3 class="ell" title={r().path}>{leaf(r().path)}</h3>
              <button class="btn small" classList={{ on: details() }} aria-pressed={details()} onClick={() => setDetails(!details())}>Details</button>
              <button class="icon-btn small" data-tip="Read files and commits" aria-label="Read files and commits" disabled={offline()} onClick={refresh}><Icon name="refresh-cw" size={13} /></button>
            </header>
            <p class="ip-brief parts"><Show when={r().branch}><span class="mono">{r().branch}</span></Show><span>{r().remains}</span><Show when={!r().fresh}><span class="warn">Offline snapshot</span></Show></p>
            <Show when={details()}>
              <pre class="ip-summary mono">{`Folder: ${r().path}\nComputer: ${r().machine}\nBranch: ${r().branch ?? "—"}\nIntegration: ${r().integration || "—"}\nSize: ${r().size}\nSessions: ${r().sessions.join(", ") || "none"}`}</pre>
            </Show>
            <div class="tabs ip-tabs" role="tablist">
              <button class="tab" role="tab" classList={{ on: tab() === "changes" }} aria-selected={tab() === "changes"} onClick={() => setTab("changes")}>Changes</button>
              <button class="tab" role="tab" classList={{ on: tab() === "commits" }} aria-selected={tab() === "commits"} onClick={() => setTab("commits")}>Commits</button>
              <button class="tab" role="tab" classList={{ on: tab() === "local" }} aria-selected={tab() === "local"} onClick={() => setTab("local")} title="Local files outside Git and folder browser">Local</button>
              <button class="tab" role="tab" classList={{ on: tab() === "sessions" }} aria-selected={tab() === "sessions"} onClick={() => setTab("sessions")}>Sessions</button>
            </div>
            <div class="ip-body">
              <Switch>
                <Match when={offline()}>
                  <div class="ip-state">Offline. Changed files are not loaded.</div>
                </Match>
                <Match when={loading()}>
                  <div class="ip-state">Loading…</div>
                </Match>
                <Match when={tab() === "changes"}>
                  <select class="field" aria-label="Changes mode" value={mode()} onChange={(e) => setMode(e.currentTarget.value as never)}>
                    <option value="uncommitted">Uncommitted changes</option>
                    <option value="base">Compare files with base</option>
                  </select>
                  <div class="ip-state">{hasChanges() ? `${inspection.files.length} changed files` : "No changed files."}</div>
                  <Show when={hasChanges()}>
                    <table class="su-table ip-files">
                      <thead><tr><th>Changed file</th><th>State</th></tr></thead>
                      <tbody>
                        <For each={inspection.files}>
                          {(f, i) => (
                            <tr classList={{ on: file() === i() }} onClick={() => setFile(i())} onDblClick={() => { setPreview(f.path); setTab("local"); }}>
                              <td class="mono ell">{f.path}</td><td class="mono">{f.state}</td>
                            </tr>
                          )}
                        </For>
                      </tbody>
                    </table>
                    <pre class="ip-diff mono">{mode() === "uncommitted" ? inspection.diff : inspection.diff.replace("@@ -12,9", "@@ -10,11")}</pre>
                  </Show>
                </Match>
                <Match when={tab() === "commits"}>
                  <pre class="ip-diff mono grow">{r().integration.startsWith("Merged") ? "Original commits absent from the base history (up to 30). Equivalent changes may already be integrated.\n\nNo additional commits." : inspection.commits}</pre>
                </Match>
                <Match when={tab() === "local"}>
                  <div class="notice warn"><Icon name="triangle-alert" size={14} /><span>Outside Git. Removing this folder deletes these files too.</span></div>
                  <div class="row">
                    <button class="btn small" onClick={() => setPreview(undefined)}>Browse all files</button>
                    <button class="btn small" aria-label="Next files">Next</button>
                    <Show when={preview()}><button class="icon-btn small" aria-label="Back to local files" data-tip="Back to local files" onClick={() => setPreview(undefined)}><Icon name="folder" size={13} /></button></Show>
                  </div>
                  <Show when={r().localData?.length || r().category !== 4} fallback={<div class="ip-state">No local files outside Git.</div>}>
                    <div class="ip-state">{preview() ? `Preview: ${preview()}` : `${(r().localData ?? inspection.local).length} entries`}</div>
                    <table class="su-table ip-files">
                      <thead><tr><th>Local file / folder</th><th>Size</th></tr></thead>
                      <tbody>
                        <For each={r().localData ?? inspection.local}>
                          {(f) => <tr onDblClick={() => setPreview(f.path)}><td class="mono ell">{f.path}</td><td>{f.size}</td></tr>}
                        </For>
                      </tbody>
                    </table>
                    <Show when={preview()}>
                      <pre class="ip-diff mono">{preview()?.endsWith("/") ? `${preview()}\nBinary file; text preview is unavailable.` : inspection.preview}</pre>
                    </Show>
                  </Show>
                </Match>
                <Match when={tab() === "sessions"}>
                  <Show when={r().sessions.length} fallback={<div class="ip-state">No related sessions.</div>}>
                    <ul class="ip-sessions" role="listbox" aria-label="Related sessions">
                      <For each={r().sessions}>
                        {(s) => (
                          <li role="option" aria-selected={session() === s} classList={{ on: session() === s }} onClick={() => setSession(s)} onDblClick={openSession}>
                            <Icon name="message-square" size={13} class="dim" />{s}
                          </li>
                        )}
                      </For>
                    </ul>
                    <button class="btn small" disabled={!session()} onClick={openSession}>Open session</button>
                  </Show>
                </Match>
              </Switch>
            </div>
            <div class="ip-status hint parts">{offline() ? "Last known values." : <><span>Updated 11:42</span><span>{r().machine}</span><span>Local Git refs</span></>}</div>
          </>
        )}
      </Show>
    </section>
  );
}
