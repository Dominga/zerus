import { tr } from "../../i18n";

/**
 * What an empty Activity says (ActivityView.cpp:1145-1150): a live native
 * session that can take messages asks to sign in first or invites the first
 * message; otherwise activity will appear as the agent works, or, for a
 * session Zerus does not track, its terminal is where to look.
 */
export function emptyActivity(summary: Record<string, unknown> | undefined, tracked: boolean): { title: string; text: string } {
  const native = summary?.backend === "dsh" && summary.runtime_state === "live" && summary.read_only !== true;
  if (native && summary.auth_required === true) {
    return { title: tr("No messages yet"), text: tr("After signing in, send a message below to start this session.") };
  }
  if (native && summary.activity === "idle") {
    return { title: tr("Ready for your first message"), text: tr("Send a message below. The agent's replies, tools and approvals will appear here.") };
  }
  return {
    title: tr("Activity will appear here"),
    text: tracked ? tr("Messages, tools and approvals will appear as your agent works.") : tr("This session has no recorded activity. Open its terminal to see the agent."),
  };
}
