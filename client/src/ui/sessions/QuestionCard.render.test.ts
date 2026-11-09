import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createServer, type ViteDevServer } from "vite";
import solid from "vite-plugin-solid";

// Task 449 (owner, screenshot of package 167: "кнопки без оформления"): on the
// "Agent needs your answer" card the footer actions were bare text and the
// question tabs ran together. The card now draws its actions as the skin's
// buttons (Submit answers primary, Back and Open Terminal secondary/ghost) and
// its tabs as the skin's tabs, with the answered form's check mark beside its
// title instead of glued to it. The stores are the only isolated modules; the
// card, its two views and their stylesheet are production code.
const entry = "\0questioncard-render";
const STUBS: Record<string, string> = {
  "src/i18n": `export const tr = (s, ...args) => s.replace(/%(\\d+)/g, (_, n) => String(args[n - 1])); export const trNoop = (s) => s;`,
  "src/text": `export const trText = (t) => (typeof t === "string" ? t : (t && t.text) || "");`,
  "src/ui/lib/dates": `export const askedTimeOf = () => "15:15:48"; export const zonedTimeOf = () => "15:15:48";`,
  "src/ui/lib/icons": `export const Icon = (p) => "[" + p.name + "]";`,
  "src/store/questions": `export const questions = { view: () => null, enabled: () => true,
    label: (t) => (typeof t === "string" ? t : (t && t.label) || ""), page: async () => {}, skip: async () => {},
    submit: async () => {}, decide: async () => {}, always: async () => {}, allowRetry: async () => {}, edit: () => {},
    setActive: () => {}, enter: async () => {}, navigate: async () => {}, dialog: () => undefined,
    cancelCold: () => {}, answerCold: async () => {} };`,
  "src/store/terminal": `export const terminal = { texts: () => ({ available: true }), opened: () => {} };`,
  "src/store/ui": `export const setDetailTab = () => {}; export const selected = () => undefined;`,
  "src/ui/sessions/native-entry": `export const nativeBinding = () => true; export const nativeRoute = () => () => {};`,
  "src/ui/sessions/Activity": `export const Body = (p) => p.text;`,
};
const stubId = (key: string) => `\0questioncard-stub:${key}`;
let server: ViteDevServer | undefined;
let renderAction: (props: Record<string, unknown>) => string;
let renderTabs: (props: Record<string, unknown>) => string;

beforeAll(async () => {
  const { fileURLToPath } = await import(/* @vite-ignore */ ["node", "url"].join(":")) as { fileURLToPath(url: URL): string };
  server = await createServer({
    configFile: false, root: fileURLToPath(new URL("../../../", import.meta.url)),
    server: { middlewareMode: true, watch: null }, logLevel: "silent",
    plugins: [{
      name: "questioncard-render-mocks", enforce: "pre",
      async resolveId(id, importer) {
        if (id === entry || id.startsWith("\0questioncard-stub:")) return id;
        if (!importer || id.startsWith("\0") || !id.startsWith(".")) return undefined;
        const resolved = await this.resolve(id, importer, { skipSelf: true });
        const path = resolved?.id.replace(/\\/g, "/").replace(/\.(tsx?|jsx?)$/, "") ?? "";
        const key = Object.keys(STUBS).find((stub) => path.endsWith(`/${stub}`));
        return key ? stubId(key) : undefined;
      },
      load(id) {
        if (id.startsWith("\0questioncard-stub:")) return STUBS[id.slice("\0questioncard-stub:".length)];
        if (id === entry) return `import {createComponent} from 'solid-js';
          import {renderToString} from 'solid-js/web';
          import {QuestionAction, QuestionTabs} from '/src/ui/sessions/QuestionCard.tsx';
          export const renderAction = (props) => renderToString(() => createComponent(QuestionAction, props));
          export const renderTabs = (props) => renderToString(() => createComponent(QuestionTabs, props));`;
        return undefined;
      },
    }, solid({ ssr: true })],
  });
  const loaded = await server.ssrLoadModule(entry);
  renderAction = loaded.renderAction;
  renderTabs = loaded.renderTabs;
});

afterAll(async () => { await server?.close(); });

/** The rendered HTML without Solid's hydration marks, with each class attribute
 *  in its plain form (classList appends its own names, with a spare space). */
const plain = (html: string) => html
  .replace(/<!--[^>]*-->/g, "")
  .replace(/\sdata-hk="[^"]*"/g, "")
  .replace(/class="([^"]*)"/g, (_match, value: string) => `class="${value.trim().split(/\s+/).join(" ")}"`);
const noop = () => {};
/** The `<button>` that carries the action's label, whole. */
const actionOf = (html: string, label: string) => html.match(new RegExp(`<button[^>]*>${label.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}</button>`))?.[0] ?? "";

