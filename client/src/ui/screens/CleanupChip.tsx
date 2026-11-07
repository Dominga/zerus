import { Show } from "solid-js";
import { showsCleanupChip } from "../../store/backgroundTasks";
import { cleanup, openCleanup } from "../../store/terminationCleanup";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";

/** Review is always reachable, including after a notice expires or is closed. */
export function CleanupChip() {
  return <button type="button" class="chip-accent cleanup-review" classList={{ "warn-chip": cleanup().kept > 0 || cleanup().unknown > 0 }} title={cleanup().rows.length || cleanup().kept || cleanup().unknown ? cleanup().title : tr("Review working folders")} aria-label={tr("Review working folders")} onClick={() => openCleanup()}><Icon name="git-branch" size={13} /><Show when={showsCleanupChip(cleanup())}><span>{cleanup().text}</span></Show></button>;
}
