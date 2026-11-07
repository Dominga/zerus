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
const marks = "\0session-card-marks";
let server: ViteDevServer | undefined;
let render: (compact: boolean, session: Session, online?: boolean, team?: { total: number; working: number; collapsed: boolean; needsYou?: number; asking?: number; errors?: number }) => string;

beforeAll(async () => {
  const { fileURLToPath } = await import(/* @vite-ignore */ ["node", "url"].join(":")) as { fileURLToPath(url: URL): string };
  server = await createServer({
    configFile: false, root: fileURLToPath(new URL("../../../", import.meta.url)),
    server: { middlewareMode: true, watch: null }, logLevel: "silent",
    plugins: [{
      name: "session-card-render-stores", enforce: "pre",
      resolveId(id, importer) {
        if ([entry, stores, translations, marks].includes(id)) return id;
        if (importer?.endsWith("/SessionCard.tsx")) {
          if (["../../store/catalog", "../../store/ui", "../../store/settings"].includes(id)) return stores;
          if (id === "../../i18n") return translations;
          if (id === "../../store/accountMarks") return marks;
        }
      },
      load(id) {
        if (id === stores) return `export const state = {online: true};
          export const computerFresh = () => state.online, computerOffline = () => !state.online;
          export const now = () => 0, settings = {};`;
        if (id === marks) return `export const sessionMark = (s) => s.accountId === "claude-a1"
          ? {key: "work", label: "Work", letter: "W", emoji: "", glyph: "W", isEmoji: false, color: "#2b97ff", ink: "#10151b", provider: "claude", machine: "arch"}
          : undefined;`;
        if (id === translations) return `export const trNoop = s => s;
          export const tr = (s, ...args) => s.replace(/%(\\d+)/g, (_, n) => String(args[n - 1]));`;
        if (id === entry) return `import {createComponent} from 'solid-js';
          import {renderToString} from 'solid-js/web';
          import {SessionCard, CompactCard} from '/src/ui/sessions/SessionCard.tsx';
          import {state} from '${stores}';
          export const render = (compact, session, online = true, team = undefined) => {
            state.online = online;
            return renderToString(() => createComponent(compact ? CompactCard : SessionCard,
              {session, selected: false, expandable: false, expanded: false, onToggleChildren() {}, onExpand() {}, team}));
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

describe("supervised and muted children (owner 2026-11-08)", () => {
  const parent = { machine: "11111111-1111-4111-8111-111111111111", session_id: "22222222-2222-4222-8222-222222222222",
    name: "codex/project/lead", conversation_id: "c", run_id: "r", state: "live" };
  // The order of the arrow, the name and the crossed bell in the first line.
  const order = (html: string) => [...html.matchAll(/class="(sc-supervised|sc-title ell|sc-mute)[" ]/g)].map((m) => m[1].split(" ")[0]);
  it("shows the supervised arrow whether muted or not, and the bell right after the name", () => {
    for (const compact of [false, true]) {
      expect(order(render(compact, row({ parent, muted: true })))).toEqual(["sc-supervised", "sc-title", "sc-mute"]);
      expect(order(render(compact, row({ parent })))).toEqual(["sc-supervised", "sc-title"]);
      expect(order(render(compact, row({ muted: true })))).toEqual(["sc-title", "sc-mute"]);
      // A parent that is not live supervises nothing.
      expect(order(render(compact, row({ parent: { ...parent, state: "missing" }, muted: true })))).toEqual(["sc-title", "sc-mute"]);
    }
  });
});

describe("account marks", () => {
  it("marks the recorded account with its letter in both views and its label on full cards", () => {
    const compact = render(true, row({ accountId: "claude-a1" }));
    expect(compact, compact).toContain("acct-mark");
    expect(compact).toContain('aria-label="Account: Work"');
    expect(text(compact)).toContain("W");
    const full = render(false, row({ accountId: "claude-a1" }));
    expect(full).toContain('class="acct-chip"');
    expect(full).toContain("--mark:#2b97ff");
    expect(text(full)).toContain("Work");
  });

  it("shows no mark for a session without a resolved account", () => {
    for (const compact of [false, true]) expect(render(compact, row())).not.toContain("acct-mark");
  });
});

describe("compact parents (owner 2026-11-07)", () => {
  it("adds a second line with the child sessions summary only to a parent", () => {
    const team = { total: 8, working: 3, collapsed: true, needsYou: 1, asking: 0, errors: 2 };
    const html = render(true, row(), true, team);
    expect(html).toContain("cc-team");
    expect(text(html)).toContain("3 of 8 working");
    expect(text(html)).toContain("Needs you: 1");
    expect(text(html)).toContain("Errors: 2");
    expect(text(html)).not.toContain("Asking");
    expect(html).toContain('aria-expanded="false"');
    expect(render(true, row())).not.toContain("cc-team");
  });

  it("keeps the toggle and every count in words when the counters shorten (zerus-sa9qn.11)", () => {
    const html = render(true, row(), true, { total: 8, working: 3, collapsed: false, needsYou: 1, asking: 2, errors: 2 });
    const label = /class="cc-team[^"]*"[^>]*aria-label="([^"]*)"/.exec(html)?.[1] ?? /aria-label="([^"]*)"[^>]*class="cc-team/.exec(html)?.[1];
    expect(label).toBeDefined();
    for (const words of ["Hide subagents and child sessions", "3 of 8 working", "Needs you: 1", "Asking: 2", "Errors: 2"]) expect(label).toContain(words);
    expect(html).toContain('class="short" aria-hidden="true"');
    expect(html).toContain('class="cc-team-body"');
    expect(html.indexOf('class="chev"')).toBeGreaterThan(html.lastIndexOf('class="cc-team-body"'));
  });
});
