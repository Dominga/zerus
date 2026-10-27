import { For, Show, createEffect, createMemo, createSignal, on } from "solid-js";
import { createVirtualizer } from "@tanstack/solid-virtual";
import { Dialog } from "../components/Overlays";
import { MachineTile, type MachineState } from "../components/MachineTile";
import { Icon } from "../lib/icons";
import type { OsKind } from "../lib/os-icons";
import { dialog, setDialog } from "../state";
import { ALL_MACHINES, agentMissing, hasSavedSignIn, labIndex } from "../mock/lab-fleet";
import type { AccountMember } from "../mock/settings-extra";
import { MachineLabel } from "../screens/settings/machine-label";
import { PROVIDER_NAME, accounts, addTarget, currentAccount, online, setAccountNotice, setAccounts, setSelectedHost } from "../screens/Accounts";

// Add an account to other machines, step by step (AccountsPage::copy, scaled
// to a large fleet): 1 choose machines from a searchable virtual list, 2 choose
// how hgs adds the profile (saved sign-in, copy over SSH, sign in there, or an
// API key for DeepSeek) and its name, 3 confirm, then follow each machine's
// outcome. Only failures verified to have changed nothing can be retried.

type Method = "copy" | "signin" | "apikey";
type Outcome = "pending" | "working" | "added" | "needs-signin" | "failed" | "unknown";
interface Progress { host: string; state: Outcome; text: string; saved: boolean }

const ROW = 48;
const slug = (s: string) => s.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 48) || "account";

