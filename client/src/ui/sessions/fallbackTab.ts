import type { DetailTab } from "../../store/ui";

/** The tab to show when `current` is hidden, or undefined while it shows. A tab
 *  that disappears returns to Activity (SessionsWindow.cpp:4019-4020); the
 *  "+ New team" tab hides because its session just got its team (Start planning
 *  or Create team), and the view follows that team to its Team tab (owner,
 *  2026-11-10: the first press used to land on Activity, as this fallback ran
 *  before the Team tab's own follow-up). */
export function fallbackTab(list: readonly { id: DetailTab; show: boolean }[], current: DetailTab): DetailTab | undefined {
  if (list.find((t) => t.id === current)?.show) return undefined;
  if (current === "newteam" && list.find((t) => t.id === "team")?.show) return "team";
  return "activity";
}
