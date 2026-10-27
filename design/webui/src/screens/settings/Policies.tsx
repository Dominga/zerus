import { Show, createMemo, createSignal, onMount } from "solid-js";
import { Icon } from "../../lib/icons";
import { demoState, setDialog } from "../../state";
import { APPROVALS, CLEANUP_POLICIES, type ApprovalResult } from "../../mock/settings-extra";
import { prefs, setPrefs, cleanupDraft, setCleanupDraft } from "./store";
import { SectionHead, Group, Row, Check, Select, NumberField, Actions, Status, Switch, Table } from "./ui";

// Settings → Worktrees (WorktreeSettings.h): per-computer cleanup rules read
// from and applied to the owning computer. Invalid or partial data fails closed.

const COMPUTERS = [
  { value: "", label: "This computer" },
  { value: "macbook", label: "macbook" },
  { value: "build-01", label: "build-01" },
  { value: "build-02", label: "build-02" },
  { value: "win-lab", label: "win-lab" },
];

export function WorktreesSection() {
  const [busy, setBusy] = createSignal(false);
  const read = (route: string) => {
    setBusy(true);
    setCleanupDraft({ route, policy: undefined, status: "Reading cleanup settings…", error: false });
    setTimeout(() => {
      setBusy(false);
      const p = CLEANUP_POLICIES[route];
      if (p === "unavailable") setCleanupDraft({ status: "This computer is unavailable or needs an updated hgs CLI for cleanup settings.", error: true });
      else if (p === "invalid") setCleanupDraft({ status: "Computer returned invalid cleanup settings.", error: true });
      else setCleanupDraft({ policy: { ...p }, status: "Cleanup settings loaded. Uncertain removals stay for manual review.", error: false });
    }, 600);
  };
  onMount(() => read(cleanupDraft.route));
  const ready = () => !busy() && !!cleanupDraft.policy;
  const p = () => cleanupDraft.policy;
  const apply = () => {
    setBusy(true);
    setCleanupDraft("status", "Reading cleanup settings…");
    setTimeout(() => {
      setBusy(false);
      if (demoState() === "error") setCleanupDraft({ policy: undefined, status: "Save was not confirmed. Refresh before changing settings again.", error: true });
      else setCleanupDraft({ status: "Cleanup settings saved on the selected computer.", error: false });
    }, 700);
  };
  return (
    <>
      <SectionHead title="Worktrees" />
      <Group>
        <Actions>
          <Select value={cleanupDraft.route} label="Computer" disabled={busy()} options={COMPUTERS} onChange={(v) => read(v)} />
          <button class="btn" disabled={busy()} onClick={() => read(cleanupDraft.route)}><Icon name="refresh-cw" size={13} />Refresh</button>
        </Actions>
        <p class="set-group-hint">Cleanup runs on the computer that owns each worktree, even when this window is closed. Rules are enabled by default and apply across its projects. Other computers keep their own settings.</p>
        <Check label="Enable automatic worktree cleanup" checked={p()?.enabled ?? false} disabled={!ready()} onChange={(v) => setCleanupDraft("policy", "enabled", v)} />
        <div class="set-indent">
          <Check label="Clean integrated worktrees after the last session is archived or terminated" checked={p()?.after_session ?? false} disabled={!ready() || !p()?.enabled} onChange={(v) => setCleanupDraft("policy", "after_session", v)} />
          <Check label="Periodically clean unused integrated worktrees" checked={p()?.unused_integrated ?? false} disabled={!ready() || !p()?.enabled} onChange={(v) => setCleanupDraft("policy", "unused_integrated", v)} />
          <Check label="Forget verified managed worktrees whose folders no longer exist" checked={p()?.missing ?? false} disabled={!ready() || !p()?.enabled} onChange={(v) => setCleanupDraft("policy", "missing", v)} />
          <Row label="Minimum inactivity for periodic cleanup" disabled={!ready() || !p()?.enabled || !p()?.unused_integrated}>
            <NumberField value={p()?.grace_hours ?? 24} min={1} max={8760} suffix="hours" label="Minimum inactivity for periodic cleanup"
              disabled={!ready() || !p()?.enabled || !p()?.unused_integrated} onChange={(v) => setCleanupDraft("policy", "grace_hours", v)} />
          </Row>
        </div>
        <p class="set-group-hint">Only verified Zerus-created worktrees qualify. Cleanup keeps branches and conversation history. Changed, untracked and ignored local files, sessions, running programs, locked folders and incomplete checks protect a worktree. Build folders and local configuration require review in Projects &gt; Worktrees.</p>
        <Status kind={busy() ? "busy" : cleanupDraft.error ? "error" : "muted"}>{cleanupDraft.status}</Status>
        <Actions><button class="btn primary" disabled={!ready()} onClick={apply}>Apply to this computer</button></Actions>
      </Group>
    </>
  );
}

