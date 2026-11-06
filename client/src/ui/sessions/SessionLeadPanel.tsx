import { For, Show, createMemo } from "solid-js";
import type { JSX } from "solid-js";
import { tr } from "../../i18n";
import { toast } from "../../store/notices";
import { leadLinks, leadOf, setLead, subordinatesOf } from "../../store/sessionLeads";
import { orderedRows } from "../../store/catalog";
import { selected, setDetailTab, setSelectedKey } from "../../store/ui";
import type { Session } from "./types";

/** Qt SessionLeadPanel: the selected session's lead or subordinates, in the
 * Details inspector. The links are one level; a lead that reports or a
 * session that already leads is refused by the store. */
export function SessionLeadPanel(): JSX.Element {
  const current = () => selected();
  const sameProject = createMemo(() => {
    const row = current();
    if (!row) return [];
    return orderedRows().filter((other) => other.key !== row.key && other.project === row.project && other.state !== "archived");
  });
  const lead = () => sameProject().find((row) => row.key === leadOf(current()?.key ?? ""));
  const children = () => {
    const ids = new Set(subordinatesOf(current()?.key ?? ""));
    return orderedRows().filter((row) => ids.has(row.key));
  };
  const candidates = () => sameProject().filter((row) => !leadOf(row.key) && subordinatesOf(row.key).length === 0);
  const open = (key: string) => {
    setSelectedKey(key);
    setDetailTab("activity");
  };
  const choose = async (key: string) => {
    if (!current()) return;
    const ok = await setLead(current()!.key, key);
    if (!ok) toast(tr("Session lead"), tr("A lead cannot report to another session, and a subordinate cannot lead."), "error");
  };
  const release = async (key: string) => {
    const ok = await setLead(key, "");
    if (!ok) toast(tr("Session lead"), tr("The link was not changed."), "error");
  };
  return (
    <Show when={current()}>
      <div class="session-lead-panel" aria-label={tr("Session lead")}>
        <Show when={lead()}>
          {(shown) => (
            <>
              <div class="session-lead-line">{tr("Reports to %1", shown().title || shown().name)}</div>
              <div class="session-lead-actions">
                <button type="button" class="btn small" onClick={() => open(shown().key)}>{tr("Open")}</button>
                <button type="button" class="btn small" title={tr("Open the lead with this session's last reply as a draft")}
                  onClick={() => open(shown().key)}>{tr("Send report")}</button>
                <button type="button" class="btn small ghost" onClick={() => void release(current()!.key)}>{tr("Detach")}</button>
              </div>
            </>
          )}
        </Show>
        <Show when={!lead() && children().length > 0}>
          <div class="session-lead-line">{tr("Leads")}</div>
          <For each={children()}>
            {(child) => (
              <div class="session-lead-row">
                <button type="button" class="link" onClick={() => open(child.key)}>{child.title || child.name}</button>
                <button type="button" class="btn small ghost" onClick={() => void release(child.key)}>{tr("Release")}</button>
              </div>
            )}
          </For>
        </Show>
        <Show when={!lead() && children().length === 0}>
          <Show when={candidates().length > 0} fallback={<div class="hint">{tr("No other session in this project can lead this one.")}</div>}>
            <label class="session-lead-line">
              {tr("Reports to")}
              <select class="field" aria-label={tr("Lead session")} value={leadLinks()[current()!.key] ?? ""}
                onChange={(event) => void choose(event.currentTarget.value)}>
                <option value="">{tr("nobody")}</option>
                <For each={candidates()}>{(row) => <option value={row.key}>{row.title || row.name}</option>}</For>
              </select>
            </label>
          </Show>
        </Show>
      </div>
    </Show>
  );
}

export type { Session };
