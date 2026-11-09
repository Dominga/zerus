import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createServer, type ViteDevServer } from "vite";
import solid from "vite-plugin-solid";
import ts from "typescript";
import composerSource from "./Composer.tsx?raw";

// Task 422 (owner, 2026-11-09): the 40x4 height grip sat in the composer's
// toolbar row, between "Mark as read" and the usage chips. It is gone from that
// row; the top border is the only drag area and draws a thin grip on the border
// line, shown on hover, focus and drag. The render half uses the real handle
// (the whole store graph of Composer.tsx is stubbed); the shape half reads the
// composer's JSX and the stylesheet, which the node run cannot lay out.
const entry = "\0composer-resize";
const STUB = "\0composer-stub:";

/** The names Composer.tsx imports from each module, generated as inert stubs. */
const stubNames = (() => {
  const map = new Map<string, string[]>();
  for (const match of composerSource.matchAll(/import\s*\{([^}]*)\}\s*from\s*"([^"]+)"/g)) {
    const specifier = match[2];
    if (specifier === "solid-js" || specifier === "solid-js/web") continue;
    const names = match[1].split(",").map((part) => part.trim().replace(/^type\s+/, "").split(/\s+as\s+/)[0]).filter(Boolean);
    map.set(specifier, [...(map.get(specifier) ?? []), ...names]);
  }
  return map;
})();

let server: ViteDevServer | undefined;
let render: () => string;
/** The stylesheet text, read through the same Vite server (?raw). */
let css: string;

beforeAll(async () => {
  const { fileURLToPath } = await import(/* @vite-ignore */ ["node", "url"].join(":")) as { fileURLToPath(url: URL): string };
  server = await createServer({
    configFile: false, root: fileURLToPath(new URL("../../../", import.meta.url)),
    server: { middlewareMode: true, watch: null }, logLevel: "silent",
    plugins: [{
      name: "composer-resize-stubs", enforce: "pre",
      resolveId(id, importer) {
        if (id === entry || id.startsWith(STUB)) return id;
        if (!importer || !stubNames.has(id)) return undefined;
        return importer.replace(/\\/g, "/").endsWith("/Composer.tsx") ? `${STUB}${id}` : undefined;
      },
      load(id) {
        if (id === entry) return `import { ResizeHandle } from "/src/ui/sessions/Composer.tsx";
          import styles from "/src/ui/styles/detail.css?raw";
          import { renderToString } from "solid-js/web";
          export const css = styles;
          export const render = () => renderToString(() => ResizeHandle({}));`;
        if (id.startsWith(STUB)) {
          const names = stubNames.get(id.slice(STUB.length)) ?? [];
          return names.map((name) => `export const ${name} = ${/^[A-Z]/.test(name) ? "(props) => null" : "() => undefined"};`).join("\n");
        }
        return undefined;
      },
    }, solid({ ssr: true })],
  });
  render = (await server.ssrLoadModule(entry)).render;
  css = (await server.ssrLoadModule(entry)).css;
});

afterAll(async () => { await server?.close(); });

/** Every JSX element in the composer, so the toolbar's own tree can be read. */
function jsxElements(): ts.Node[] {
  const tree = ts.createSourceFile("Composer.tsx", composerSource, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const found: ts.Node[] = [];
  const visit = (node: ts.Node) => {
    if (ts.isJsxElement(node) || ts.isJsxSelfClosingElement(node)) found.push(node);
    ts.forEachChild(node, visit);
  };
  visit(tree);
  return found;
}
const tag = (node: ts.Node) => (ts.isJsxElement(node) ? node.openingElement : node as ts.JsxSelfClosingElement).tagName.getText();
const className = (node: ts.Node) => {
  const attributes = (ts.isJsxElement(node) ? node.openingElement : node as ts.JsxSelfClosingElement).attributes.properties;
  const attribute = attributes.find((a) => ts.isJsxAttribute(a) && a.name.getText() === "class");
  return attribute && ts.isJsxAttribute(attribute) && attribute.initializer && ts.isStringLiteral(attribute.initializer) ? attribute.initializer.text : "";
};
const within = (outer: ts.Node, inner: ts.Node) => inner.getStart() >= outer.getStart() && inner.getEnd() <= outer.getEnd();

describe("the composer's height grip (task 422)", () => {
  it("draws its grip on the top edge, which stays keyboard reachable", () => {
    const html = render();
    expect(html).toMatch(/class="cmp-resize edge\b/);
    expect(html).toContain('role="separator"');
    expect(html).toContain('aria-orientation="horizontal"');
    expect(html).toContain('tabindex="0"');
    expect(html).toContain('<i class="cmp-grip"></i>');
    // One handle, and it is the edge: no toolbar gap variant.
    expect(html).toContain("cmp-resize");
    expect(html).not.toMatch(/cmp-resize[^"]*gap/);
  });

  it("keeps the grip out of the toolbar row", () => {
    const elements = jsxElements();
    const toolbar = elements.find((node) => className(node) === "cmp-toolbar");
    expect(toolbar).toBeDefined();
    const inside = elements.filter((node) => within(toolbar!, node) && node !== toolbar);
    expect(inside.some((node) => tag(node) === "ResizeHandle")).toBe(false);
    expect(inside.some((node) => /cmp-grip|cmp-resize/.test(className(node)))).toBe(false);
    // The row keeps its spacer, so the trailing chips stay right.
    expect(inside.some((node) => className(node) === "grow")).toBe(true);
    // The handle the composer renders is the top edge, outside the row.
    const handles = elements.filter((node) => tag(node) === "ResizeHandle" && !within(toolbar!, node));
    expect(handles).toHaveLength(1);
    expect(handles[0].getText()).toContain("<ResizeHandle");
  });

  it("hides the grip until hover, focus or drag and never holds a toolbar gap", () => {
    expect(css).not.toContain(".cmp-resize.gap");
    expect(css).toMatch(/\.cmp-grip \{[^}]*height: 3px;[^}]*opacity: 0;/);
    expect(css).toMatch(/\.cmp-grip \{[^}]*background: var\(--z-color-accent\);/);
    expect(css).toMatch(/\.cmp-resize\.edge:hover \.cmp-grip, \.cmp-resize\.edge:focus-visible \.cmp-grip, \.cmp-resize\.dragging \.cmp-grip \{[^}]*opacity: 1;/);
    // The strip sits on the top border line, centred.
    expect(css).toMatch(/\.cmp-resize\.edge \{[^}]*top: -3px;[^}]*height: 6px;[^}]*place-items: center;/);
  });
});
