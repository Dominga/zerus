import { tr } from "../../i18n";

// Task 52: the team history's lines (Qt feedLine, tray/src/TeamModel.cpp),
// for the feed events the core reads (`feed:<id>`, core FeedEvent). Pure: a
// view draws what this says. An event without its own line shows its kind,
// as Qt does. Values sit apart by words and brackets, never by dots.

export interface HistoryEvent { seq?: number; at?: number; kind: string; detail: unknown }
export type HistoryTone = "neutral" | "muted" | "warn" | "danger";
export interface HistoryLine { text: string; by: string; tone: HistoryTone }

const obj = (value: unknown): Record<string, unknown> => (value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {});
const str = (value: unknown) => (typeof value === "string" ? value : "");
const num = (value: unknown) => (typeof value === "number" && Number.isFinite(value) ? value : 0);

/** One event's line; `tasks` give the titles the event leaves out. */
export function historyLine(event: HistoryEvent, tasks: readonly { id: number; title: string }[] = []): HistoryLine {
  const d = obj(event.detail);
  const line: HistoryLine = { text: event.kind, by: str(d.by) || str(d.member), tone: "neutral" };
  const id = num(d.task);
  const title = str(d.title) || tasks.find((task) => task.id === id)?.title || "";
  const ref = id ? (title ? `#${id} ${title}` : `#${id}`) : "";
  const attempt = typeof d.attempt === "number" ? d.attempt : undefined;
  switch (event.kind) {
    // Task 22: a start that waits for its host to finish starting.
    case "launch_waiting":
      line.text = tr("The host is still starting; starting again (%1 of %2)", num(d.wait), num(d.of));
      line.tone = "muted";
      break;
    // Task 36: a member woken when its own team run settled.
    case "run_settled_woken": {
      const member = str(d.member);
      if (d.delivered === false) {
        line.text = attempt === undefined ? tr("Its run settled, but waking %1 was refused (%2)", member, ref)
          : tr("Its run settled, but waking %1 was refused (%2, attempt %3)", member, ref, attempt);
        line.tone = "warn";
      } else {
        line.text = attempt === undefined ? tr("Woke %1: its run settled (%2)", member, ref)
          : tr("Woke %1: its run settled (%2, attempt %3)", member, ref, attempt);
      }
      break;
    }
  }
  return line;
}
