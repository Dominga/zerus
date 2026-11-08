import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createServer, type ViteDevServer } from "vite";
import solid from "vite-plugin-solid";
import type { Session } from "./types";

// Render the actual JSX in Node without adding a DOM dependency. Only the
// stateful application stores and English translation are isolated; the card,
// status/chip selectors and icons are production modules.
const entry = "\0session-card-render";
const stores = "\0session-card-stores";
const translations = "\0session-card-i18n";
let server: ViteDevServer | undefined;
let render: (compact: boolean, session: Session, online?: boolean, extra?: Record<string, unknown>) => string;

beforeAll(async () => {
  const { fileURLToPath } = await import(/* @vite-ignore */ ["node", "url"].join(":")) as { fileURLToPath(url: URL): string };
  server = await createServer({
    configFile: false, root: fileURLToPath(new URL("../../../", import.meta.url)),
    server: { middlewareMode: true, watch: null }, logLevel: "silent",
    plugins: [{
      name: "session-card-render-stores", enforce: "pre",
      resolveId(id, importer) {
        if ([entry, stores, translations].includes(id)) return id;
        if (importer?.endsWith("/SessionCard.tsx")) {
          if (["../../store/catalog", "../../store/ui", "../../store/settings"].includes(id)) return stores;
          if (id === "../../i18n") return translations;
        }
      },
      load(id) {
        if (id === stores) return `export const state = {online: true};
          export const computerFresh = () => state.online, computerOffline = () => !state.online;
          export const now = () => 0, settings = {};`;
        if (id === translations) return `export const trNoop = s => s;
          export const tr = (s, ...args) => s.replace(/%(\\d+)/g, (_, n) => String(args[n - 1]));`;
        if (id === entry) return `import {createComponent} from 'solid-js';
          import {renderToString} from 'solid-js/web';
          import {SessionCard, CompactCard} from '/src/ui/sessions/SessionCard.tsx';
          import {state} from '${stores}';
          export const render = (compact, session, online = true, extra = {}) => {
            state.online = online;
            return renderToString(() => createComponent(compact ? CompactCard : SessionCard,
              {session, selected: false, expandable: false, expanded: false, onToggleChildren() {}, onExpand() {}, ...extra}));
          };`;
      },
    }, solid({ ssr: true })],
  });
  render = (await server.ssrLoadModule(entry)).render;
});

afterAll(async () => { await server?.close(); });

const row = (extra: Partial<Session> = {}): Session => ({
  key: "host\nrun:fixture", catalogKey: "run:fixture", computer: "host", name: "codex/project/reviewer",
  title: "reviewer", machine: "host", rawMachine: "host", project: "project", agent: "codex", model: "",
  cwd: "/fixture/project", status: "ready", caption: "Ready", summary: "", state: "running",
  processState: "alive", activity: "idle", tracked: true, resumable: true, cmd: "codex", unread: false, draft: false,
  sharedRead: false, cwdPath: "/fixture/project", cwdCanonical: "/fixture/project", gitRoot: "",
  terminalSupported: false, terminalReported: false, terminalAttachSupported: false, terminalAttachReported: false,
  bulkLifecycle: true, created: 0, fingerprint: "", needsAction: false, ackStatus: "ready", ackCaption: "Ready",
  baseChildAttention: false, baseNotificationError: false, baseStatus: "ready", baseCaption: "Ready",
  baseUnread: false, baseSummary: "", baseMuted: false, sharedNotifications: false,
  ...extra,
});
const chip = (html: string) => html.match(/<span\b[^>]*\bclass="qchip[^"]*"[^>]*>[\s\S]*?<\/span>/)?.[0];
const text = (html: string) => html.replace(/<[^>]*>/g, "");

