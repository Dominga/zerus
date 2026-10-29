import { batch, createEffect, createRoot, on } from "solid-js";
import { activityPort, composerTab } from "../bridge";
import { applyBatch, onReplaced, orderedRows, setDraftKeys } from "./catalog";
import { composer } from "./composer";
import { num } from "./settings";
import { bindActivity, bindSelection } from "./actions";
import { observeMarks } from "./marks";
import { createActivity } from "./activity";
import { applyGrouping } from "./organization";
import { activityReadOnly } from "./activitySource";
import { compactRows, detailTab, page, selected, selectedKey, setCompactRows, setSelectedKey, subagentOpen } from "./ui";
import { startScreens } from "./screens";
import { documentVisibility } from "./visibility";
import type { Batch } from "../types";

// Wires the stores together: batches go to the catalog, the grouping, the
// followed Activity and the open composer, the selected session is the
// followed one and has the open composer, and a
// `replaced` run hands its selection and row layout to the new run of the
// same name. An archived row is never followed (Qt streams only rows
// without an archive id); store/archiveActivity.ts reads its recorded
// Activity.

export const activity = createActivity(activityPort);
bindActivity(activity);
bindSelection(() => ({ key: selectedKey(), subagent: subagentOpen() }));

/** Applies one batch from the bridge: its rows and their grouping are one reactive update. */
export function receive(b: Batch) {
  batch(() => {
    applyBatch(b);
    applyGrouping(b);
  });
  observeMarks(b);
  if (b.activity.length) activity.applyActivity(b.activity);
  if (b.composer) composer.receive(b.composer);
  if (b.draft_keys) setDraftKeys(b.draft_keys);
}

createRoot(() => {
  createEffect(on(() => {
    const s = selected();
    return s ? `${s.key}\n${s.name}` : undefined;
  }, () => {
    const s = selected();
    activity.follow(s && !activityReadOnly(s.key) ? { id: s.key, computer: s.computer, key: s.catalogKey, name: s.name, runId: s.runId, conversationId: s.conversationId } : undefined);
  }));
  // The selected session's composer. A rename keeps the catalog key, so Rust
  // moves its draft to the new name. An archived row's recorded Activity has
  // no composer (store/activitySource.ts), so none is open for it.
  createEffect(on(() => {
    const s = selected();
    return s && !activityReadOnly(s.key) ? `${s.computer}\n${s.catalogKey}\n${s.name}\n${s.state === "archived" ? s.archiveId ?? "" : ""}` : undefined;
  }, (identity) => {
    const s = selected();
    void composer.open(identity && s ? { computer: s.computer, key: s.catalogKey, name: s.name, archive_id: s.state === "archived" ? s.archiveId ?? null : null } : undefined);
  }));
  // The detail tab that shows: behind Terminal a failed read waits Qt's
  // background interval (Settings > Processes), which Rust reads when told.
  createEffect(on(() => [detailTab(), num("processes/backgroundSeconds"), num("processes/activeSeconds")] as const, ([tab]) => {
    void composerTab(tab).catch(() => {});
  }));
});

// `?select=<session name>` opens that session once it is listed (screenshots, scripted measurements).
const wanted = new URLSearchParams(location.search).get("select");
if (wanted) {
  createRoot((dispose) => {
    createEffect(() => {
      const row = orderedRows().find((s) => s.name === wanted);
      if (!row) return;
      if (!selectedKey()) setSelectedKey(row.key);
      dispose();
    });
  });
}

onReplaced((from, to) => {
  if (compactRows[from]) setCompactRows(to, true);
  if (selectedKey() === from) setSelectedKey(to);
});

// The Overview, Projects, Machines and Accounts reads run while their page shows.
startScreens(page, documentVisibility);
