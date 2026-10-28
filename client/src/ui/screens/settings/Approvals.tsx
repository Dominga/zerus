import { Show, createSignal, onCleanup, onMount } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { inTauri } from "../../../bridge";
import { flag } from "../../../store/settings";
import { Icon } from "../../lib/icons";
import { Actions, Group, Lines, SectionHead, Status, Table, ToggleRow } from "./ui";

// Settings > Automatic approvals (tray/src/AutomaticApprovalSettings.cpp) as
// the approved mockup lays it out. This client never approves a tool
// request: the switch stays off, and a notice says when the Qt client had it
// on at the switch. The journal is read only, 50 entries a page.

interface Entry { id: number; at: number; host: string; session: string; prompt: string; result: string }
interface Page { entries: Entry[]; next: number; previous: number }

/** QLocale().toString(QDateTime, QLocale::ShortFormat): the person's local time. */
export const entryTime = (at: number) => new Date(at).toLocaleString(undefined, { dateStyle: "short", timeStyle: "short" });

const PLACEHOLDER = "Select a journal entry to read the question and its outcome.";

export function ApprovalsSection() {
  const [host, setHost] = createSignal("");
  const [session, setSession] = createSignal("");
  const [page, setPage] = createSignal<Page>({ entries: [], next: 0, previous: 0 });
  const [loading, setLoading] = createSignal(false);
  const [status, setStatus] = createSignal("");
  const [selected, setSelected] = createSignal<string | undefined>();
  const [detail, setDetail] = createSignal<string | undefined>();
  const [detailHint, setDetailHint] = createSignal(PLACEHOLDER);
  let before = 0;
  let after = 0;
  let busy = false;
  let again = false;
  let detailRequest = 0;

  // Filter edits during a read coalesce into one more read; obsolete pages
  // never queue (AutomaticApprovalSettings::refresh).
  const refresh = () => {
    setStatus("Loading approval journal…");
    setLoading(true);
    detailRequest++;
    setSelected(undefined);
    setDetail(undefined);
    setDetailHint(PLACEHOLDER);
    if (!inTauri()) return;
    if (busy) { again = true; return; }
    busy = true;
    invoke<Page>("approvals_list", { host: host(), session: session(), before, after }).then(
      (next) => {
        busy = false;
        if (again) { again = false; refresh(); return; }
        setPage(next);
        setLoading(false);
        setStatus(next.entries.length
          ? `${next.entries.length} entries. Unconfirmed attempts require checking Activity or Terminal; they are not retried.`
          : "No automatic approvals recorded for this filter.");
      },
      (error) => {
        busy = false;
        if (again) { again = false; refresh(); return; }
        setLoading(false);
        setStatus(String(error));
      },
    );
  };
  const firstPage = () => { before = 0; after = 0; refresh(); };
  let debounce: ReturnType<typeof setTimeout> | undefined;
  const edited = () => { clearTimeout(debounce); debounce = setTimeout(firstPage, 300); };
  onMount(firstPage);
  onCleanup(() => clearTimeout(debounce));

  const select = (key: string) => {
    setSelected(key);
    setDetail(undefined);
    setDetailHint("Loading question…");
    const request = ++detailRequest;
    void invoke<string>("approvals_detail", { id: Number(key) }).then(
      (text) => { if (request === detailRequest) setDetail(text); },
      (error) => { if (request === detailRequest) setDetail(String(error)); },
    );
  };

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
            selected={selected()} onSelect={select}
            empty={<span class="muted">{loading() ? "Loading approval journal…" : "No automatic approvals recorded for this filter."}</span>}
            columns={[
              { key: "time", label: "Time", class: "tabular", render: (e) => <span title={entryTime(e.at)}>{entryTime(e.at)}</span> },
              { key: "computer", label: "Computer", render: (e) => <span title={e.host}>{e.host}</span> },
              { key: "session", label: "Session", render: (e) => <span title={e.session}>{e.session}</span> },
              { key: "question", label: "Question", class: "ell-cell", render: (e) => <span title={e.prompt}>{e.prompt}</span> },
              { key: "result", label: "Result", render: (e) => <span title={e.result}>{e.result}</span> },
            ]} />
          <aside class="an-detail">
            <Show when={detail()} fallback={<p class="muted">{detailHint()}</p>}>{(text) => <Lines text={text()} />}</Show>
          </aside>
        </div>
        <Actions>
          <button type="button" class="btn small" disabled={loading() || page().previous <= 0}
            onClick={() => { after = page().previous; before = 0; refresh(); }}>Newer</button>
          <button type="button" class="btn small" disabled={loading() || page().next <= 0}
            onClick={() => { before = page().next; after = 0; refresh(); }}>Older</button>
          <span class="muted">Up to 50 entries per page</span>
        </Actions>
        <Show when={status()}>{(text) => <Status kind={loading() ? "busy" : "muted"}>{text()}</Status>}</Show>
      </Group>
    </>
  );
}
