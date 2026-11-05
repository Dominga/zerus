import type { JSX } from "solid-js";

// The role palette (design/agent-teams HANDOFF §2): colours chosen away from
// the accent green, warn amber and danger red; every badge also carries the
// role's name. A role the palette does not name gets one of these colours by
// its name, so it keeps its colour across views.

const PALETTE: readonly { words: readonly string[]; dark: string; light: string }[] = [
  { words: ["lead", "coordinator"], dark: "#b79cff", light: "#6f4fd8" },
  { words: ["architect"], dark: "#8e9bff", light: "#3d4bc4" },
  { words: ["backend", "core", "rust", "cli", "server"], dark: "#6fa8ff", light: "#2f66c9" },
  { words: ["frontend", "ui", "solid", "web"], dark: "#f08bb4", light: "#b83a72" },
  { words: ["tester", "test", "qa"], dark: "#56c8e0", light: "#137f95" },
  { words: ["reviewer", "review"], dark: "#d9b38c", light: "#8a5a2b" },
  { words: ["integrator", "merger", "merge"], dark: "#a3b1c6", light: "#4d5d75" },
  { words: ["researcher", "research"], dark: "#b9d36b", light: "#5f7a12" },
];

/** The palette entry of a role: the first whose word the role's name holds, else one picked by the name. */
export function roleColor(role: string, lead = false): { dark: string; light: string } {
  const words = role.toLowerCase().split(/[^a-z0-9]+/).filter(Boolean);
  let hash = 0;
  for (const char of role) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  const entry = lead ? PALETTE[0]
    : PALETTE.find((found) => found.words.some((word) => words.includes(word))) ?? PALETTE[1 + (hash % (PALETTE.length - 1))];
  return { dark: entry.dark, light: entry.light };
}

/** The badge's colours as CSS variables; teams.css picks the one for the appearance. */
export function roleStyle(role: string, lead = false): JSX.CSSProperties {
  const color = roleColor(role, lead);
  return { "--role-dark": color.dark, "--role-light": color.light };
}
