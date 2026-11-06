import { For, Show } from "solid-js";
import { tr } from "../../i18n";
import type { CleanupProgram } from "./termination-offer";

/** Frozen native display data. No PID, label or selection is sent back as consent. */
export function TerminationPrograms(props: { programs: readonly CleanupProgram[]; force?: boolean }) {
  return <Show when={props.programs.length > 0}>
    <section aria-label={tr("Programs using this folder")} class="termination-programs">
      <p class="hint">{props.force
        ? tr("Force stop may discard unsaved work in these programs.")
        : tr("These programs will be asked to stop. Save their work before continuing.")}</p>
      <ul style={{ "max-height": "180px", overflow: "auto", margin: "8px 0", "padding-left": "0", "list-style": "none" }}>
        <For each={props.programs}>{(program) => <li dir="auto">{tr("%1 (PID %2)", program.label, String(program.pid))}</li>}</For>
      </ul>
    </section>
  </Show>;
}
