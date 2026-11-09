import { For, Show, createEffect, createSignal, onCleanup, onMount, type JSX } from "solid-js";
import { tr } from "../../../i18n";
import { useModel, windowPort } from "../../teams/ProposalCards";
import { DiskModel, decisionsLine, driveLine, folderKind, folderState, lastUse, size, type DiskPolicy } from "./disk-model";
import { TextInput } from "../../lib/TextField";

/** Task 109: the open section reads again this often, measuring a few folders each time. */
export const DISK_LIVE_MS = 10_000;

/** Settings > Build folders (task 108): the policy, what Zerus holds, a dry run and Clean now. */
export function DiskSection(): JSX.Element {
  const { model, tick } = useModel((changed) => new DiskModel(windowPort, changed));
  const read = () => (tick(), model);
  const [policy, setPolicy] = createSignal<DiskPolicy | undefined>();
  const [scope, setScope] = createSignal<"policy" | "all">("policy");
  onMount(() => {
    void model.load();
    const live = setInterval(() => void model.poll(), DISK_LIVE_MS);
    onCleanup(() => clearInterval(live));
  });
  // The form follows the stored policy when it changes (first read, a save,
  // another window); edits not yet saved stay while the live read repeats it.
  let stored = "";
  createEffect(() => {
    const now = read().view?.policy;
    if (!now || JSON.stringify(now) === stored) return;
    stored = JSON.stringify(now);
    setPolicy({ ...now });
  });
  const field = (key: keyof DiskPolicy, value: number | boolean) => setPolicy((now) => (now ? { ...now, [key]: value } : now));
  const number = (key: "unused_days" | "free_gb" | "free_percent" | "team_limit_gb", label: string, max: number) => (
    <label class="set-field">
      <span>{label}</span>
      <TextInput class="field" type="number" min={0} max={max} value={policy()?.[key] ?? 0} disabled={read().busy}
        onInput={(event) => field(key, Math.max(0, Math.min(max, Number(event.currentTarget.value) || 0)))} />
    </label>
  );
  return (
    <div class="set-section disk-section">
      <h2>{tr("Build folders")}</h2>
      <p class="set-note">{tr("Zerus removes only the build folders it created or was told to use, never a folder of yours, never sources, and never a folder a build or test is using; a removal that may have half happened waits for your review.")}</p>
      <Show when={policy()}>{(current) => (
        <div class="set-group" data-disk="policy">
          <label class="set-check">
            <input type="checkbox" checked={current().enabled} disabled={read().busy} onChange={(event) => field("enabled", event.currentTarget.checked)} />
            {tr("Clean up automatically on this computer")}
          </label>
          {number("unused_days", tr("Remove a folder unused for (days, 0: never)"), 365)}
          {number("free_gb", tr("Keep this much free on its drive (GB)"), 10000)}
          {number("free_percent", tr("or this share of the drive (%)"), 90)}
          {number("team_limit_gb", tr("Tell the lead when one team's build folders pass (GB)"), 100000)}
          <div class="set-actions">
            <button type="button" class="btn small" disabled={read().busy} onClick={() => void model.savePolicy(current())}>{tr("Save policy")}</button>
          </div>
        </div>
      )}</Show>
      <Show when={read().view}>{(view) => (
        <>
          <div class="set-group" data-disk="drives">
            <For each={view().drives}>{(drive) => <div classList={{ "disk-drive": true, warn: drive.low }}>{driveLine(drive)}</div>}</For>
            <div class="dim">{tr("Zerus holds %1 in build folders", size(view().held))}</div>
          </div>
          <div class="set-actions">
            <select class="field" value={scope()} disabled={read().busy} onChange={(event) => setScope(event.currentTarget.value as "policy" | "all")}>
              <option value="policy">{tr("What the policy removes now")}</option>
              <option value="all">{tr("Every folder of Zerus's")}</option>
            </select>
            <button type="button" class="btn small" disabled={read().busy} onClick={() => void model.clean(true, scope())}>{tr("Dry run")}</button>
            <button type="button" class="btn small danger" disabled={read().busy} onClick={() => void model.clean(false, scope())}>{tr("Clean now…")}</button>
            <button type="button" class="btn ghost small" disabled={read().busy} onClick={() => void model.load(true)}>{tr("Measure again")}</button>
          </div>
          <Show when={read().notice}><p class="team-waiting-notice" role="status">{read().notice}</p></Show>
          <Show when={read().decisions.length > 0 || !read().dry}>
            <div class="set-group" data-disk="decisions">
              <b>{decisionsLine(read().decisions, read().dry)}</b>
              <For each={read().decisions}>{(decision) => (
                <div class="disk-row" data-action={decision.action}>
                  <code class="ell" title={decision.path}>{decision.path}</code>
                  <span class="dim">{decision.action === "remove" ? `${decision.reason}; ${size(decision.bytes)}` : decision.detail}</span>
                </div>
              )}</For>
            </div>
          </Show>
          <For each={view().teams} fallback={<p class="dim">{tr("Zerus has recorded no build folder on this computer yet")}</p>}>{(team) => (
            <div class="set-group" data-disk-team={team.team}>
              <b>{team.team_name || tr("Sessions")}</b> <span class="dim">{size(team.bytes)}</span>
              <div class="set-actions">
                <button type="button" class="btn ghost small" disabled={read().busy} data-disk-clean-team={team.team}
                  title={tr("Remove the targets of this team's finished tasks; Zerus asks first and keeps a folder in use")}
                  onClick={() => void model.clean(false, `team:${team.team}`)}>{tr("Clean finished tasks…")}</button>
              </div>
              <For each={team.folders}>{(folder) => (
                <div class="disk-row" data-state={folder.state} data-kind={folderKind(folder)}>
                  <code class="ell" title={folder.path}>{folder.path}</code>
                  <span class="dim">{size(folder.size)}</span>
                  <span class="dim">{lastUse(folder.last_used)}</span>
                  <span classList={{ dim: true, ell: true, warn: folderKind(folder) === "busy" }}>{folderState(folder)}</span>
                  <Show when={folder.state === "review"}>
                    <button type="button" class="btn ghost small" disabled={read().busy} onClick={() => void model.forget(folder.path)}>{tr("I dealt with it")}</button>
                  </Show>
                </div>
              )}</For>
            </div>
          )}</For>
        </>
      )}</Show>
    </div>
  );
}
