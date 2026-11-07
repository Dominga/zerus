import { For, Show } from "solid-js";
import { tr } from "../../i18n";
import type { CleanupProgram } from "./termination-offer";

/** Frozen native display data. No PID, label or selection is sent back as consent. */
export function TerminationPrograms(props: { programs: readonly CleanupProgram[]; force?: boolean }) {
  const list = () => <ul style={{ "max-height": "180px", overflow: "auto", margin: "8px 0", "padding-left": "0", "list-style": "none" }}>
    <For each={props.programs}>{(program) => <li dir="auto">{tr("%1 (PID %2)", program.label, String(program.pid))}</li>}</For>
  </ul>;
  return <Show when={props.programs.length > 0}>
    <section aria-label={tr("Programs using this folder")} class="termination-programs">
      <Show when={props.force} fallback={<details>
        <summary>{props.programs.length === 1
          ? tr("1 other program will be asked to stop. Save its work before continuing.")
          : tr("%1 other programs will be asked to stop. Save their work before continuing.", String(props.programs.length))}</summary>
        {list()}
      </details>}>
        <p class="hint">{tr("Force stop may discard unsaved work in these programs.")}</p>
        {list()}
      </Show>
    </section>
  </Show>;
}
