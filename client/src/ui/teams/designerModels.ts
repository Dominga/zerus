import { tr } from "../../i18n";
import { EFFORTS, KNOWN_MODELS } from "./catalogControls";

// Task #226: Qt's TeamDesignerPage model menu (TeamDesignerPage.cpp:321). The
// models come from the same lists New team and the catalog editor offer
// (catalogControls.ts: Qt Teams::knownModels until an account's catalog
// answers); DeepSeek runs its own model, so it has the default entry only.
// Effort is a submenu, disabled for DeepSeek and Kimi.

export const DESIGNER_AGENTS: [string, string][] = [
  ["claude", "Claude Code"],
  ["codex", "Codex"],
  ["kimi", "Kimi"],
  ["dsh", "DeepSeek"],
  ["opencode", "OpenCode"],
];

export function agentName(agent: string): string {
  return DESIGNER_AGENTS.find(([id]) => id === agent)?.[1] ?? agent;
}

export interface ModelSection {
  agent: string;
  label: string;
  models: string[];
}

/** Qt knownModels by agent; DeepSeek (dsh) offers only its default model. */
export function modelSections(): ModelSection[] {
  return DESIGNER_AGENTS.map(([agent, label]) => ({
    agent,
    label,
    models: agent === "dsh" ? [""] : (KNOWN_MODELS[agent] ?? [""]),
  }));
}

/** Whether the agent takes an effort choice (Qt disables it for dsh and kimi). */
export function effortAllowed(agent: string): boolean {
  return agent !== "dsh" && agent !== "kimi";
}

/** Qt updateModelButton's text: the model (or the agent's name), then the effort. */
export function designerModelLabel(agent: string, model: string, effort: string): string {
  const parts = [agent === "dsh" || !model ? agentName(agent) : model];
  if (effort && effortAllowed(agent)) parts.push(effort);
  return parts.join(" ");
}

/** The efforts a model offers: the known ones; an unknown current effort stays offered. */
export function designerEfforts(agent: string, effort = ""): string[] {
  if (!effortAllowed(agent)) return [];
  return effort && !EFFORTS.includes(effort) ? [...EFFORTS, effort] : [...EFFORTS];
}

/** The submenu's own label: "Effort: high", or "Effort: —" before a choice. */
export function effortLabel(effort: string): string {
  return `${tr("Effort")}: ${effort || "—"}`;
}
