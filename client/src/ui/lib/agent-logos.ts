// The providers' own logos (LobeHub lobe-icons, MIT; src/assets/agent-icons),
// the same files and the same agent-to-file map as the Qt GUI's agentLogoIcon
// (tray/src/WorkspaceIcons.h). Inlined, so `currentColor` follows the theme:
// each logo keeps the colors it has, and its monochrome parts take the text
// color. An agent without a logo keeps the stroke glyph (icons.tsx).

const files = import.meta.glob("../../assets/agent-icons/*.svg", { query: "?raw", import: "default", eager: true }) as Record<string, string>;

/** The agent's key, as the listing or the presentation kind carries it, to its file. */
export const LOGO_FILES: Readonly<Record<string, string>> = {
  claude: "claude",
  codex: "codex",
  dsh: "deepseek",
  deepseek: "deepseek",
  kimi: "kimi-color",
  opencode: "opencode",
  ollama: "ollama",
};

export interface Logo {
  /** The svg's content without its <title>, ids still as the file has them. */
  inner: string;
  /** The root's own fill, "currentColor" for the monochrome logos. */
  fill: string;
  rule?: "nonzero" | "evenodd";
}

const parsed = new Map<string, Logo | undefined>();

/** The logo file's text of an agent, or undefined when it has none. */
export function logoSource(agent: string | undefined): string | undefined {
  const file = LOGO_FILES[(agent ?? "").toLowerCase()];
  return file ? files[`../../assets/agent-icons/${file}.svg`] : undefined;
}

/** Split one logo file into what an inline <svg> needs; undefined for no logo or an unreadable file. */
export function parseLogo(source: string | undefined): Logo | undefined {
  const root = /^\s*<svg\b([^>]*)>([\s\S]*)<\/svg>\s*$/.exec(source ?? "");
  if (!root) return undefined;
  const attr = (name: string) => new RegExp(`(?:^|\\s)${name}="([^"]*)"`).exec(root[1])?.[1];
  return { inner: root[2].replace(/<title>[\s\S]*?<\/title>/, ""), fill: attr("fill") ?? "currentColor", rule: attr("fill-rule") === "evenodd" ? "evenodd" : undefined };
}

export function logoOf(agent: string | undefined): Logo | undefined {
  const key = (agent ?? "").toLowerCase();
  if (!parsed.has(key)) parsed.set(key, parseLogo(logoSource(key)));
  return parsed.get(key);
}

/**
 * The logo's markup with its ids made unique to one instance. Two inline svgs
 * with one gradient id break when the first is hidden (display:none), so every
 * instance owns its ids.
 */
export function uniqueIds(inner: string, uid: string): string {
  let out = inner;
  for (const [, id] of inner.matchAll(/\bid="([^"]+)"/g)) {
    out = out.split(`id="${id}"`).join(`id="${id}-${uid}"`).split(`#${id}`).join(`#${id}-${uid}`);
  }
  return out;
}

let sequence = 0;
export const nextLogoId = () => `l${++sequence}`;
