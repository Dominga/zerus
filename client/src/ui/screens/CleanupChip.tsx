import { Show, createMemo } from "solid-js";
import { showsCleanupChip } from "../../store/backgroundTasks";
import { cleanup, groupCleanupRows, openCleanup } from "../../store/terminationCleanup";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";

/** Permanent session-footer entry. Counts describe folders, never action IDs. */
export function CleanupReviewButton() {
  const folders = createMemo(() => groupCleanupRows(cleanup().rows));
  const count = createMemo(() => Math.max(cleanup().kept + cleanup().unknown,
    folders().filter((folder) => ["kept", "review", "unknown"].includes(folder.status)).length)
    + folders().filter((folder) => folder.status === "ready" && !folder.rows.some((row) => row.busy)).length);
  const caption = () => count() ? `${tr("Review working folders")}\n${tr("Working folders needing review: %1", count())}` : tr("Review working folders");
  return <button type="button" class="icon-btn small cleanup-review" classList={{ attention: count() > 0, warn: cleanup().kept > 0 || cleanup().unknown > 0 }}
    title={[caption(), count() ? cleanup().title : ""].filter(Boolean).join("\n")} aria-label={caption()} onClick={() => openCleanup()}>
    <Icon name="git-branch" size={16} /><Show when={count()}><span class="tabular worktree-review-count" aria-hidden="true">{count()}</span></Show>
  </button>;
}

/** Results stay in the bottom bar; running work has its own background item. */
export function CleanupChip() {
  return <Show when={showsCleanupChip(cleanup())}>
    <button type="button" class="chip-accent cleanup-result" classList={{ "warn-chip": cleanup().kept > 0 || cleanup().unknown > 0 }}
      title={cleanup().title} onClick={() => openCleanup()}>{cleanup().text}</button>
  </Show>;
}
