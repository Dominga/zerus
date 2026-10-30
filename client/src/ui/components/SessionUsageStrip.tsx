import { Show } from "solid-js";
import { page, selected, subagentOpen } from "../../store/ui";
import { AccountUsageChip } from "../sessions/UsageChips";
import { usageStripVisible } from "../sessions/session-usage-ui";

/** Qt's fixed sessionUsageStrip; usage updates keep this owner mounted. */
export function SessionUsageStrip() {
  return <Show when={usageStripVisible(page(), selected(), subagentOpen())}>
    <AccountUsageChip session={selected()!} />
  </Show>;
}
