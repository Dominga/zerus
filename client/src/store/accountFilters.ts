import { batch, createMemo, createSignal } from "solid-js";
import { accountsPage, loadAccountFilters } from "./accounts";
import { accountFilter, setAccountFilter } from "./ui";
import { accountChoices, type AccountChoice } from "../ui/sessions/scope-filters";

// Retain only selected captions/bindings, never a per-session account cache.
const [retained, setRetained] = createSignal<AccountChoice[]>([]);
export const filterAccounts = createMemo(() => accountChoices(accountsPage().accounts, accountsPage().binding_evidence_complete ? accountsPage().binding_evidence ?? [] : [], accountFilter(), retained()));
export const selectedFilterAccounts = createMemo(() => {
  const selected = new Set(accountFilter());
  return filterAccounts().filter((a) => a.bindings.some((b) => selected.has(b)));
});
export function chooseFilterAccounts(ids: string[]) {
  const wanted = new Set(ids), choices = filterAccounts().filter((a) => wanted.has(a.id));
  // The bounded Accounts view admits at most 64 * 256 current bindings and their home aliases.
  batch(() => { setRetained(choices); setAccountFilter([...new Set(choices.flatMap((a) => a.bindings))]); });
}
/** A menu or chip action uses the exact bindings it presented, never a regrouped account id. */
export function toggleFilterAccount(choice: AccountChoice, checked: boolean) {
  const next = new Set(accountFilter());
  for (const b of choice.bindings) { if (checked) next.add(b); else next.delete(b); }
  const kept = new Map<string, AccountChoice>();
  for (const c of [...retained(), ...(checked ? [choice] : [])]) {
    const bindings = c.bindings.filter((b) => next.has(b));
    if (!bindings.length) continue;
    const old = kept.get(c.id);
    kept.set(c.id, { ...c, bindings: [...new Set([...(old?.bindings ?? []), ...bindings])] });
  }
  batch(() => { setRetained([...kept.values()]); setAccountFilter([...next]); });
}
let reading: Promise<void> | undefined;
let generation = 0;
/** The hidden Accounts screen emits no change events. Pull only while this
 * explicit catalog request runs: at most 64 view reads, never another CLI read. */
export function openFilterAccounts(): Promise<void> {
  if (reading) return reading;
  const token = ++generation;
  const job = (async () => {
    await loadAccountFilters(true);
    for (let i = 0; i < 64 && token === generation && accountsPage().catalog_loading; i++) {
      await new Promise((resolve) => setTimeout(resolve, 250));
      if (token === generation) await loadAccountFilters();
    }
  })();
  reading = job;
  void job.finally(() => { if (reading === job) reading = undefined; }).catch(() => {});
  return job;
}
export function cancelFilterAccountReads() { generation++; reading = undefined; }
