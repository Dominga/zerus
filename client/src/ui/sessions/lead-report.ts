import { tr } from "../../i18n";
import type { ActivityEvent } from "../../types";

// Qt parity (I4): the Session lead panel's Send report
// (SessionsWindow::sendReportToLead). The report is a draft in the lead's
// composer; nothing is sent.

/** The session's last reply: the newest main-agent Stop or AgentMessage with text, trimmed; "" without one. */
export function lastReply(events: readonly ActivityEvent[]): string {
  for (let at = events.length - 1; at >= 0; at--) {
    const event = events[at];
    if (event.agent_id || (event.type !== "Stop" && event.type !== "AgentMessage")) continue;
    const reply = (event.detail ?? "").trim();
    if (reply) return reply;
  }
  return "";
}

/** The lead's draft: "Report from <session>:", then the reply after a blank line when there is one. */
export function reportDraft(title: string, reply: string): string {
  return tr("Report from %1:", title) + (reply ? `\n\n${reply}` : "");
}
