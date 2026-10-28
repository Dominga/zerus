import { For, Show, createSignal, onCleanup, onMount } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { inTauri } from "../../../bridge";
import { Icon } from "../../lib/icons";
import { Actions, Check, Group, NumberField, Row, SectionHead, Select, Status } from "./ui";

// Settings > Worktrees (tray/src/WorktreeSettings.h) as the approved mockup
// lays it out: each owning computer's cleanup rules, read from and applied
// to that computer's hgs. Opening it never removes a checkout. A policy
// that could not be read leaves nothing to apply (it fails closed).

interface Rules { enabled: boolean; after_session: boolean; unused_integrated: boolean; missing: boolean; grace_hours: number }
interface Policy { rules: Rules; raw: Record<string, unknown> }

// The open page's read, for opening the section again while it shows.
let reread: (() => void) | undefined;

/** Opening the section reads its computer's rules again (SettingsPage.h:157). */
export function refreshWorktrees() {
  reread?.();
}

export function WorktreesSection() {
  const [route, setRoute] = createSignal("");
  const [peers, setPeers] = createSignal<string[]>([]);
  const [busy, setBusy] = createSignal(false);
  // The policy read from the selected computer; a change is made on it.
  const [read, setRead] = createSignal<Policy | undefined>();
  // What the controls show: the last read, as edited.
  const [rules, setRules] = createSignal<Rules>({ enabled: false, after_session: false, unused_integrated: false, missing: false, grace_hours: 24 });
  const [status, setStatus] = createSignal("Refresh to read this computer's cleanup rules.");
  const [failed, setFailed] = createSignal(false);

  const answered = (saved: boolean) => (policy: Policy) => {
    setBusy(false);
    setRead(policy);
    setRules(policy.rules);
    setFailed(false);
    setStatus(saved ? "Cleanup settings saved on the selected computer." : "Cleanup settings loaded. Uncertain removals stay for manual review.");
  };
  const fail = (error: unknown) => {
    setBusy(false);
    setRead(undefined);
    setFailed(true);
    setStatus(String(error));
  };
  const begin = () => {
    if (busy() || !inTauri()) return false;
    setBusy(true);
    setFailed(false);
    setStatus("Reading cleanup settings…");
    return true;
  };
  const refresh = () => {
    if (!begin()) return;
    // The peers as this computer lists them now (WorktreeSettings::setPeers).
    void invoke<string[]>("worktree_policy_peers").then((list) => {
      setPeers(list);
      if (route() && !list.includes(route())) { setRoute(""); setRead(undefined); }
      return invoke<Policy>("worktree_policy_read", { route: route() });
    }).then(answered(false), fail);
  };
  const choose = (next: string) => {
    setRoute(next);
    setRead(undefined);
    refresh();
  };
  const apply = () => {
    const policy = read();
    if (!policy || !begin()) return;
    void invoke<Policy>("worktree_policy_apply", { route: route(), read: policy.raw, rules: rules() }).then(answered(true), fail);
  };
  // The section reads when it opens, as SettingsPage does.
  onMount(() => { reread = refresh; refresh(); });
  onCleanup(() => { if (reread === refresh) reread = undefined; });

  const ready = () => !busy() && !!read();
  const edit = (change: Partial<Rules>) => setRules({ ...rules(), ...change });
  return (
    <>
      <SectionHead title="Worktrees" />
      <Group>
        <Actions>
          <Select value={route()} label="Computer" disabled={busy()} onChange={choose}
            options={[{ value: "", label: "This computer" }, ...peers().map((peer) => ({ value: peer, label: peer }))]} />
          <button type="button" class="btn" disabled={busy()} onClick={refresh}><Icon name="refresh-cw" size={13} />Refresh</button>
        </Actions>
        <p class="set-group-hint">Cleanup runs on the computer that owns each worktree, even when this window is closed. Rules are enabled by default and apply across its projects. Other computers keep their own settings.</p>
        <Check label="Enable automatic worktree cleanup" checked={rules().enabled} disabled={!ready()} onChange={(on) => edit({ enabled: on })} />
        <For each={[
          ["after_session", "Clean integrated worktrees after the last session is archived or terminated"],
          ["unused_integrated", "Periodically clean unused integrated worktrees"],
          ["missing", "Forget verified managed worktrees whose folders no longer exist"],
        ] as const}>
          {([key, label]) => (
            <Check label={label} checked={rules()[key]} disabled={!ready() || !rules().enabled} onChange={(on) => edit({ [key]: on })} />
          )}
        </For>
        <Row label="Minimum inactivity for periodic cleanup" disabled={!ready() || !rules().enabled || !rules().unused_integrated}>
          <NumberField value={rules().grace_hours} min={1} max={8760} suffix="hours" label="Minimum inactivity for periodic cleanup"
            disabled={!ready() || !rules().enabled || !rules().unused_integrated} onChange={(hours) => edit({ grace_hours: hours })} />
        </Row>
        <p class="set-group-hint">{"Only verified Zerus-created worktrees qualify. Cleanup keeps branches and conversation history. Changed, untracked and ignored local files, sessions, running programs, locked folders and incomplete checks protect a worktree. Build folders and local configuration require review in Projects > Worktrees."}</p>
        <Show when={status()}>{(text) => <Status kind={busy() ? "busy" : failed() ? "error" : "muted"}>{text()}</Status>}</Show>
        <Actions><button type="button" class="btn primary" disabled={!ready()} onClick={apply}>Apply to this computer</button></Actions>
      </Group>
    </>
  );
}
