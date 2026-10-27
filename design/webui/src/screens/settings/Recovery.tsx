import { For, Show, createSignal } from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import { demoState, setPage, toast } from "../../state";
import { RECOVERY_RULES, type RecoveryRule } from "../../mock/settings-extra";
import { SectionHead, Group, Check, Select, Actions, Status } from "./ui";

// Settings → Automatic recovery (RecoverySettings.h). The policy lives in hgs
// and is shared by every connected machine; Save is compare-and-set against
// the version this page loaded.

const loaded = () => ({ enabled: true, mode: "reset", rules: RECOVERY_RULES.map((r) => ({ ...r })) });

function validDelays(text: string): number[] | undefined {
  const parts = text.split(",");
  if (!parts.length || parts.length > 16) return undefined;
  const list: number[] = [];
  for (const [n, raw] of parts.entries()) {
    const v = Number(raw.trim());
    const last = n === parts.length - 1;
    if (!Number.isInteger(v) || raw.trim() === "") return undefined;
    if (!((v >= 1 && v <= 86400) || (last && (v === -1 || (v === 0 && n > 0))))) return undefined;
    list.push(v);
  }
  return list;
}

export function RecoverySection() {
  const [policy, setPolicy] = createStore(loaded());
  const [message, setMessage] = createSignal("");
  const [saving, setSaving] = createSignal(false);
  const noMachines = () => demoState() === "empty";
  const syncing = () => demoState() === "loading" || saving();
  const usable = () => !noMachines() && demoState() !== "loading";
  const on = () => usable() && policy.enabled;
  const status = () =>
    noMachines() ? "To configure automatic recovery, enable local sessions for this computer in Machines or add a remote machine."
      : syncing() ? "Synchronizing recovery settings…"
      : demoState() === "error" ? "Waiting for build-02, win-lab. Changes will sync when connected; those machines keep their last settings."
      : "Waiting for build-02. Changes will sync when connected; those machines keep their last settings.";
  const save = () => {
    for (const r of policy.rules) {
      if (!validDelays(r.delays)) { setMessage("Use 1–16 values: delays from 1 to 86400 seconds; −1 or 0 may only be last, and 0 needs a preceding delay."); return; }
    }
    setMessage("");
    setSaving(true);
    setTimeout(() => { setSaving(false); setMessage("Saved."); toast("Automatic recovery", "Saved. Other machines receive it when they connect."); }, 900);
  };
  const rule = (r: RecoveryRule, i: number) => {
    const delaysOn = () => on() && policy.rules[i].on && (r.id !== "session_limit" || policy.mode === "interval");
    return (
      <Group class="rec-rule">
        <Check label={r.label} checked={policy.rules[i].on} disabled={!on()} onChange={(v) => setPolicy("rules", i, "on", v)} />
        <Show when={r.id === "session_limit"}>
          <div class="rec-mode">
            <Select value={policy.mode} label="Session usage limit recovery" disabled={!on() || !policy.rules[i].on} onChange={(v) => setPolicy("mode", v)}
              options={[{ value: "reset", label: "Wait for reset" }, { value: "interval", label: "Retry on a schedule" }]} />
            <p class="set-hint">Wait until the provider's reset time by default. If no reset time is available, recovery needs your attention. Choose a schedule to retry at your own intervals.</p>
          </div>
        </Show>
        <label class="rec-delays" classList={{ disabled: !delaysOn() }}>
          <span class="set-label">Retry after (seconds)</span>
          <input class="field mono grow" placeholder="15, 30, 60, 300, -1" value={policy.rules[i].delays} disabled={!delaysOn()}
            aria-invalid={!validDelays(policy.rules[i].delays)} onInput={(e) => setPolicy("rules", i, "delays", e.currentTarget.value)} />
        </label>
      </Group>
    );
  };
  return (
    <>
      <SectionHead title="Automatic recovery" intro="Shared by all connected machines. Agents finish their own retries first; recovery continues the same task." />
      <Group>
        <Check label="Recover automatically after temporary failures" checked={policy.enabled} disabled={!usable()} onChange={(v) => setPolicy("enabled", v)} />
      </Group>
      <div class="rec-rules"><For each={RECOVERY_RULES}>{(r, i) => rule(r, i())}</For></div>
      <div class="set-note">
        <p>Each positive number is a delay before the next attempt. End with −1 to stop, or 0 to repeat the previous delay forever.<br />Example: 300, 0 retries every 5 minutes until stopped.</p>
        <p>New messages and Stop cancel recovery. Sign-in, balance, other quota and context errors require your action.</p>
      </div>
      <Status kind={noMachines() ? "warn" : syncing() ? "busy" : demoState() === "error" ? "warn" : "muted"}>{status()}</Status>
      <Show when={noMachines()}>
        <Actions><button class="btn" onClick={() => setPage("machines")}>Open Machines</button></Actions>
      </Show>
      <Show when={message()}>
        <Status kind={message() === "Saved." ? "ok" : "error"}>{message()}</Status>
      </Show>
      <Actions>
        <button class="btn primary" disabled={!usable() || syncing()} onClick={save}>Save recovery settings</button>
        <button class="btn" disabled={syncing()} onClick={() => { setPolicy(reconcile(loaded())); setMessage(""); }}>Reload</button>
      </Actions>
    </>
  );
}
