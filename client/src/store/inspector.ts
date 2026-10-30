import { createMemo } from "solid-js";
import { skin } from "../skin/runtime";
import { chord } from "./shortcuts";
import { settings, setSetting } from "./settings";

export const INSPECTOR_TABS = ["plan", "details", "worktrees", "tasks"] as const;
export type InspectorTab = typeof INSPECTOR_TABS[number];
export const INSPECTOR_WIDTH = { initial: 400, min: 240, max: 900 };
export { panelGeometry, closedCards } from "./inspector-layout";
import { closedCards } from "./inspector-layout";
export const inspectorOpen = () => typeof settings["workspace/inspectorOpen"] === "boolean" ? settings["workspace/inspectorOpen"] as boolean : skin().layout.inspector === "docked";
export const inspectorTab = (): InspectorTab => INSPECTOR_TABS.find((t) => t === settings["workspace/inspectorTab"]) ?? "plan";
export const inspectorWidth = () => Math.max(240, Math.min(900, Number(settings["workspace/inspectorWidth"]) || 400));
export const inspectorClosed = createMemo(() => new Set(closedCards(settings["workspace/inspectorClosed"])));
export const setInspectorOpen = (open: boolean) => { void setSetting("workspace/inspectorOpen", open); };
export const setInspectorTab = (tab: InspectorTab) => { void setSetting("workspace/inspectorTab", tab); };
export const setInspectorWidth = (width: number) => { void setSetting("workspace/inspectorWidth", Math.round(Math.max(240, Math.min(900, width)))); };
export function toggleInspectorCard(id: string) {
  const next = [...inspectorClosed()].filter((key) => key !== id);
  if (!inspectorClosed().has(id)) next.push(id);
  void setSetting("workspace/inspectorClosed", JSON.stringify(next.slice(-128)));
}
export function openInspector(tab: InspectorTab) { setInspectorTab(tab); setInspectorOpen(true); }

export function inspectorHint(message: string) {
  const keys = chord("panel.toggle");
  return keys ? `${message} (${keys})` : message;
}
