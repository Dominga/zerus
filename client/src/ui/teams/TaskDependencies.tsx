import { For, Show, createEffect, createSignal, on } from "solid-js";
import { tr } from "../../i18n";
import { TextInput } from "../lib/TextField";
import type { TeamCard, TeamDependency } from "../../store/teams";
import { canSetDeps } from "./planTasks";

/** The complete edge list, including endpoints outside the graph's page. */
export function TaskDependencies(props: { task: TeamCard; status: string; busy: boolean; onSetDeps: (deps: TeamDependency[]) => void }) {
  const [number, setNumber] = createSignal("");
  createEffect(on(() => props.task.node, () => setNumber(""), { defer: true }));
  const editable = () => canSetDeps(props.task, props.status, props.busy);
  const id = () => Number(number());
  const valid = () => editable() && Number.isInteger(id()) && id() > 0 && id() <= 0xffff_ffff && id() !== props.task.node && !props.task.deps?.some((dep) => dep.node === id());
  return <section data-task-dependencies>
    <h4>{tr("Dependencies")}</h4>
    <For each={props.task.deps ?? []}>{(dep) => <p>
      <span>#{dep.node}<Show when={dep.kind === "soft"}> ({tr("soft")})</Show></span>{" "}
      <Show when={editable()}><button type="button" class="btn small" aria-label={tr("Remove dependency on #%1", dep.node)}
        onClick={() => props.onSetDeps((props.task.deps ?? []).filter((edge) => edge.node !== dep.node))}>{tr("Remove")}</button></Show>
    </p>}</For>
    <Show when={editable()}><form onSubmit={(event) => {
      event.preventDefault();
      if (valid()) { props.onSetDeps([...(props.task.deps ?? []), { node: id() }]); setNumber(""); }
    }}>
      <label>{tr("Dependency task number")} <TextInput historyKey={`team-dependency-${props.task.node}`} class="field" type="number" min="1" max="4294967295" step="1" value={number()} onInput={(event) => setNumber(event.currentTarget.value)} /></label>{" "}
      <button type="submit" class="btn small" disabled={!valid()}>{tr("Add dependency")}</button>
    </form></Show>
  </section>;
}
