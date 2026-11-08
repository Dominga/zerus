import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createServer, type ViteDevServer } from "vite";
import solid from "vite-plugin-solid";

// Task #328 (F10b): the footer Qt draws above the status bar, rendered as the
// Tauri client draws it; no draft has a disabled Create team and says why.
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
  const { NewTeamFooter } = await server.ssrLoadModule("/src/ui/teams/NewTeamFooter.tsx");
  render = (props) => renderToString(() => createComponent(NewTeamFooter, props));
});

afterAll(async () => { await server?.close(); });

describe("New team footer (task #328)", () => {
  it("offers a disabled Create team and Start planning first before a draft", () => {
    const html = render({ draft: false, dirty: false, revision: 0, busy: false, createWhy: "Start planning first", onCreate: () => {} });
    const button = html.match(/<button[^>]*>Create team<\/button>/)?.[0] ?? "";
    expect(button).toContain("Create team");
    expect(button).toContain("disabled");
    expect(html).toContain("Start planning first");
    expect(html).toContain('title="Start planning first"');
  });
  it("enables Create team for a valid draft", () => {
    const html = render({ draft: true, dirty: false, revision: 2, busy: false, preflightErrors: 0, createWhy: "", onCreate: () => {} });
    expect(html).toContain("Draft saved, revision 2");
    expect(html).toContain("Preflight: ok");
    const button = html.match(/<button[^>]*>Create team<\/button>/)?.[0] ?? "";
    expect(button).not.toContain("disabled");
  });
});
