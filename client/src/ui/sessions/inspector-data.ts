import { sessionUsageView } from "./session-usage-ui";
import { object, text } from "../screens/tasks-ui";
export function sessionPlan(value: unknown) {
  const lists = object(object(value).task_lists), main = object(lists.main);
  const items = Array.isArray(main.items) ? main.items : [];
  return items.slice(0, 100).map((v) => { const item = object(v); return { title: text(item.title), status: text(item.status) }; }).filter((v) => v.title);
}
export function sessionGoal(value: unknown) {
  const goal = object(object(value).goal);
  return { objective: text(goal.objective), completion: text(goal.completion_criterion), status: text(goal.status) };
}
export function usageRows(value: unknown): [string, string][] {
  const view = sessionUsageView(object(value).session_usage);
  return [...view.rows.map((row): [string, string] => [row.label, row.value]), ...(view.context ? [["Latest context", view.context] as [string, string]] : [])];
}