describe("the question card's footer actions (task 449)", () => {
  it("draws Open Terminal and Back as the skin's secondary buttons and Submit answers as the primary one", () => {
    const html = plain(renderAction({ label: "Open Terminal", kind: "ghost", tip: "Terminal is busy", onClick: noop })
      + renderAction({ label: "← Back", kind: "ghost", disabled: true, onClick: noop })
      + renderAction({ label: "Submit answers", kind: "primary", onClick: noop }));
    const open = actionOf(html, "Open Terminal");
    const back = actionOf(html, "← Back");
    const submit = actionOf(html, "Submit answers");
    // Every footer action is a button of the skin, never a bare label.
    for (const button of [open, back, submit]) {
      expect(button).toMatch(/^<button[^>]*class="btn small /);
      expect(button).toMatch(/type="button"/);
    }
    expect(open).toContain('class="btn small ghost"');
    expect(open).toContain('title="Terminal is busy"');
    expect(back).toContain('class="btn small ghost"');
    expect(back).toMatch(/\sdisabled/);
    // The card's own decision is its primary button: accent, and a plain button
    // carries no accent class of its own.
    expect(submit).toContain('class="btn small primary"');
    expect(submit).not.toContain("ghost");
  });

  it("keeps the accent step ('Next') a ghost button, not the primary one", () => {
    const html = plain(renderAction({ label: "Next", kind: "accent", onClick: noop }));
    expect(html).toContain('class="btn small ghost accent"');
    expect(html).not.toContain("primary");
  });
});

describe("the question card's tabs (task 449)", () => {
  it("draws each tab as the skin's tab and keeps a filled form's check mark apart from its title", () => {
    const html = plain(renderTabs({ tabs: ["✓ 1: Overlay MRAE", "2: Тайл явно"], page: 1, onChoose: noop }));
    expect(html).toContain('class="qcard-tabs tabs" role="tablist"');
    const tabs = [...html.matchAll(/<button[^>]*class="tab[^"]*"[^>]*>[\s\S]*?<\/button>/g)].map((match) => match[0]);
    expect(tabs).toHaveLength(2);
    for (const tab of tabs) {
      expect(tab).toMatch(/^<button[^>]*class="tab[^"]*"/);
      expect(tab).toMatch(/role="tab"/);
      // The title is its own span, so a narrow card shortens it with an ellipsis.
      expect(tab).toContain('<span class="tab-label">');
    }
    // The answered form's mark stands in its own span, beside the title.
    expect(tabs[0]).toContain('<span class="tab-check" aria-hidden="true">✓</span>');
    expect(tabs[0]).toContain('<span class="tab-label">1: Overlay MRAE</span>');
    // A form with no mark keeps only the label, and the page's own tab is the on one.
    expect(tabs[1]).not.toContain("tab-check");
    expect(tabs[1]).toContain('<span class="tab-label">2: Тайл явно</span>');
    expect(tabs[0]).toMatch(/class="tab(?! on)"/);
    expect(tabs[1]).toMatch(/class="tab on"/);
    expect(tabs[1]).toContain('aria-selected="true"');
  });

  it("carries the skin's tab spacing and the button skin in the card's stylesheet", async () => {
    const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as { readFileSync(path: URL, encoding: "utf8"): string };
    const text = fs.readFileSync(new URL("client/src/ui/styles/detail.css", new URL("../../../../", import.meta.url)), "utf8");
    // The tabs: a gap between them, the skin's padding and height on each, the
    // check mark's own colour and the underline inside the active tab.
    expect(text).toMatch(/\.qcard-tabs \{[^}]*gap: 6px;/);
    expect(text).toMatch(/\.qcard-tabs \.tab \{[^}]*padding: 0 10px;/);
    expect(text).toMatch(/\.qcard-tabs \.tab \{[^}]*height: 30px;/);
    expect(text).toMatch(/\.qcard-tabs \.tab-check \{[^}]*color: var\(--z-color-accent\);/);
    expect(text).toMatch(/\.qcard-tabs \.tab\.on::after \{[^}]*left: 8px; right: 8px;/);
    // The footer: a real gap, the button's own height and border, the primary
    // button in the accent, and the standard focus ring and disabled state.
    expect(text).toMatch(/\.qcard-actions \{[^}]*gap: 8px;/);
    expect(text).toMatch(/\.qcard-actions > \.btn \{[^}]*height: calc\(var\(--control-h\) - 6px\);/);
    expect(text).toMatch(/\.qcard-actions > \.btn \{[^}]*border: 1px solid var\(--z-color-border\);/);
    expect(text).toMatch(/\.qcard-actions > \.btn\.primary \{[^}]*background: var\(--z-color-accent\);/);
    expect(text).toMatch(/\.qcard-actions > \.btn:disabled \{[^}]*opacity: \.5;/);
    expect(text).toMatch(/\.qcard-actions > \.btn:focus-visible \{[^}]*outline: 2px solid var\(--z-color-focus\);/);
  });
});
