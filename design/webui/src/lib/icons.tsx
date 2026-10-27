import type { JSX } from "solid-js";
import { ICONS } from "./icons.generated";
import type { AgentKind } from "../mock/types";

// Static SVG markup: an icon costs one element and no reactive bookkeeping,
// which matters in virtualized lists that mount rows while scrolling.

const ZERUS_PETAL =
  "M39 9 C54 3 70 13 77 29 C72 25 69 24 72 32 C81 44 77 58 65 65 C70 51 65 36 52 28 C40 21 30 25 22 34 C25 21 38 15 51 19 C57 21 56 16 51 13 C47 11 43 10 39 9 Z";

const rotations = (count: number, body: (deg: number) => string) =>
  Array.from({ length: count }, (_, i) => body((360 / count) * i)).join("");

// Agent glyphs ported from tray/src/WorkspaceIcons.h (24x24, 1.6 stroke).
const AGENT_GLYPHS: Record<AgentKind, string> = {
  codex: rotations(6, (d) => `<path transform="rotate(${d} 12 12)" d="M12 5C20 2 23 12 16 15L10 12L10 7"/>`),
  claude: rotations(8, (d) => `<path transform="rotate(${d} 12 12)" d="M12 15V21"/>`),
  kimi: `<path d="M6 4V20M6 12L18 4M6 12L18 20"/>`,
  opencode: `<path d="M8 5L3 12L8 19M16 5L21 12L16 19M14 5L10 19"/>`,
  deepseek: `<path d="M3 10C5 5 15 4 18 10L23 7L22 13L18 13C15 22 3 19 3 10Z"/><circle cx="8" cy="10" r=".6" fill="currentColor"/>`,
};

export const AGENT_LABEL: Record<AgentKind, string> = {
  claude: "Claude",
  codex: "Codex",
  kimi: "Kimi",
  deepseek: "DeepSeek",
  opencode: "OpenCode",
};

interface IconProps {
  name: string;
  size?: number;
  class?: string;
  style?: JSX.CSSProperties;
  stroke?: number;
}

export function Icon(props: IconProps) {
  return (
    <svg
      class={props.class}
      style={props.style}
      width={props.size ?? 14}
      height={props.size ?? 14}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width={props.stroke ?? 2}
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
      innerHTML={ICONS[props.name] ?? ""}
    />
  );
}

export function AgentGlyph(props: { agent: AgentKind; size?: number; class?: string }) {
  return (
    <svg
      class={props.class}
      width={props.size ?? 12}
      height={props.size ?? 12}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
      innerHTML={AGENT_GLYPHS[props.agent]}
    />
  );
}

export function ZerusMark(props: { size?: number; class?: string }) {
  return (
    <svg class={props.class} width={props.size ?? 24} height={props.size ?? 24} viewBox="0 0 100 100" aria-hidden="true"
      innerHTML={`<g fill="currentColor" transform="translate(50 50) scale(1.06) translate(-50 -50)">${[0, 120, 240]
        .map((d) => `<path transform="rotate(${d} 50 50) translate(56 30) scale(.88) translate(-56 -30)" d="${ZERUS_PETAL}"/>`)
        .join("")}</g>`}
    />
  );
}
