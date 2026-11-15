import { createSignal } from "solid-js";

export type PlanFilter = "all" | "active" | "problems";
export interface PlanPreferences {
  filter: PlanFilter;
  showCancelled: boolean;
  hideFinished: boolean;
  zoom: number;
  allCollapsed: boolean;
  /** Exceptions to allCollapsed; bounded independently of the queue pages. */
  sections: readonly number[];
}
export const PLAN_TEAMS_MAX = 64;
export const PLAN_SECTIONS_MAX = 200;
export const PLAN_ZOOM_MIN = 0.4;
export const PLAN_ZOOM_MAX = 1.6;
const defaults: PlanPreferences = { filter: "all", showCancelled: false, hideFinished: false, zoom: 1, allCollapsed: false, sections: [] };
export const planZoom = (value: number) => Number.isFinite(value) ? Math.max(PLAN_ZOOM_MIN, Math.min(PLAN_ZOOM_MAX, value)) : 1;
export const sectionCollapsed = (state: PlanPreferences, section: number) => state.allCollapsed !== state.sections.includes(section);

/** Viewer-only preferences. No transport, timer, journal or team mutation.
 * Kept by the UI store across Graph/Layers mounts; least recently edited teams
 * and section exceptions have explicit bounds. No task or session rows live here. */
export function createTeamPlanPreferences() {
  const [states, setStates] = createSignal<ReadonlyMap<string, PlanPreferences>>(new Map());
  const get = (team: string): PlanPreferences => states().get(team) ?? defaults;
  const update = (team: string, patch: Partial<PlanPreferences>) => {
    setStates((old) => {
      const next = new Map(old);
      const state = { ...(old.get(team) ?? defaults), ...patch };
      state.zoom = planZoom(state.zoom);
      state.sections = [...new Set(state.sections)].slice(-PLAN_SECTIONS_MAX);
      next.delete(team); next.set(team, state);
      while (next.size > PLAN_TEAMS_MAX) next.delete(next.keys().next().value!);
      return next;
    });
  };
  const toggleSection = (team: string, section: number) => {
    const state = get(team);
    update(team, { sections: state.sections.includes(section) ? state.sections.filter((id) => id !== section) : [...state.sections, section] });
  };
  return { get, update, toggleSection };
}
export type TeamPlanPreferences = ReturnType<typeof createTeamPlanPreferences>;
