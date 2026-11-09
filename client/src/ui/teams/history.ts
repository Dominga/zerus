import { tr } from "../../i18n";

// Task 52: the team history's lines (Qt feedLine, tray/src/TeamModel.cpp),
// for the feed events the core reads (`feed:<id>`, core FeedEvent). Pure: a
// view draws what this says. An event without its own line shows its kind,
// as Qt does. Values sit apart by words, commas and brackets, never by dots
// (Qt's middle-dot separator is a comma here). Task 198: every kind Qt words.

export interface HistoryEvent { seq?: number; at?: number; kind: string; detail: unknown }
export type HistoryTone = "neutral" | "muted" | "warn" | "danger" | "done" | "active";
export interface HistoryLine { text: string; by: string; tone: HistoryTone }

const obj = (value: unknown): Record<string, unknown> => (value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {});
const str = (value: unknown) => (typeof value === "string" ? value : "");
const num = (value: unknown) => (typeof value === "number" && Number.isFinite(value) ? value : 0);
const list = (value: unknown): unknown[] => (Array.isArray(value) ? value : []);

/** A pipeline stage in words (Qt stageName). */
export function historyStage(kind: string): string {
  switch (kind) {
    case "review": return tr("Review");
    case "test": return tr("Test");
    case "command": return tr("Command");
    case "land": return tr("Land");
    case "implement": return tr("Implement");
    case "auto": return tr("Auto");
    case "sub_lead": return tr("Sub-lead");
    case "lead": return tr("Lead");
    default: return kind ? kind[0].toUpperCase() + kind.slice(1) : "";
  }
}

