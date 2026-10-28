import { createEffect, createRoot, on } from "solid-js";
import { activityPort } from "../bridge";
import { applyBatch, onReplaced, orderedRows } from "./catalog";
import { bindActivity, bindSelection } from "./actions";
import { observeMarks } from "./marks";
import { createActivity } from "./activity";
import { compactRows, page, selected, selectedKey, setCompactRows, setSelectedKey, subagentOpen } from "./ui";
import { startScreens } from "./screens";
import { documentVisibility } from "./visibility";
import type { Batch } from "../types";

// Wires the stores together: batches go to the catalog and the followed
// Activity, the selected session is the followed one, and a `replaced` run
// hands its selection and row layout to the new run of the same name.

export const activity = createActivity(activityPort);
bindActivity(activity);
bindSelection(() => ({ key: selectedKey(), subagent: subagentOpen() }));

/** Applies one batch from the bridge. */
export function receive(b: Batch) {
  applyBatch(b);
  observeMarks(b);
  if (b.activity.length) activity.applyActivity(b.activity);
}

createRoot(() => {
  createEffect(on(() => {
    const s = selected();
    return s ? `${s.key}\n${s.name}` : undefined;
  }, () => {
    const s = selected();
    activity.follow(s ? { id: s.key, computer: s.computer, key: s.catalogKey, name: s.name, runId: s.runId, conversationId: s.conversationId } : undefined);
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
