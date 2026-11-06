// Task 82 (owner, 2026-11-06: "heat is not telling"): the plan's heat is a
// task's lateness against the plan baseline, with its overrun against the
// estimate as a separate badge beside it. Own lateness tints the card;
// lateness inherited from a predecessor that ran late is shown muted, "from
// #N"; a task before its planned start, or waiting on dependencies that run
// on schedule, is cold. The core projects the numbers (ProfileTaskView); this
// module only words them.
import { tr } from "../../i18n";
import type { TeamProfileView } from "../../types";
import { keyName, lossKey } from "./profile-palette";

/** Own lateness at or past this tints the card fully. */
export const LATE_SCALE_S = 7200;

export interface TaskHeat {
  /** The card tint, 0..1: own lateness against the 2 h scale. */
  tint: number;
  /** "late 40m", "late 20m from #3", "on time", "lateness unknown". */
  label: string;
  /** Inherited, on time or unknown: muted text, no tint. */
  muted: boolean;
  /** The overrun badge: "+40m over 1h 00m", "within 1h 00m", "no estimate"; empty before the task started. */
  badge: string;
  /** The graph node's compact badge: "x1.7". */
  badgeCompact: string;
  /** The overrun's own scale, 0..1: at or under the estimate 0, twice it 1. */
  over: number;
  /** Planned and actual moments, own and inherited lateness, overrun and causes, one per line. */
  tip: string;
}

/** "<1m", "40m", "1h 05m", as the Qt GUI words a duration. */
export function durationText(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 60) return tr("<1m");
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return tr("%1m", minutes);
  return tr("%1h %2m", Math.floor(minutes / 60), String(minutes % 60).padStart(2, "0"));
}

/** "HH:mm" today, "6 Nov HH:mm" before; `now` in seconds since the epoch. */
export function momentText(at: number | null | undefined, now: number = Date.now() / 1000): string {
  if (at == null || !Number.isFinite(at) || at <= 0) return "";
  const when = new Date(at * 1000);
  const time = `${String(when.getHours()).padStart(2, "0")}:${String(when.getMinutes()).padStart(2, "0")}`;
  if (when.toDateString() === new Date(now * 1000).toDateString()) return time;
  return `${when.toLocaleDateString(undefined, { day: "numeric", month: "short" })} ${time}`;
}

function lossName(kind: string): string {
  const key = lossKey(kind);
  return key ? keyName(key) : kind;
}

/** The heat of one task, or null when the profile has no row for it. */
export function taskHeat(profile: TeamProfileView | null | undefined, task: number, now?: number): TaskHeat | null {
  const row = profile?.task_rows?.find((candidate) => candidate.task === task);
  if (!profile || !row) return null;
  const lines: string[] = [];
  let label = "";
  let muted = true;
  let tint = 0;
  const late = row.lateness;
  if (!late) {
    lines.push(tr("No plan baseline: this task was planned before lateness was measured"));
  } else {
    const moment = (at: number | null, none: string) => momentText(at, now) || none;
    lines.push(tr("Planned: start %1, end %2", moment(late.planned_start, tr("unknown")), moment(late.planned_end, tr("unknown"))));
    lines.push(tr("Actual: start %1, end %2", moment(late.actual_start, tr("not yet")), moment(late.actual_end, tr("not yet"))));
    if (!late.known) {
      label = tr("lateness unknown");
      lines.push(tr("Lateness unknown: %1", late.unknown || tr("an estimate is missing")));
    } else if (late.own_s > 0) {
      muted = false;
      tint = Math.min(1, late.own_s / LATE_SCALE_S);
      label = late.inherited_s > 0
        ? tr("late %1 (%2 from #%3)", durationText(late.own_s), durationText(late.inherited_s), late.from ?? 0)
        : tr("late %1", durationText(late.own_s));
    } else if (late.inherited_s > 0) {
      label = late.from ? tr("late %1 from #%2", durationText(late.inherited_s), late.from) : tr("late %1, inherited", durationText(late.inherited_s));
    } else {
      label = tr("on time");
    }
    if (late.known && late.start_late_s <= 0) lines.push(tr("On schedule"));
    else if (late.known) {
      let line = tr("Late %1: own %2", durationText(late.start_late_s), durationText(late.own_s));
      if (late.inherited_s > 0) {
        line += late.from
          ? tr(", %1 from #%2 (it ran late)", durationText(late.inherited_s), late.from)
          : tr(", %1 inherited", durationText(late.inherited_s));
      }
      lines.push(line);
    }
  }
  let badge = "";
  let badgeCompact = "";
  let over = 0;
  const overrun = row.overrun;
  if (overrun) {
    if (!overrun.started) {
      lines.push(tr("Overrun: not started"));
    } else if (!overrun.known) {
      badge = tr("no estimate");
      lines.push(tr("Overrun: own work %1, no estimate", durationText(overrun.work_s)));
    } else {
      const ratio = overrun.ratio.toFixed(1);
      badgeCompact = `x${ratio}`;
      badge = overrun.overrun_s > 0
        ? tr("+%1 over %2", durationText(overrun.overrun_s), durationText(overrun.estimate_s))
        : tr("within %1", durationText(overrun.estimate_s));
      over = Math.max(0, Math.min(1, overrun.ratio - 1));
      lines.push(tr("Overrun: own work %1 against a %2 estimate (x%3)", durationText(overrun.work_s), durationText(overrun.estimate_s), ratio));
    }
  }
  const causes = profile.losses
    .filter((loss) => loss.task === task && loss.seconds > 0)
    .sort((a, b) => b.seconds - a.seconds)
    .slice(0, 3)
    .map((loss) => `${lossName(loss.kind)} ${durationText(loss.seconds)}`);
  if (causes.length > 0) lines.push(tr("Causes: %1", causes.join("; ")));
  return { tint, label, muted, badge, badgeCompact, over, tip: lines.join("\n") };
}
