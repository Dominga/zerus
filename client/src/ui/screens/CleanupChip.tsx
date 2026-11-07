import { Show } from "solid-js";
import { showsCleanupChip } from "../../store/backgroundTasks";
import { cleanup, openCleanup } from "../../store/terminationCleanup";

/** Held results and their review; running work has the Background tasks item. */
export function CleanupChip() {
  return <Show when={showsCleanupChip(cleanup())}><button type="button" class="chip-accent" style={{ "white-space": "pre" }} classList={{ "warn-chip": cleanup().kept > 0 || cleanup().unknown > 0 }} title={cleanup().title} aria-label={cleanup().title} onClick={() => openCleanup()}>{cleanup().text}</button></Show>;
}
