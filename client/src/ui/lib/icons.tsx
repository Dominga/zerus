import type { JSX } from "solid-js";
import { ICONS } from "./icons.generated";
import { QT_ICONS, ZERUS_PETAL } from "./qt-icons";
import type { AgentKind } from "../sessions/types";

// Static SVG markup: an icon costs one element and no reactive bookkeeping,
// which matters in virtualized lists that mount rows while scrolling.

const rotations = (count: number, body: (deg: number) => string) =>
  Array.from({ length: count }, (_, i) => body((360 / count) * i)).join("");

// Agent glyphs ported from tray/src/WorkspaceIcons.h (24x24, 1.6 stroke).
const AGENT_GLYPHS: Record<AgentKind, string> = {
  codex: rotations(6, (d) => `<path transform="rotate(${d} 12 12)" d="M12 5C20 2 23 12 16 15L10 12L10 7"/>`),
  claude: rotations(8, (d) => `<path transform="rotate(${d} 12 12)" d="M12 15V21"/>`),
  kimi: `<path d="M6 4V20M6 12L18 4M6 12L18 20"/>`,
  opencode: `<path d="M8 5L3 12L8 19M16 5L21 12L16 19M14 5L10 19"/>`,
  deepseek: `<path d="M3 10C5 5 15 4 18 10L23 7L22 13L18 13C15 22 3 19 3 10Z"/><circle cx="8" cy="10" r=".6" fill="currentColor"/>`,
  sh: QT_ICONS.terminal.body,
};

export const AGENT_LABEL: Record<AgentKind, string> = {
  claude: "Claude",
  codex: "Codex",
  kimi: "Kimi",
  deepseek: "DeepSeek",
  opencode: "OpenCode",
  sh: "Shell",
};

// Lucide names used across the mockup that mean the same thing as a Qt glyph.
// A name that is itself a Qt glyph (folder, settings, search, pause, play, copy,
// send, pin, trash, calendar, terminal, video, git-branch, chevron-*) needs no
// entry: Qt glyphs win by name. Names without a Qt counterpart stay Lucide.
export const QT_ALIAS: Record<string, string> = {
  bell: "attention",
  "bell-off": "mute",
  zap: "active",
  archive: "archived",
  monitor: "machines",
  "folder-open": "folder",
  "user-round-cog": "accounts",
  users: "agents",
  plus: "add",
  ellipsis: "more",
  "square-terminal": "terminal",
  paperclip: "attachment",
  mic: "microphone",
  square: "stop",
  "circle-stop": "stop",
  "check-check": "read-all",
  check: "task-check",
  "external-link": "external",
  "panel-right": "inspector",
  "panel-right-close": "collapse-panel",
  inbox: "task-inbox",
  "list-todo": "task-list",
  clock: "task-clock",
  funnel: "task-funnel",
  pencil: "task-edit",
  "arrow-up-to-line": "task-top",
  "pin-off": "unpinned",
  "refresh-cw": "refresh",
  "rotate-ccw": "reset",
  x: "close",
  "calendar-days": "calendar",
  "circle-question-mark": "question",
  "circle-alert": "context-warning",
  "triangle-alert": "context-warning",
  "git-compare-arrows": "git-diff",
  link: "connect",
};

interface Glyph { html: string; viewBox: string; stroke: number; fill: boolean }

// One entry per distinct name, so the table stays as small as the icon set.
const resolved = new Map<string, Glyph>();

/** A Qt glyph by name, then through QT_ALIAS, then the Lucide glyph. */
export function resolveIcon(name: string): Glyph {
  let glyph = resolved.get(name);
  if (glyph) return glyph;
  const alias = QT_ALIAS[name];
  const qt = QT_ICONS[name] ?? (alias ? QT_ICONS[alias] : undefined);
  glyph = qt
    ? { html: qt.body, viewBox: `0 0 ${qt.view ?? 24} ${qt.view ?? 24}`, stroke: qt.stroke ?? 1.6, fill: !!qt.fill }
    : { html: ICONS[name] ?? "", viewBox: "0 0 24 24", stroke: 2, fill: false };
  resolved.set(name, glyph);
  return glyph;
}

interface IconProps {
  name: string;
  size?: number;
  class?: string;
  style?: JSX.CSSProperties;
  /** Pen width in grid units; defaults to the glyph's own Qt pen (1.6) or Lucide's 2. */
  stroke?: number;
}

export function Icon(props: IconProps) {
  const glyph = () => resolveIcon(props.name);
  return (
    <svg
      class={props.class}
      style={props.style}
      width={props.size ?? 14}
      height={props.size ?? 14}
      viewBox={glyph().viewBox}
      fill={glyph().fill ? "currentColor" : "none"}
      stroke={glyph().fill ? "none" : "currentColor"}
      stroke-width={props.stroke ?? glyph().stroke}
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
      innerHTML={glyph().html}
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
