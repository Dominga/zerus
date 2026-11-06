import { tr } from "../../i18n";

// Task #79 (owner, 2026-11-06: "All colours must be clearly readable"): the
// team profile's span palette, the same table as Qt's TeamProfile::kindStyles
// (tray/src/TeamProfile.cpp; a test keeps them equal). One fixed palette in
// every skin: any two kinds at least 24 apart (CIE76), every colour at least
// 3:1 against every shipped lane background, and every wait its own hatch, so
// a wait is told by its pattern too, not only by hue.

export type Pattern = "solid" | "dots" | "bdiag" | "fdiag" | "diagcross" | "hor" | "ver" | "cross";
export interface KindStyle { key: string; dark: string; light: string; pattern: Pattern }

export const KIND_STYLES: readonly KindStyle[] = [
  { key: "work", dark: "#07ab2d", light: "#0f7d41", pattern: "solid" },
  { key: "rework", dark: "#f3a065", light: "#803e03", pattern: "solid" },
  { key: "build", dark: "#e5c043", light: "#8a7216", pattern: "solid" },
  { key: "review", dark: "#8075e6", light: "#3c3886", pattern: "solid" },
  { key: "test", dark: "#278cbc", light: "#137aa6", pattern: "solid" },
  { key: "stage", dark: "#98b3f2", light: "#2b6dc0", pattern: "solid" },
  { key: "blocked", dark: "#d1625b", light: "#81191e", pattern: "solid" },
  { key: "background", dark: "#0ba4a7", light: "#0d8072", pattern: "solid" },
  { key: "delivery", dark: "#848484", light: "#666666", pattern: "dots" },
  { key: "member", dark: "#c362a2", light: "#78185d", pattern: "bdiag" },
  { key: "deps", dark: "#708e34", light: "#416000", pattern: "fdiag" },
  { key: "lead", dark: "#be57de", light: "#6a1a84", pattern: "diagcross" },
  { key: "folder_held", dark: "#bc7305", light: "#bc7305", pattern: "hor" },
  { key: "hold", dark: "#c4c4c4", light: "#5f3643", pattern: "ver" },
  { key: "stage_wait", dark: "#6ad0fb", light: "#224560", pattern: "cross" },
];

/** A span's palette key, as Qt's kindKey: question, permission, stop and no_report are "blocked"; delivery and start_delay one key; other stages "stage". */
export function kindKey(kind: string, stageKind = ""): string {
  if (["question", "permission", "stop", "no_report"].includes(kind)) return "blocked";
  if (kind === "delivery" || kind === "start_delay") return "delivery";
  if (kind === "stage") return stageKind === "review" || stageKind === "test" ? stageKind : "stage";
  return kind;
}

/** A profile loss's palette key (the loss kinds hgs reports); none for a loss no span draws. */
export function lossKey(kind: string): string | undefined {
  const key = { rework: "rework", member: "member", member_wait: "member", deps: "deps", blocked_chain: "deps", lead: "lead",
    folder_held: "folder_held", hold: "hold", stage_wait: "stage_wait" }[kind];
  return key && KIND_STYLES.some((style) => style.key === key) ? key : undefined;
}

export const styleOf = (key: string) => KIND_STYLES.find((style) => style.key === key);
export const colorOf = (key: string, dark: boolean) => { const style = styleOf(key); return style ? (dark ? style.dark : style.light) : undefined; };

/** The legend's name of a key (Qt's keyName). */
export function keyName(key: string): string {
  switch (key) {
    case "work": return tr("Work");
    case "rework": return tr("Rework (a returned attempt)");
    case "build": return tr("Build and merge");
    case "review": return tr("Review stage");
    case "test": return tr("Test stage");
    case "stage": return tr("Other stage");
    case "blocked": return tr("Blocked (question, permission or stop)");
    case "background": return tr("Background work");
    case "delivery": return tr("Delivery or start delay");
    case "member": return tr("Waited for a free member");
    case "deps": return tr("Waited for dependencies");
    case "lead": return tr("Waited for the lead");
    case "folder_held": return tr("Folder held by another task");
    case "hold": return tr("Held");
    case "stage_wait": return tr("Waited for a stage");
    default: return key;
  }
}

/** The CSS background of a swatch: a solid fill, or the wait's hatch over a faint fill. */
export function swatchBackground(style: KindStyle, dark: boolean): string {
  const c = dark ? style.dark : style.light;
  const line = (angle: number) => `repeating-linear-gradient(${angle}deg, ${c} 0 1.5px, transparent 1.5px 4px)`;
  switch (style.pattern) {
    case "solid": return c;
    case "dots": return `radial-gradient(${c} 1px, transparent 1.2px) 0 0 / 3px 3px`;
    case "bdiag": return line(45);
    case "fdiag": return line(-45);
    case "diagcross": return `${line(45)}, ${line(-45)}`;
    case "hor": return line(0);
    case "ver": return line(90);
    case "cross": return `${line(0)}, ${line(90)}`;
  }
}

const channel = (hex: string, at: number) => parseInt(hex.slice(at, at + 2), 16) / 255;
const linear = (v: number) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
export function luminance(hex: string): number {
  return 0.2126 * linear(channel(hex, 1)) + 0.7152 * linear(channel(hex, 3)) + 0.0722 * linear(channel(hex, 5));
}
/** WCAG contrast ratio of two #rrggbb colours. */
export function contrast(a: string, b: string): number {
  const [x, y] = [luminance(a), luminance(b)];
  return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
}
/** A label's colour on a bar of `fill`: near black or white, whichever reads better. */
export const labelOn = (fill: string) => (contrast(fill, "#101418") >= contrast(fill, "#ffffff") ? "#101418" : "#ffffff");

/** CIE76 distance of two #rrggbb colours. */
export function distance(a: string, b: string): number {
  const lab = (hex: string) => {
    const [r, g, bl] = [1, 3, 5].map((at) => linear(channel(hex, at)));
    const f = (t: number) => (t > 0.008856 ? Math.cbrt(t) : 7.787 * t + 16 / 116);
    const x = (0.4124 * r + 0.3576 * g + 0.1805 * bl) / 0.95047, y = 0.2126 * r + 0.7152 * g + 0.0722 * bl, z = (0.0193 * r + 0.1192 * g + 0.9505 * bl) / 1.08883;
    return [116 * f(y) - 16, 500 * (f(x) - f(y)), 200 * (f(y) - f(z))];
  };
  const [p, q] = [lab(a), lab(b)];
  return Math.hypot(p[0] - q[0], p[1] - q[1], p[2] - q[2]);
}
