import { describe, expect, it } from "vitest";
import { LOGO_FILES, logoOf, logoSource, nextLogoId, parseLogo, uniqueIds } from "./agent-logos";
import { agentKind } from "../../store/catalog";
import type { AgentKind } from "../sessions/types";

// The provider logos are Qt's: the same files, the same agent-to-file map
// (tray/src/WorkspaceIcons.h agentLogoIcon). Read from disk, as icons.test.ts does.
const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as {
  readFileSync(path: URL, encoding: "utf8"): string;
  existsSync(path: URL): boolean;
};
const text = (relative: string) => fs.readFileSync(new URL(relative, import.meta.url), "utf8").replace(/\r\n/g, "\n");
const KINDS: AgentKind[] = ["claude", "codex", "kimi", "deepseek", "opencode", "ollama", "sh"];

describe("agent logos", () => {
  it("map every agent to the file Qt uses: dsh and deepseek to DeepSeek, kimi to its colored logo, ollama included", () => {
    expect(LOGO_FILES).toEqual({
      claude: "claude", codex: "codex", dsh: "deepseek", deepseek: "deepseek", kimi: "kimi-color", opencode: "opencode", ollama: "ollama",
    });
    expect(agentKind("dsh")).toBe("deepseek");
    expect(logoSource("dsh")).toBe(logoSource("deepseek"));
    expect(logoSource("kimi")).toContain("#1783FF");
    expect(logoSource("ollama")).toContain("Ollama");
  });

  // Task #28 (team ede724c4): the Qt copies (tray/resources/agent-icons and
  // WorkspaceIcons.h) are gone; these files and the table above are the source.
  it("ships each mapped SVG file and the MIT notice", () => {
    for (const file of new Set(Object.values(LOGO_FILES))) {
      expect(text(`../../assets/agent-icons/${file}.svg`), file).toContain("<svg");
    }
    const license = text("../../assets/agent-icons/LICENSE");
    expect(license).toContain("MIT License");
    expect(license).toContain("LobeHub");
    expect(text("../../../../THIRD_PARTY_NOTICES.md")).toContain("client/src/assets/agent-icons");
  });

  it("keeps each logo's own colors and lets the monochrome parts follow the theme", () => {
    for (const kind of KINDS.filter((k) => k !== "sh")) {
      const logo = logoOf(kind)!;
      expect(logo, kind).toBeDefined();
      expect(logo.inner, kind).not.toContain("<title>");
      expect(logo.inner, kind).toContain("<path");
      // A path without a fill of its own is painted by the root's: currentColor, the text color.
      const bare = [...logo.inner.matchAll(/<path\b[^>]*>/g)].filter((m) => !/\bfill="/.test(m[0]));
      if (bare.length) expect(logo.fill, kind).toBe("currentColor");
    }
    expect(logoOf("claude")!.inner).toContain('fill="#D97757"');
    expect(logoOf("deepseek")!.inner).toContain('fill="#4D6BFE"');
    expect(logoOf("kimi")!.inner).toContain('fill="currentColor"');
    for (const mono of ["opencode", "ollama"]) expect(logoOf(mono)!.fill).toBe("currentColor");
    expect(logoOf("ollama")!.rule).toBe("evenodd");
  });

  it("falls back to the stroke glyph for an agent without a logo, an unknown one and an unreadable file", () => {
    expect(logoOf("sh")).toBeUndefined();
    expect(logoOf(undefined)).toBeUndefined();
    expect(logoOf("some-future-agent")).toBeUndefined();
    expect(parseLogo(undefined)).toBeUndefined();
    expect(parseLogo("not an svg")).toBeUndefined();
    // Every presentation kind has either a logo or is the shell: the glyph tables cover the rest.
    expect(KINDS.filter((k) => !logoOf(k))).toEqual(["sh"]);
    expect(agentKind("ollama")).toBe("ollama");
    expect(agentKind("something else")).toBe("claude");
  });

  it("gives every instance its own gradient ids, so a hidden first copy cannot break the next", () => {
    const inner = logoOf("codex")!.inner;
    const id = /\bid="([^"]+)"/.exec(inner)![1];
    const a = uniqueIds(inner, "l1");
    const b = uniqueIds(inner, "l2");
    expect(a).toContain(`id="${id}-l1"`);
    expect(a).toContain(`url(#${id}-l1)`);
    expect(a).not.toContain(`url(#${id})`);
    expect(b).toContain(`url(#${id}-l2)`);
    expect(uniqueIds(logoOf("claude")!.inner, "l1")).toBe(logoOf("claude")!.inner);
    expect(nextLogoId()).not.toBe(nextLogoId());
  });
});
