import { Show, createSignal, onCleanup, onMount } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { inTauri } from "../../../bridge";
import { flag } from "../../../store/settings";
import { Icon } from "../../lib/icons";
import { Actions, Group, SectionHead, Status, Table, ToggleRow } from "./ui";
import { PLACEHOLDER, createJournal, type JournalState, type Page } from "./approvals-journal";

// Settings > Automatic approvals (tray/src/AutomaticApprovalSettings.cpp) as
// the approved mockup lays it out. This client never approves a tool
// request: the switch stays off, and a notice says when the Qt client had it
// on at the switch. The journal is read only, 50 entries a page.

/** QLocale().toString(QDateTime, QLocale::ShortFormat): the person's local time. */
export const entryTime = (at: number) => new Date(at).toLocaleString(undefined, { dateStyle: "short", timeStyle: "short" });

export function ApprovalsSection() {
  const [host, setHost] = createSignal("");
  const [session, setSession] = createSignal("");
  const [state, setState] = createSignal<JournalState>({ page: { entries: [], next: 0, previous: 0 }, loading: false, status: "", detailHint: PLACEHOLDER });
  const journal = createJournal({
    list: (request) => invoke<Page>("approvals_list", { ...request }),
    detail: (id) => invoke<string>("approvals_detail", { id }),
  }, () => ({ host: host(), session: session() }), setState);
  // Outside the app there is no journal to read.
  const firstPage = () => { if (inTauri()) journal.firstPage(); };
  let debounce: ReturnType<typeof setTimeout> | undefined;
  const edited = () => { clearTimeout(debounce); debounce = setTimeout(firstPage, 300); };
  onMount(firstPage);
  onCleanup(() => clearTimeout(debounce));
  const page = () => state().page;
  const loading = () => state().loading;

  return (
    <>
      <SectionHead title="Automatic approvals" />
      <Group>
        <ToggleRow label="Automatically approve tool requests (dangerous)" checked={false} disabled onChange={() => {}}
          hint="This client never approves tool requests automatically. Answer each request in its session." />
        <Show when={flag("automaticApprovals/qtEnabledAtSwitch")}>
          <Status kind="warn">Automatic approvals are turned on in the Qt client. They work only while the Qt client is running; this client does not approve tool requests.</Status>
        </Show>
      </Group>

      <Group title="Approval journal">
        <Actions>
          <input class="field grow" placeholder="Exact computer name (optional)" aria-label="Exact computer name (optional)"
            value={host()} onInput={(e) => { setHost(e.currentTarget.value); edited(); }} />
          <input class="field grow" placeholder="Exact session name (optional)" aria-label="Exact session name (optional)"
            value={session()} onInput={(e) => { setSession(e.currentTarget.value); edited(); }} />
          <button type="button" class="btn" onClick={firstPage}><Icon name="refresh-cw" size={13} />Refresh</button>
        </Actions>
        <div class="an-split">
          <Table class="an-table" label="Approval journal" rows={loading() ? [] : page().entries} rowKey={(e) => String(e.id)}
            selected={state().selected} onSelect={journal.select}
            empty={<span class="muted">{loading() ? "Loading approval journal…" : "No automatic approvals recorded for this filter."}</span>}
            columns={[
              { key: "time", label: "Time", class: "tabular", render: (e) => <span title={entryTime(e.at)}>{entryTime(e.at)}</span> },
              { key: "computer", label: "Computer", render: (e) => <span title={e.host}>{e.host}</span> },
              { key: "session", label: "Session", render: (e) => <span title={e.session}>{e.session}</span> },
              { key: "question", label: "Question", class: "ell-cell", render: (e) => <span title={e.prompt}>{e.prompt}</span> },
              { key: "result", label: "Result", render: (e) => <span title={e.result}>{e.result}</span> },
            ]} />
          <aside class="an-detail appr-detail">
            <Show when={state().detail} fallback={<p class="muted">{state().detailHint}</p>}>{(text) => <div class="appr-text">{text()}</div>}</Show>
          </aside>
        </div>
        <Actions>
          <button type="button" class="btn small" disabled={loading() || page().previous <= 0} onClick={journal.newer}>Newer</button>
          <button type="button" class="btn small" disabled={loading() || page().next <= 0} onClick={journal.older}>Older</button>
          <span class="muted">Up to 50 entries per page</span>
        </Actions>
        <Show when={state().status}>{(text) => <Status kind={loading() ? "busy" : "muted"}>{text()}</Status>}</Show>
      </Group>
    </>
  );
}