/** One event's line; `tasks` give the titles the event leaves out. */
export function historyLine(event: HistoryEvent, tasks: readonly { id: number; title: string }[] = []): HistoryLine {
  const d = obj(event.detail);
  const line: HistoryLine = { text: event.kind, by: str(d.by) || str(d.member), tone: "neutral" };
  const id = num(d.task);
  const title = str(d.title) || tasks.find((task) => task.id === id)?.title || "";
  const ref = id ? (title ? `#${id} ${title}` : `#${id}`) : "";
  const attempt = typeof d.attempt === "number" ? d.attempt : undefined;
  const at = attempt === undefined ? "" : tr(", attempt %1", attempt);
  const stage = historyStage(str(d.stage));
  const set = (text: string, tone: HistoryTone = "neutral") => { line.text = text; line.tone = tone; };
  switch (event.kind) {
    case "created": set(tr("Created the team")); break;
    case "planned": set(num(d.errors) ? tr("Changed the plan, %1 errors", num(d.errors)) : tr("Changed the plan, %1 tasks", num(d.tasks))); break;
    case "approved": set(tr("Approved the plan")); break;
    case "discarded": set(tr("Discarded the plan")); break;
    case "paused": set(tr("Paused the team")); break;
    case "resumed": set(tr("Resumed the team")); break;
    case "stopped": set(tr("Stopped the team"), "danger"); break;
    case "archived": set(tr("Archived the team")); break;
    case "done": set(tr("Done, %1 of %2 tasks accepted", num(d.accepted), num(d.tasks)), "done"); break;
    case "phase_added": {
      const phaseTitle = str(d.title);
      set(phaseTitle ? tr("Added phase %1, %2, %3 tasks", num(d.phase), phaseTitle, num(d.tasks)) : tr("Added phase %1, %2 tasks", num(d.phase), num(d.tasks)));
      break;
    }
    case "phase_approved": set(tr("Approved phase %1, %2 tasks", num(d.phase), num(d.tasks))); break;
    case "phase_addition_warned": {
      const count = list(d.tasks).length;
      set(count === 1 ? tr("Asked to add 1 task to running phase %1, waits for your confirmation", num(d.phase))
        : tr("Asked to add %1 tasks to running phase %2, waits for your confirmation", count, num(d.phase)), "warn");
      break;
    }
    case "phase_addition_declined": {
      const count = list(d.tasks).length;
      set(count === 1 ? tr("Declined adding 1 task to running phase %1", num(d.phase)) : tr("Declined adding %1 tasks to running phase %2", count, num(d.phase)));
      break;
    }
    case "phase_tasks_added":
      set(tr("Added %1 to running phase %2, outside its original scope", list(d.tasks).map((task) => `#${num(task)}`).join(", "), num(d.phase)), "warn");
      break;
    case "phase_moved": {
      const moved = "phase" in d ? num(d.phase) : num(d.from);
      set(d.renumbered === true ? tr("Moved phase %1 to position %2, drafts renumbered", moved, num(d.to)) : tr("Moved phase %1 to position %2", moved, num(d.to)));
      break;
    }
    case "phase_discarded": set(tr("Discarded phase %1, %2 tasks", num(d.phase), Array.isArray(d.tasks) ? d.tasks.length : num(d.tasks)), "muted"); break;
    case "phase_renumbered": {
      const moves = list(d.phases).map((value) => `${num(obj(value).from)}→${num(obj(value).to)}`);
      set(moves.length ? tr("Drafts renumbered: phases %1", moves.join(", ")) : tr("Drafts renumbered"), "muted");
      break;
    }
    case "all_decided": set(tr("All tasks decided, waiting for the lead"), "active"); break;
    case "finished": set(tr("Finished, %1 of %2 tasks accepted", num(d.accepted), num(d.tasks)), "done"); break;
    case "lead_told": if (str(d.kind) === "all_decided") set(tr("Told the Lead that all tasks are decided")); break;
    case "handed_out": {
      const by = str(d.by);
      line.by = !by || by === "Lead" || by === "Zerus" ? tr("Lead") : by;
      set(tr("Task to %1, %2%3", str(d.to), ref, at));
      break;
    }
    case "delivered": set(tr("Delivered %1%2", ref, at)); break;
    case "started": set(tr("Started %1%2", ref, at)); break;
    case "start_late": set(tr("Delivered and not started yet, %1%2", ref, at), "warn"); break;
    case "refused": set(tr("Not delivered %1%2 (%3); handed out again later", ref, at, str(d.error)), "warn"); break;
    case "unconfirmed": set(tr("Delivery unconfirmed, %1%2 (%3)", ref, at, str(d.error)), "danger"); break;
    case "reported": {
      const quote = str(d.text);
      const to = str(d.to) || tr("Lead");
      // Task #362: the lead's use of a member's last reply quotes the reply.
      set(quote ? tr("Report to %1, %2%3: “%4”", to, ref, at, quote) : tr("Report to %1, %2%3", to, ref, at));
      break;
    }
    case "interim_report": {
      const text = str(d.text);
      set(text ? tr("Interim report, %1%2: “%3”", ref, at, text) : tr("Interim report, %1%2", ref, at), "active");
      break;
    }
    case "report_replaced": set(tr("Replaced the captured reply, %1%2", ref, at)); break;
    case "question_asked": {
      const text = str(d.text);
      set(text ? tr("Question from the member, %1%2: %3", ref, at, text) : tr("A question for the Lead, %1%2", ref, at), "warn");
      break;
    }
    case "question_answered": set(tr("Answered the question, %1%2", ref, at)); break;
    case "held": {
      const reason = str(d.reason);
      set(reason ? tr("Held %1%2: %3", ref, at, reason) : tr("Held %1%2", ref, at), "muted");
      break;
    }
    case "released": set(tr("Released %1%2", ref, at)); break;
    case "member_adopted": set(tr("Adopted the existing session, %1", str(d.session))); break;
    case "stale_report": set(tr("Late report refused, %1%2", ref, at), "warn"); break;
    case "stage_started": {
      const rounds = num(d.max_rounds) > 0 ? num(d.max_rounds) : 3;
      const who = str(d.who);
      set(who ? tr("%1 round %2 of %3 started, %4, %5", stage, num(d.round), rounds, ref, who) : tr("%1 round %2 of %3 started, %4", stage, num(d.round), rounds, ref));
      break;
    }
    case "stage_passed": set(tr("%1 passed %2, round %3", stage, ref, num(d.round)), "done"); break;
    case "stage_returned": {
      const n = list(d.findings).length;
      set(tr("%1 returned %2, round %3, %4", stage, ref, num(d.round), n === 1 ? tr("1 finding") : tr("%1 findings", n)), "warn");
      break;
    }
    case "round_limit": set(tr("Out of rounds, %1 goes to the lead after %2 returns", ref, num(d.rounds)), "warn"); break;
    case "stages_passed": set(tr("Every stage passed, %1 waits for %2", ref, str(d.to) || tr("Lead"))); break;
    case "stage_skipped": set(tr("Skipped %1 on %2 at round %3: “%4”", stage, ref, num(d.round), str(d.reason)), "muted"); break;
    case "sent_back":
      set(d.stage ? tr("Sent %1 back from %2: “%3”", ref, stage, str(d.reason)) : tr("Sent %1 back: “%2”", ref, str(d.reason)), "warn");
      break;
    case "accepted": {
      const reason = str(d.reason);
      set(line.by === "Pipeline" || !reason ? tr("Accepted %1", ref) : tr("Accepted %1: “%2”", ref, reason), "done");
      break;
    }
    case "rejected": set(tr("Rejected %1%2: “%3”", ref, at, str(d.reason)), "danger"); break;
    case "reassigned": set(tr("Reassigned %1 to %2", ref, str(d.to))); break;
    case "retried": set(tr("Retried %1", ref)); break;
    case "taken_over": set(tr("Took over %1", ref)); break;
    case "cancelled": set(tr("Cancelled %1", ref)); break;
    case "checked": set(d.delivered === true ? tr("Confirmed delivery of %1%2", ref, at) : tr("Marked %1%2 as not delivered", ref, at)); break;
    case "no_report": set(tr("Turn ended without a report, %1%2", ref, at), "warn"); break;
    case "notice_unconfirmed": set(tr("Report for %1 may not have reached the Lead", ref), "warn"); break;
    case "member_started": set(tr("Session started, %1", str(d.session))); break;
    case "launch_failed": set(tr("Session failed to start: %1", str(d.error)), "danger"); break;
    // Task 22: a start that waits for its host to finish starting.
    case "launch_waiting":
      set(tr("The host is still starting; starting again (%1 of %2)", num(d.wait), num(d.of)), "muted");
      break;
    // Task 36: a member woken when its own team run settled.
    case "run_settled_woken": {
      const member = str(d.member);
      if (d.delivered === false) {
        set(attempt === undefined ? tr("Its run settled, but waking %1 was refused (%2)", member, ref)
          : tr("Its run settled, but waking %1 was refused (%2, attempt %3)", member, ref, attempt), "warn");
      } else {
        set(attempt === undefined ? tr("Woke %1: its run settled (%2)", member, ref)
          : tr("Woke %1: its run settled (%2, attempt %3)", member, ref, attempt));
      }
      break;
    }
    case "relaunch": set(tr("Start %1 again", str(d.member))); break;
    case "member_paused": set(tr("Paused %1", str(d.member))); break;
    case "member_resumed": set(tr("Resumed %1", str(d.member))); break;
    case "member_added": {
      const by = str(d.by), word = str(d.owner_word);
      set(word ? tr("Added %1 by %2: “%3”", str(d.member), by, word) : tr("Added %1 by %2", str(d.member), by));
      line.by = by;
      break;
    }
    case "max_members_raised": set(tr("Raised the member cap %1 → %2 on your word", num(d.old), num(d.new))); break;
    case "member_removed": set(tr("Removed %1 from the team", str(d.member))); break;
  }
  return line;
}

/** Qt's inbox kinds in words (TeamWorkspace::inboxStrip). */
export function inboxKind(kind: string): string {
  switch (kind) {
    case "report": return tr("report");
    case "question": return tr("question");
    case "permission": return tr("permission");
    case "lead_task": return tr("lead task");
    case "stalled": return tr("stalled");
    case "budget": return tr("budget");
    case "interim_report": return tr("interim");
    case "merge_waiting": return tr("merge waiting");
    case "attempt_recovered": return tr("attempt recovered");
    default: return kind;
  }
}

/** "09:41", as Qt's clock() of a feed line. */
export function feedClock(at: number): string {
  if (!at) return "";
  const date = new Date(at * 1000);
  return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

/** The feed lines a member focus leaves (Qt: its role sends them or they name it). */
export function focusedLines<T extends HistoryLine>(lines: readonly T[], role: string): T[] {
  return role ? lines.filter((line) => line.by === role || line.text.includes(role)) : [...lines];
}