describe("full and compact session question presentation", () => {
  it("shows a pending optional Codex question in compact as well as full view without making Ready blocking", () => {
    for (const compact of [false, true]) {
      const html = render(compact, row({ optionalQuestions: 1 }));
      const question = chip(html);
      expect(question, compact ? "compact question chip" : "full question chip").toBeDefined();
      expect(text(question!)).toBe("1");
      expect(question).toContain("1 optional question waiting");
      expect(question).toContain("The agent can continue without an answer.");
      expect(question).toContain("<svg");
      expect(html).toContain("sbadge k-neutral");
      expect(html).toContain("Ready");
      expect(html).not.toContain("Needs input");
    }
  });

  it("keeps a blocking question at Needs input with an attention badge in both views", () => {
    for (const compact of [false, true]) {
      const html = render(compact, row({ status: "attention", caption: "Needs input", phase: "input", attention: "question" }));
      expect(html).toContain("sbadge k-attention");
      expect(html).toContain("Needs input");
      expect(chip(html)).toBeUndefined();
    }
  });

  it.each([
    ["offline", {}, false], ["saved", { state: "paused" }, true], ["stopped", { state: "stopped" }, true],
    ["archived", { state: "archived" }, true], ["exited", { processState: "exited" }, true],
    ["unknown count", { optionalQuestions: undefined }, true], ["answered", { optionalQuestions: 0 }, true],
  ] as const)("hides stale or absent optional questions: %s", (_name, extra, online) => {
    for (const compact of [false, true]) expect(chip(render(compact, row({ optionalQuestions: 3, ...extra }), online))).toBeUndefined();
  });

  it("preserves muted styling and the bounded count in both views", () => {
    for (const compact of [false, true]) {
      const question = chip(render(compact, row({ optionalQuestions: 1000, muted: true })))!;
      expect(question).toBeDefined();
      expect(text(question)).toBe("99+");
      expect(question).toContain("qchip muted");
      expect(question).toContain("1000 optional questions waiting");
    }
  });
});

describe("a team member's row (task 330)", () => {
  it("keeps a team member's box at the pane edge and its row connector", async () => {
    const { readFileSync } = await import(/* @vite-ignore */ ["node", "fs"].join(":")) as { readFileSync(path: URL): string };
    const stylesheet = String(readFileSync(new URL("../../ui/styles/sessions.css", import.meta.url)));
    const rule = (selector: string) => {
      const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      return stylesheet.replace(/\r\n/g, "\n").match(new RegExp(`${escaped} \\{([^}]*)\\}`))?.[1] ?? "";
    };
    const html = render(false, row(), true, { depth: 1, teamFirst: true });
    expect(html).toContain("team-member");
    expect(html).toMatch(/style="[^"]*left:\s*2px/);
    expect(html).not.toMatch(/left:\s*24px/);

    // Parse the stylesheet, never a literal run of its text: the box spans the
    // row (and so does the hover), its content stays indented, and the vertical
    // connector runs from the row above to the last member's centre.
    const member = rule(".scard.team-member");
    expect(stylesheet).toContain(".scard.team-member");
    expect(member).toMatch(/left:\s*2px/);
    expect(member).toMatch(/right:\s*2px/);
    expect(member).toMatch(/padding-left:\s*22px/);
    const before = rule(".scard.team-member::before");
    expect(before).toMatch(/left:\s*12px/);
    expect(before).toMatch(/top:\s*-8px/);
    expect(before).toMatch(/bottom:\s*-6px/);
    expect(rule(".scard.team-member.team-last::before")).toMatch(/bottom:\s*50%/);
    expect(rule(".scard.team-member::after")).toMatch(/top:\s*50%/);
  });
});
describe("session cards at the narrow side panel (task 343)", () => {
  it("keeps the lead and member parts at 480 and 360 px", async () => {
    const { readFileSync } = await import(/* @vite-ignore */ ["node", "fs"].join(":")) as { readFileSync(path: URL): string };
    const css = String(readFileSync(new URL("../../ui/styles/sessions.css", import.meta.url))).replace(/\r\n/g, "\n");
    const block = (selector: string) => {
      const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      return [...css.matchAll(new RegExp(`(?:^|\\n)\\s*${escaped}\\s*\\{([^}]*)\\}`, "gm"))].at(-1)?.[1] ?? "";
    };
    expect(block(".scard")).toContain("container-type: inline-size");
    expect(block(".sc-cold")).toContain("flex: none");
    expect(block(".sc-l3 > .ell")).toContain("flex: 1 1 auto");
    const container = (head: string) => { const at = css.indexOf(head); return at < 0 ? "" : css.slice(at, at + 900); };
    const narrow = container("@container (max-width: 430px)");
    expect(narrow).toContain(".agent-badge:not(.narrow)");
    expect(narrow).toContain("width: 26px");
    expect(narrow).toContain(".ab-label");
    expect(narrow).toContain(".mbadge");
    expect(container("@container (max-width: 360px)")).toContain(".sc-cold button");

    const member = render(false, row(), true, { depth: 1, teamFirst: true });
    expect(member).toContain("team-member");
    expect(member).toMatch(/style="[^"]*left:\s*2px/);
    const lead = render(false, row(), true, { team: { name: "T", status: "running", phase: 1, phase_done: 1, phase_total: 2,
      phases_done: 1, phases_total: 2, attention: 0, members: [] } });
    expect(lead).toContain("team-strip");
  });
});
