import { tr } from "../../i18n";

// The designer's agent names and effort support. The selected native account's
// model catalog and the model's efforts come through catalogControls.

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

/** The submenu's own label: "Effort: high", or "Effort: —" before a choice. */
export function effortLabel(effort: string): string {
  return `${tr("Effort")}: ${effort || "—"}`;
}