export function AddToMachineDialog() {
  const a = () => currentAccount();
  const provider = () => a()?.provider ?? "claude";
  const [step, setStep] = createSignal<1 | 2 | 3>(1);
  const [query, setQuery] = createSignal("");
  const [picked, setPicked] = createSignal<string[]>([]);
  const [availableOnly, setAvailableOnly] = createSignal(false);
  const [method, setMethod] = createSignal<Method>("copy");
  const [useSaved, setUseSaved] = createSignal(true);
  const [source, setSource] = createSignal("");
  const [name, setName] = createSignal("");
  const [profileId, setProfileId] = createSignal("");
  const [apiKey, setApiKey] = createSignal("");
  const [progress, setProgress] = createSignal<Progress[]>([]);

  const present = () => new Set((a()?.members ?? []).map((m) => m.host));
  const sources = () => (a()?.members ?? []).filter((m) => online(m.host) && m.state === "ready").map((m) => m.host);

  /** Why a machine cannot be chosen, or nothing when it can. */
  const blocked = (host: string): string | undefined => {
    const m = ALL_MACHINES.find((x) => x.alias === host)!;
    if (present().has(host)) return "Already has this account";
    if (!online(host)) return `Not connected · last seen ${m.lastSeen}`;
    if (m.connection === "reconnecting") return "Connecting…";
    if (agentMissing(host)) return `${PROVIDER_NAME[provider()]} is not installed`;
    if (provider() === "opencode" && !m.thisComputer) return "OpenCode accounts stay on this computer";
    return undefined;
  };
  const tileState = (host: string): MachineState => {
    const m = ALL_MACHINES.find((x) => x.alias === host)!;
    return !online(host) ? "offline" : m.connection === "reconnecting" ? "connecting" : "online";
  };
  const shown = createMemo(() => {
    const q = query().trim().toLowerCase();
    return ALL_MACHINES.filter((m) => (!q || m.alias.includes(q) || m.platform.includes(q)) && (!availableOnly() || !blocked(m.alias)));
  });
  const selectable = () => shown().filter((m) => !blocked(m.alias));
  const toggle = (host: string) => setPicked(picked().includes(host) ? picked().filter((h) => h !== host) : [...picked(), host]);
  const savedCount = () => picked().filter(hasSavedSignIn).length;

  // A fresh flow each time the dialog opens; a chosen machine (from elsewhere) starts selected.
  createEffect(on(dialog, (d) => {
    if (d !== "accounts-add-to-machine") return;
    setStep(1); setQuery(""); setAvailableOnly(false); setProgress([]); setApiKey("");
    setPicked(addTarget() !== "*" && !blocked(addTarget()) ? [addTarget()] : []);
    setMethod(provider() === "deepseek" ? "apikey" : sources().length ? "copy" : "signin");
    setUseSaved(true); setSource(sources()[0] ?? "");
    setName(a()?.label ?? ""); setProfileId(slug(`${provider()}-${a()?.label ?? "account"}`));
  }));

  let scroller: HTMLDivElement | undefined;
  const virtualizer = createVirtualizer({
    get count() { return shown().length; },
    getScrollElement: () => scroller ?? null,
    estimateSize: () => ROW,
    overscan: 6,
  });
  createEffect(on(() => shown().length, () => { scroller?.scrollTo({ top: 0 }); virtualizer.measure(); }, { defer: true }));

  // --- Step 3: run each machine in turn (simulated hgs account operations) ---
  const outcomeFor = (host: string, retry: boolean): Progress => {
    const saved = method() !== "apikey" && useSaved() && hasSavedSignIn(host);
    const i = labIndex(host);
    if (saved) return { host, state: "added", text: "Added with the saved sign-in on that machine", saved };
    if (method() === "apikey") return { host, state: "added", text: "API key saved", saved };
    if (method() === "signin") return { host, state: "needs-signin", text: "Profile created. Complete the sign-in in a terminal on that machine.", saved };
    if (!retry && i % 9 === 4) return { host, state: "failed", text: `Refused before any change: the profile folder on ${host} is not writable.`, saved };
    if (!retry && i % 11 === 6) return { host, state: "unknown", text: `The connection dropped while copying; the result is unknown. Check ${host} before trying again.`, saved };
    return { host, state: "added", text: `Sign-in copied from ${source()} into a new profile`, saved };
  };
  const addMember = (p: Progress) => {
    const ai = accounts.findIndex((x) => x.key === a()?.key);
    if (ai < 0 || present().has(p.host)) return;
    const member: AccountMember = { host: p.host, id: profileId(), state: p.state === "added" ? "ready" : "signed-out", isDefault: false, permission: "provider" };
    setAccounts(ai, "members", (list) => [...list, member]);
  };
  const run = (hosts: string[], retry = false) => {
    hosts.forEach((host, k) => {
      setProgress((list) => list.map((p) => (p.host === host ? { ...p, state: "working", text: "Working…" } : p)));
      setTimeout(() => {
        const result = outcomeFor(host, retry);
        setProgress((list) => list.map((p) => (p.host === host ? result : p)));
        if (result.state === "added" || result.state === "needs-signin") addMember(result);
      }, 500 + k * 350);
    });
  };
  const start = () => {
    setProgress(picked().map((host) => ({ host, state: "pending" as Outcome, text: "Waiting", saved: false })));
    setStep(3);
    run(picked());
  };
  const finished = () => progress().length > 0 && progress().every((p) => p.state !== "pending" && p.state !== "working");
  const count = (s: Outcome) => progress().filter((p) => p.state === s).length;
  // Once every machine has an outcome, problems come first so none hide in a long list.
  const RANK: Record<Outcome, number> = { failed: 0, unknown: 1, "needs-signin": 2, working: 3, pending: 4, added: 5 };
  const ordered = () => (finished() ? [...progress()].sort((x, y) => RANK[x.state] - RANK[y.state]) : progress());
  const close = () => {
    const added = count("added") + count("needs-signin");
    if (added) {
      setAccountNotice(`${a()?.label} added on ${added} machine${added === 1 ? "" : "s"}${count("needs-signin") ? `; ${count("needs-signin")} still need sign-in` : ""}.`);
      const first = progress().find((p) => p.state === "added" || p.state === "needs-signin");
      if (first) setSelectedHost(first.host);
    }
    setDialog(undefined);
  };
  const idValid = () => /^[a-z0-9][a-z0-9-]{0,47}$/.test(profileId());
  const canAdd = () => picked().length > 0 && name().trim().length > 0 && idValid() && (method() !== "apikey" || apiKey().trim().length > 0) && (method() !== "copy" || !!source() || useSaved() && savedCount() === picked().length);

  const STEPS = ["Machines", "How to add", "Add"];
  return (
    <Dialog id="accounts-add-to-machine" title={`Add ${a()?.label ?? "account"} to machines`} width="600px" icon="user-round-cog"
      footer={
        <>
          <Show when={step() === 1}><span class="dim atm-count">{picked().length ? `${picked().length} selected` : "Choose one or more machines"}</span></Show>
          <span class="spacer" />
          <Show when={step() !== 3} fallback={<button class="btn primary" disabled={!finished()} onClick={close}>Done</button>}>
            <Show when={step() === 2} fallback={<button class="btn" onClick={() => setDialog(undefined)}>Cancel</button>}>
              <button class="btn" onClick={() => setStep(1)}>Back</button>
            </Show>
            <Show when={step() === 1} fallback={<button class="btn primary" disabled={!canAdd()} onClick={start}>Add to {picked().length} machine{picked().length === 1 ? "" : "s"}</button>}>
              <button class="btn primary" disabled={!picked().length} onClick={() => setStep(2)}>Next</button>
            </Show>
          </Show>
        </>
      }>
      <ol class="atm-steps" aria-label="Steps">
        <For each={STEPS}>{(label, i) => <li classList={{ on: step() === i() + 1, done: step() > i() + 1 }}><b>{i() + 1}</b>{label}</li>}</For>
      </ol>

      <Show when={step() === 1}>
        <div class="atm-tools">
          <input class="field atm-search" type="search" placeholder={`Search ${ALL_MACHINES.length} machines`} value={query()} onInput={(e) => setQuery(e.currentTarget.value)} aria-label="Search machines" />
          <label class="check"><input type="checkbox" checked={availableOnly()} onChange={(e) => setAvailableOnly(e.currentTarget.checked)} />Available only</label>
          <button class="btn small ghost" disabled={!selectable().length} onClick={() => setPicked([...new Set([...picked(), ...selectable().map((m) => m.alias)])])}>Select all available</button>
        </div>
        <div class="atm-list" ref={(el) => { scroller = el; requestAnimationFrame(() => virtualizer._willUpdate()); }} role="listbox" aria-multiselectable="true" aria-label="Machines">
          <Show when={shown().length} fallback={<div class="atm-empty dim">No machine matches “{query()}”.</div>}>
            <div style={{ height: `${virtualizer.getTotalSize()}px`, position: "relative" }}>
              <For each={virtualizer.getVirtualItems()}>{(item) => {
                const m = () => shown()[item.index];
                return (
                  <Show when={m()}>
                    <div class="atm-row" role="option" aria-selected={picked().includes(m()!.alias)} aria-disabled={!!blocked(m()!.alias)}
                      classList={{ on: picked().includes(m()!.alias), blocked: !!blocked(m()!.alias), has: present().has(m()!.alias) }}
                      style={{ transform: `translateY(${item.start}px)` }}
                      onClick={() => { if (!blocked(m()!.alias)) toggle(m()!.alias); }}>
                      <span class="atm-check" aria-hidden="true">
                        <Show when={present().has(m()!.alias)} fallback={<Show when={picked().includes(m()!.alias)}><Icon name="check" size={12} /></Show>}><Icon name="check" size={12} /></Show>
                      </span>
                      <MachineTile os={m()!.platform as OsKind} state={tileState(m()!.alias)} size={28} />
                      <span class="atm-name"><MachineLabel alias={m()!.alias} /><span class="dim tabular">hgs {m()!.hgsVersion}</span></span>
                      <span class="atm-why" classList={{ ok: !blocked(m()!.alias) }}>{blocked(m()!.alias) ?? (hasSavedSignIn(m()!.alias) ? "Saved sign-in available" : "")}</span>
                    </div>
                  </Show>
                );
              }}</For>
            </div>
          </Show>
        </div>
        <p class="hint mock-note">MOCKUP: 45 synthetic lab machines join the demo fleet to show the list at scale.</p>
      </Show>

      <Show when={step() === 2}>
        <p>{a()?.label} will get a separate profile on {picked().length === 1 ? picked()[0] : `${picked().length} machines`}. Other accounts and sessions there stay as they are; after sign-in, the provider identity decides which account a profile belongs to.</p>
        <Show when={method() === "apikey"} fallback={
          <div class="col atm-how" role="radiogroup" aria-label="How to add">
            <Show when={savedCount()}>
              <label class="check"><input type="checkbox" checked={useSaved()} onChange={(e) => setUseSaved(e.currentTarget.checked)} />
                Use the saved sign-in where a machine already has one ({savedCount()} of {picked().length})</label>
            </Show>
            <label class="check"><input type="radio" name="atm-how" checked={method() === "copy"} disabled={!sources().length} onChange={() => setMethod("copy")} />
              Copy the existing sign-in over SSH from
              <select class="field atm-src" value={source()} disabled={method() !== "copy" || !sources().length} onChange={(e) => setSource(e.currentTarget.value)}>
                <For each={sources()}>{(h) => <option value={h}>{h}</option>}</For>
              </select>
            </label>
            <label class="check"><input type="radio" name="atm-how" checked={method() === "signin"} onChange={() => setMethod("signin")} />
              Create the profile and sign in on each machine (a terminal opens there)</label>
          </div>
        }>
          <div class="form-row"><label for="atm-key">DeepSeek API key</label>
            <input id="atm-key" class="field" type="password" autocomplete="off" maxLength={4096} value={apiKey()} onInput={(e) => setApiKey(e.currentTarget.value)} />
          </div>
          <p class="hint">The same key is saved on every selected machine.</p>
        </Show>
        <div class="form-row"><label for="atm-name">Account name</label>
          <input id="atm-name" class="field" maxLength={160} value={name()} onInput={(e) => setName(e.currentTarget.value)} />
        </div>
        <div class="form-row"><label for="atm-id">Profile ID</label>
          <input id="atm-id" class="field mono" maxLength={48} value={profileId()} classList={{ invalid: !idValid() }} onInput={(e) => setProfileId(e.currentTarget.value)} />
        </div>
        <Show when={!idValid()}><p class="hint warn">Use lowercase letters, digits and hyphens, starting with a letter or digit.</p></Show>
      </Show>

      <Show when={step() === 3}>
        <p class="atm-summary">
          <Show when={finished()} fallback={<>Adding {a()?.label} to {progress().length} machine{progress().length === 1 ? "" : "s"}…</>}>
            {count("added")} added{count("needs-signin") ? `, ${count("needs-signin")} need sign-in` : ""}{count("failed") ? `, ${count("failed")} failed` : ""}{count("unknown") ? `, ${count("unknown")} unknown` : ""}.
          </Show>
        </p>
        <div class="atm-results">
          <For each={ordered()}>{(p) => (
            <div class={`atm-result r-${p.state}`}>
              <span class="atm-ricon" aria-hidden="true">
                <Show when={p.state === "working"} fallback={<Icon name={p.state === "added" ? "check" : p.state === "needs-signin" ? "key-round" : p.state === "failed" ? "x" : p.state === "unknown" ? "circle-alert" : "clock"} size={14} />}><i class="atm-spin" /></Show>
              </span>
              <MachineLabel alias={p.host} />
              <span class="atm-rtext">{p.text}</span>
              <Show when={p.state === "failed"}><button class="btn small" onClick={() => run([p.host], true)}>Retry</button></Show>
              <Show when={p.state === "needs-signin"}><button class="btn small" onClick={() => setAccountNotice(`Complete ${PROVIDER_NAME[provider()]} sign-in in the terminal on ${p.host}; this page updates automatically.`)}>Sign in…</button></Show>
            </div>
          )}</For>
        </div>
        <Show when={count("unknown")}><p class="hint">An unknown result is never retried automatically: the copy may have finished. Open the machine in Accounts to check it.</p></Show>
      </Show>
    </Dialog>
  );
}
