import { expect, it } from "vitest";
import { createTeamPlanPreferences } from "../../store/teamPlan";
import type { TeamCard } from "../../store/teams";
import { fitPlanZoom, planMatches, planProblem, planTone } from "./planToolbar";

// Synthetic decoder controls only. These fragments are intentionally written
// by hand; product-written graph/held fixtures cover the mounted view separately.
const card = (state: string, held = false, code?: string) => ({ state, held, stop_outcome: code ? { code } : null }) as TeamCard;
it("maps upstream states to the five legend tones", () => {
  expect(["accepted", "in_progress", "reported", "pending", "cancelled"].map((state) => planTone(card(state)))).toEqual(["done", "work", "review", "notstarted", "cancelled"]);
});
it("combines All/Active/Problems with the two independent visibility toggles", () => {
  const state = createTeamPlanPreferences().get("a");
  const rows = [card("pending"), card("in_progress"), card("reported"), card("accepted"), card("cancelled"), card("pending", true)];
  expect(rows.filter((row) => planMatches(row, state))).toHaveLength(5);
  expect(rows.filter((row) => planMatches(row, { ...state, showCancelled: true }))).toHaveLength(6);
  expect(rows.filter((row) => planMatches(row, { ...state, showCancelled: true, hideFinished: true }))).toHaveLength(5);
  expect(rows.filter((row) => planMatches(row, { ...state, filter: "active" })).map((row) => row.state)).toEqual(["in_progress", "reported"]);
  expect(rows.filter((row) => planMatches(row, { ...state, filter: "problems" }))).toEqual([rows[5]]);
  expect(planMatches(card("cancelled", true), { ...state, filter: "problems" })).toBe(false);
});
it("uses held or unfinished/refused stop facts, never successful stop outcomes as problems", () => {
  for (const code of ["pending", "cancel_refused", "key_clear_refused"]) expect(planProblem(card("pending", false, code))).toBe(true);
  for (const code of ["unchanged", "released", "cancelled", "key_cleared"]) expect(planProblem(card("accepted", false, code))).toBe(false);
});
it("fits the widest open section within the fork's bounded zoom and ignores absent geometry", () => {
  expect(fitPlanZoom(502, [250, 1000])).toBe(0.5);
  expect(fitPlanZoom(2502, [1000])).toBe(1);
  expect(fitPlanZoom(102, [1000])).toBe(0.4);
  expect(fitPlanZoom(0, [1000])).toBeUndefined();
  expect(fitPlanZoom(500, [])).toBeUndefined();
});