// Settings → Automatic approvals (AutomaticApprovalSettings.cpp): a dangerous,
// default-off opt-in with a private, paged journal of every attempt.

const RESULT_KIND: Record<ApprovalResult, string> = { "Approved once": "ok", Failed: "error", Cancelled: "muted", Unconfirmed: "warn" };

export function ApprovalsSection() {
  const [computer, setComputer] = createSignal("");
  const [session, setSession] = createSignal("");
  const [applied, setApplied] = createSignal({ computer: "", session: "" });
  const [selected, setSelected] = createSignal<string | undefined>();
  const [loading, setLoading] = createSignal(true);
  onMount(() => setTimeout(() => setLoading(false), 500));
  const rows = createMemo(() => APPROVALS.filter((e) => (!applied().computer || e.computer === applied().computer) && (!applied().session || e.session === applied().session)));
  const entry = () => APPROVALS.find((e) => e.id === selected());
  const refresh = () => { setLoading(true); setApplied({ computer: computer().trim(), session: session().trim() }); setSelected(undefined); setTimeout(() => setLoading(false), 450); };
  const busy = () => loading() || demoState() === "loading";
  return (
    <>
      <SectionHead title="Automatic approvals" />
      <Group class="appr-toggle">
        <div class="set-row">
          <div class="set-row-text">
            <label class="set-label danger" for="appr-on">Automatically approve tool requests (dangerous)</label>
          </div>
          <div class="set-row-control">
            <Switch id="appr-on" checked={prefs.approvals} label="Automatically approve tool requests (dangerous)"
              onChange={(on) => (on ? setDialog("settings-approvals-enable") : setPrefs("approvals", false))} />
          </div>
        </div>
        <div class="notice danger"><Icon name="triangle-alert" size={14} />
          <span>Allows agents to run commands and change files without waiting for you, including destructive actions and paid usage. Resuming after an expired cache may process the full conversation again. Applies to all reachable sessions while this Zerus is running, including with its window hidden. Ordinary questions, folder trust and permanent permission changes still need you.</span>
        </div>
        <p class="set-group-hint">Off by default and separate from automatic recovery. Supports verified one-time Claude, OpenCode and DeepSeek tool permissions. A journal entry must be saved first. Unconfirmed delivery is never retried automatically. Turning this off stops new approvals; requests already sent may still finish.</p>
      </Group>

      <Group title="Approval journal">
        <Actions>
          <input class="field grow" placeholder="Exact computer name (optional)" aria-label="Exact computer name (optional)" value={computer()} onInput={(e) => setComputer(e.currentTarget.value)} />
          <input class="field grow" placeholder="Exact session name (optional)" aria-label="Exact session name (optional)" value={session()} onInput={(e) => setSession(e.currentTarget.value)} />
          <button class="btn" onClick={refresh}><Icon name="refresh-cw" size={13} />Refresh</button>
        </Actions>
        <div class="appr-split">
          <Show when={!busy()} fallback={<div class="set-table-wrap appr-table"><div class="set-loading"><Status kind="busy">Loading approval journal…</Status></div></div>}>
            <Table class="appr-table" label="Approval journal" rows={rows()} rowKey={(e) => e.id} selected={selected()} onSelect={setSelected}
              empty={<span class="muted">No automatic approvals recorded for this filter.</span>}
              columns={[
                { key: "time", label: "Time", class: "tabular nowrap", render: (e) => e.time },
                { key: "computer", label: "Computer", render: (e) => e.computer },
                { key: "session", label: "Session", render: (e) => e.session },
                { key: "question", label: "Question", class: "ell-cell", render: (e) => e.question },
                { key: "result", label: "Result", class: "nowrap", render: (e) => <span class={`appr-result k-${RESULT_KIND[e.result]}`}>{e.result}</span> },
              ]} />
          </Show>
          <aside class="appr-detail">
            <Show when={entry()} fallback={<p class="muted">Select a journal entry to read the question and its outcome.</p>}>
              {(e) => (
                <>
                  <div class="eyebrow">{e().time} · {e().computer} · {e().session}</div>
                  <p class="appr-q">{e().question}</p>
                  <p>Selected answer: {e().answer}</p>
                  <p class={`appr-result k-${RESULT_KIND[e().result]}`}>{e().result}</p>
                  <p class="muted">{e().detail}</p>
                </>
              )}
            </Show>
          </aside>
        </div>
        <Actions>
          <button class="btn small" disabled>Newer</button>
          <button class="btn small" disabled>Older</button>
          <span class="dim">Up to 50 entries per page</span>
          <span class="grow" />
          <Show when={!busy()}>
            <span class="muted">{rows().length ? `${rows().length} entries. Unconfirmed attempts require checking Activity or Terminal; they are not retried.` : "No automatic approvals recorded for this filter."}</span>
          </Show>
        </Actions>
      </Group>
    </>
  );
}
