import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createServer, type ViteDevServer } from "vite";
import solid from "vite-plugin-solid";

// Task #337: the working-mode suggestion as one compact row: the project's
// reasons in its tooltip, and Keep / Propose / close beside them.
let server: ViteDevServer | undefined;
let render: (props: Record<string, unknown>) => string;

beforeAll(async () => {
  const { fileURLToPath } = await import(/* @vite-ignore */ ["node", "url"].join(":")) as { fileURLToPath(url: URL): string };
  server = await createServer({
    configFile: false, root: fileURLToPath(new URL("../../../", import.meta.url)),
    server: { middlewareMode: true, watch: null }, logLevel: "silent",
    plugins: [solid({ ssr: true })],
  });
  const { renderToString } = await server.ssrLoadModule("solid-js/web");
  const { createComponent } = await server.ssrLoadModule("solid-js");
  const { WorkingModeRow } = await server.ssrLoadModule("/src/ui/teams/WorkingModeRow.tsx");
  render = (props) => renderToString(() => createComponent(WorkingModeRow, props));
});

afterAll(async () => { await server?.close(); });

const row = () => render({
  preset: "branch-pipeline", title: "Branch pipeline",
  reasons: ["A code repository with tests", "12 source files of 14 tracked"],
  disagreeing: ["folder policy", "branch per task"],
  onKeep: () => {}, onPropose: () => {}, onClose: () => {},
});

describe("the working-mode row (task #337)", () => {
  it("keeps the team's settings, proposes the switch and closes, in one row", () => {
    const html = row();
    expect(html).toContain('class="team-working-mode"');
    expect(html).toContain('data-working-mode="branch-pipeline"');
    expect(html).toContain("Working mode: the project suggests Branch pipeline");
    expect(html).toContain("settings differ: folder policy, branch per task");
    // Keep records the choice; Propose asks for the switch; the x closes.
    expect(html).toContain("data-working-mode-keep");
    expect(html).toContain("Keep this team");
    expect(html).toContain("data-working-mode-switch");
    expect(html).toContain("Propose the switch");
    expect(html).toContain("data-working-mode-close");
    expect(html).toContain('aria-label="Close"');
  });

  it("puts the long reasons in the row's tooltip, never in a tall list", async () => {
    const html = row();
    // The title attribute carries the proposal's reasons.
    const title = html.match(/title="([^"]*)"/)?.[1] ?? "";
    expect(title).toContain("A code repository with tests");
    expect(title).toContain("12 source files of 14 tracked");
    expect(html).not.toContain("<ul>");
    // Compact: one flex row, not a column panel (styles/newteam.css).
    const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as { readFileSync(path: URL, encoding: "utf8"): string };
    const text = fs.readFileSync(new URL("client/src/ui/styles/newteam.css", new URL("../../../../", import.meta.url)), "utf8");
    const rule = text.split("\n").find((line) => line.trim().startsWith(".team-working-mode {")) ?? "";
    expect(rule).toMatch(/display: flex;/);
    expect(rule).toMatch(/align-items: center;/);
    expect(rule).not.toMatch(/flex-direction: column;/);
  });
});
