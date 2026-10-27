import { For, Show, createMemo, createSignal, onMount } from "solid-js";
import { Icon } from "../../lib/icons";
import { demoState, setDialog } from "../../state";
import { ANALYTICS_COVERAGE, ANALYTICS_OVERVIEW, ANALYTICS_ROWS, ANALYTICS_VOICE, type AnalyticsRow } from "../../mock/settings-extra";
import { dialogCtx, setDialogCtx } from "./store";
import { SectionHead, Group, Check, Select, NumberField, Actions, Status, Lines, Table } from "./ui";

// Settings → Private analytics (AnalyticsSettings.cpp; docs/analytics.md).
// Reads never enable collection; collection and purge are local-only.

const PAGE = 50;
const PROVIDER_LABEL: Record<string, string> = { claude: "Claude", codex: "Codex", kimi: "Kimi", deepseek: "DeepSeek" };

export const [collect, setCollect] = createSignal(true);
export const [analyticsNote, setAnalyticsNote] = createSignal("");

export function AnalyticsSection() {
  const [scope, setScope] = createSignal("all");
  const [from, setFrom] = createSignal("2026-10-14");
  const [through, setThrough] = createSignal("2026-10-27");
  const [applied, setApplied] = createSignal({ scope: "all", from: "2026-10-14", through: "2026-10-27" });
  const [reading, setReading] = createSignal(true);
  const [provider, setProvider] = createSignal("");
  const [page, setPage] = createSignal(0);
  const [selected, setSelected] = createSignal<string | undefined>();
  onMount(() => setTimeout(() => setReading(false), 700));
  const days = () => Math.round((Date.parse(through()) - Date.parse(from())) / 86400000) + 1;
  const rangeOk = () => days() >= 1 && days() <= 90;
  const refresh = () => {
    if (!rangeOk()) return;
    setApplied({ scope: scope(), from: from(), through: through() });
    setReading(true); setPage(0); setSelected(undefined);
    setTimeout(() => setReading(false), 700);
  };
  const rows = createMemo(() => ANALYTICS_ROWS.filter((r) =>
    (applied().scope === "all" || r.computer === "Computer 1") && r.day >= applied().from && r.day <= applied().through && (!provider() || r.provider === provider())));
  const pageRows = () => rows().slice(page() * PAGE, page() * PAGE + PAGE);
  const key = (r: AnalyticsRow) => `${r.day}|${r.computer}|${r.provider}|${r.producer}`;
  const detail = () => rows().find((r) => key(r) === selected());
  const busy = () => reading() || demoState() === "loading";
  const routes = () => (applied().scope === "all" ? 3 : 1);
  const applyLocal = () => {
    if (dialogCtx.retentionDraft < dialogCtx.analyticsRetention) { setDialog("settings-analytics-shorten"); return; }
    setDialogCtx("analyticsRetention", dialogCtx.retentionDraft);
    setAnalyticsNote("Applying local collection settings…");
    setTimeout(() => setAnalyticsNote(collect() ? "Collection is enabled on this computer." : "Collection is disabled on this computer. Existing history is kept."), 600);
  };
  const purge = () => {
    if (demoState() === "error") { setAnalyticsNote("Local analytics epoch is stale or ambiguous. Refresh before purging."); return; }
    setDialog("settings-analytics-purge");
  };
  return (
    <>
      <SectionHead title="Private analytics" intro="Content-free daily counters stay on each computer. Reads do not enable collection. Unknown measures are unavailable, not zero. Dates are inclusive UTC days (up to 90)." />
      <Group title="Selected computers">
        <Actions>
          <Select value={scope()} label="Selected computers" onChange={setScope} options={[{ value: "local", label: "This computer" }, { value: "all", label: "All configured computers" }]} />
          <label class="an-date"><span class="set-label">From</span><input class="field tabular" type="date" aria-label="From UTC day" value={from()} onInput={(e) => setFrom(e.currentTarget.value)} /></label>
          <label class="an-date"><span class="set-label">Through</span><input class="field tabular" type="date" aria-label="Through UTC day" value={through()} onInput={(e) => setThrough(e.currentTarget.value)} /></label>
          <button class="btn primary" disabled={!rangeOk() || busy()} onClick={refresh}><Icon name="refresh-cw" size={13} />Apply and refresh</button>
        </Actions>
        <Show when={!rangeOk()}><Status kind="error">Choose an inclusive UTC range of at most 90 days.</Status></Show>
        <Show when={!busy()} fallback={<Status kind="busy">Reading {routes()} configured route(s)…</Status>}>
          <Lines class="an-overview" text={applied().scope === "all" ? ANALYTICS_OVERVIEW : ANALYTICS_OVERVIEW.replace("3 selected route(s), 3 current", "1 selected route(s), 1 current")} />
          <div class="an-coverage">
            <For each={applied().scope === "all" ? ANALYTICS_COVERAGE : ANALYTICS_COVERAGE.slice(0, 1)}>
              {(text) => <Lines class={text.includes("excluded") ? "an-cov stale" : "an-cov"} text={text} />}
            </For>
          </div>
          <Status>Read finished. Last-known or missing data is excluded from current totals.</Status>
        </Show>
      </Group>

      <Group title="Daily agent work" aside={
        <Select value={provider()} label="Provider" onChange={(v) => { setProvider(v); setPage(0); }}
          options={[{ value: "", label: "All providers" }, ...Object.entries(PROVIDER_LABEL).map(([value, label]) => ({ value, label }))]} />
      }>
        <div class="an-split">
          <Table class="an-table" label="Daily agent work" rows={busy() ? [] : pageRows()} rowKey={key} selected={selected()} onSelect={setSelected}
            empty={<span class="muted">{busy() ? "Reading…" : "No current read available."}</span>}
            columns={[
              { key: "day", label: "UTC day", class: "tabular nowrap", render: (r) => r.day },
              { key: "computer", label: "Computer", class: "nowrap", render: (r) => r.computer },
              { key: "provider", label: "Provider", render: (r) => PROVIDER_LABEL[r.provider] },
              { key: "producer", label: "Producer", render: (r) => r.producer },
              { key: "turns", label: "Completed turns", class: "num", render: (r) => r.turns },
              { key: "work", label: "Work", class: "num", render: (r) => <span classList={{ dim: r.work === "Unavailable" }}>{r.work}</span> },
              { key: "approval", label: "Approval wait", class: "num", render: (r) => r.approval },
              { key: "input", label: "Input wait", class: "num", render: (r) => <span classList={{ dim: r.input === "Unknown" }}>{r.input}</span> },
            ]} />
          <aside class="an-detail">
            <Show when={detail()} fallback={<p class="muted">Select a day to see all reported counts, token usage, typed failures and histogram means.</p>}>
              {(r) => (
                <Lines text={`${r().day} · ${r().computer}\n${PROVIDER_LABEL[r().provider]} · ${r().producer}\n\nCompleted turns: ${r().turns}\nInterrupted turns: ${r().turns % 3}\nWork: ${r().work}\nApproval wait: ${r().approval}\nInput wait: ${r().input}\n\nInput tokens: ${(r().turns * 41.3).toFixed(0)} k\nOutput tokens: ${(r().turns * 3.1).toFixed(0)} k\nCache read: ${(r().turns * 0.42).toFixed(1)} M\nTyped failures: ${r().turns % 5 === 0 ? "rate_limit 1" : "none reported"}\n\nWork histogram mean: ${r().work === "Unavailable" ? "Unavailable (no samples)" : `${2900 + r().turns * 17} ms (${r().turns} samples)`}`} />
              )}
            </Show>
          </aside>
        </div>
        <Actions>
          <button class="btn small" disabled={page() === 0 || busy()} onClick={() => setPage(page() - 1)}><Icon name="chevron-left" size={12} />Previous</button>
          <button class="btn small" disabled={(page() + 1) * PAGE >= rows().length || busy()} onClick={() => setPage(page() + 1)}>Next<Icon name="chevron-right" size={12} /></button>
          <Show when={!busy() && rows().length}>
            <span class="muted tabular">{page() * PAGE + 1}–{Math.min(rows().length, (page() + 1) * PAGE)} of {rows().length} daily buckets (up to 50 per page)</span>
          </Show>
        </Actions>
      </Group>

      <Group title="Voice on processing computers" hint="Voice counts are separate from agent turns. Accepted audio and total queue/processing time have units; percentile latency is unavailable. Speech collection is managed independently on its processing host.">
        <Lines class="muted" text={ANALYTICS_VOICE} />
      </Group>

      <Group title="Collection on this computer">
        <Check label="Collect content-free agent analytics" checked={collect()} onChange={setCollect} />
        <div class="set-row">
          <div class="set-row-text"><label class="set-label">Daily aggregate retention</label></div>
          <div class="set-row-control">
            <NumberField value={dialogCtx.retentionDraft} min={1} max={90} suffix="days" label="Daily aggregate retention" onChange={(v) => setDialogCtx("retentionDraft", v)} />
          </div>
        </div>
        <Actions>
          <button class="btn primary" onClick={applyLocal}>Apply locally</button>
          <button class="btn danger" onClick={purge}><Icon name="trash" size={13} />Purge local history…</button>
        </Actions>
        <Show when={analyticsNote()}><Status kind={analyticsNote().startsWith("Local analytics epoch") ? "error" : analyticsNote().endsWith("…") ? "busy" : "ok"}>{analyticsNote()}</Status></Show>
        <p class="set-group-hint">Disabling collection keeps existing history. Purge removes only this computer's agent analytics, not speech statistics or durable delivery receipts. Remote collection controls are read-only here.</p>
      </Group>
      <Actions><button class="btn" onClick={() => setDialog("settings-analytics-review")}><Icon name="file-text" size={14} />Review aggregate diagnostics…</button></Actions>
    </>
  );
}
