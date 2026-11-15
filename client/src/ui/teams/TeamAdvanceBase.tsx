import { For, Show, createSignal, onMount, onCleanup } from "solid-js";
import { teams, type TeamBaseFacts } from "../../store/teams";
import { tr } from "../../i18n";
import { TextInput } from "../lib/TextField";

/** An explicit bounded read when opened. The read's revision stays with the
 * selection; a later store refresh never renews this person's confirmation. */
export function TeamAdvanceBase(props: {
  team: string; busy: boolean; onCancel: () => void;
  onAdvance: (commit: string, revision: number) => void;
  load?: (team: string) => Promise<TeamBaseFacts>;
}) {
  const [facts, setFacts] = createSignal<TeamBaseFacts>();
  const [commit, setCommit] = createSignal("");
  const [failure, setFailure] = createSignal("");
  let alive = true;
  onCleanup(() => { alive = false; });
  onMount(async () => {
    try {
      const read = await (props.load ?? teams.base)(props.team);
      if (!alive) return;
      setFacts(read);
      // Prefer the locally fetched origin/main only as a form suggestion.
      // The operation checks both literal refs again under its writer lock.
      setCommit((read.refs.find((r) => r.reference === "refs/remotes/origin/main") ?? read.refs[0])?.commit ?? "");
    } catch (error) { if (alive) setFailure(String(error)); }
  });
  const valid = () => /^(?:[a-f0-9]{40}|[a-f0-9]{64})$/.test(commit()) && commit() !== facts()?.base_commit;
  return <section class="team-notice" data-team-base-form aria-label={tr("Advance base")}>
    <Show when={failure()}>{(error) => <p class="notice danger" role="alert">{error()}</p>}</Show>
    <Show when={facts()} fallback={<Show when={!failure()}><p role="status">{tr("Reading the current base and main commits…")}</p></Show>}>
      {(read) => <>
        <dl class="team-facts"><dt>{tr("Current base")}</dt><dd class="mono">{read().base_commit}</dd>
          <For each={read().refs}>{(ref) => <><dt>{ref.reference}</dt><dd class="mono">{ref.commit}</dd></>}</For>
        </dl>
        <p>{tr("Future hand-outs use the new base. Earlier hand-outs keep their assigned base. Branches are unchanged.")}</p>
        <label>{tr("New base commit")}<TextInput class="field mono" aria-label={tr("New base commit")} value={commit()} onInput={(event) => setCommit(event.currentTarget.value.trim())} /></label>
        <button type="button" class="btn small primary" disabled={props.busy || !valid()} data-team-base-continue
          onClick={() => { if (valid() && !props.busy) props.onAdvance(commit(), read().revision); }}>{tr("Continue")}</button>
      </>}
    </Show>
    <button type="button" class="btn small" onClick={props.onCancel}>{tr("Cancel")}</button>
  </section>;
}
