import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createServer, type ViteDevServer } from "vite";
import solid from "vite-plugin-solid";
import type { Session, Subagent } from "./types";

// Render the actual JSX in Node without adding a DOM dependency. Only the
// stateful application stores and English translation are isolated; the card,
// status/chip selectors and icons are production modules.
const entry = "\0session-card-render";
const stores = "\0session-card-stores";
const translations = "\0session-card-i18n";
const marks = "\0session-card-marks";
let server: ViteDevServer | undefined;
let render: (compact: boolean, session: Session, online?: boolean, team?: { total: number; working: number; collapsed: boolean; needsYou?: number; asking?: number; errors?: number }, extra?: Record<string, unknown>) => string;
let renderChild: (parent: Session, child: Subagent) => string;

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
          import {SessionCard, CompactCard, ChildCard} from '/src/ui/sessions/SessionCard.tsx';
          import {state} from '${stores}';
          export const render = (compact, session, online = true, team = undefined, extra = {}) => {
            state.online = online;
            return renderToString(() => createComponent(compact ? CompactCard : SessionCard,
              {session, selected: false, expandable: false, expanded: false, onToggleChildren() {}, onExpand() {}, team, ...extra}));
          };
          export const renderChild = (parent, child) => renderToString(() => createComponent(ChildCard,
            {parent, child, first: true, last: true, selected: false}));`;
      },
    }, solid({ ssr: true })],
  });
  ({ render, renderChild } = await server.ssrLoadModule(entry) as { render: typeof render; renderChild: typeof renderChild });
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

describe("the compact row's tooltip (Qt SessionCardDelegate.h:125-138, SessionList.cpp:189-196)", () => {
  const attribute = (html: string, cls: string, name: string) =>
    html.match(new RegExp(`<[^>]*class="${cls}[^"]*"[^>]*>`))?.[0].match(new RegExp(`\\b${name}="([^"]*)"`))?.[1]?.replace(/&#10;|&#x0?a;/gi, "\n");
  it("names the title, the session, its status and the provider the row draws as an icon", () => {
    const html = render(true, row({ caption: "Working" }));
    expect(attribute(html, "sc-title", "title")).toBe("reviewer\ncodex/project/reviewer\nWorking\nCodex");
    expect(attribute(html, "cc-expand", "title")).toBe("Expand session row\nreviewer\ncodex/project/reviewer\nWorking\nCodex");
    expect(attribute(html, "cc-expand", "aria-description")).toBe("reviewer\ncodex/project/reviewer\nWorking\nCodex");
  });
  it("gives a name once when the title is the name", () => {
    const html = render(true, row({ title: "codex/project/reviewer", caption: "Ready" }));
    expect(attribute(html, "sc-title", "title")).toBe("codex/project/reviewer\nReady\nCodex");
  });
});

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

describe("a team member's row and the team tree (tasks 330, 425)", () => {
  const styles = async () => {
    const { readFileSync } = await import(/* @vite-ignore */ ["node", "fs"].join(":")) as { readFileSync(path: URL): string };
    const read = (name: string) => String(readFileSync(new URL(`../../ui/styles/${name}.css`, import.meta.url))).replace(/\r\n/g, "\n");
    const stylesheet = read("sessions");
    // Parse the stylesheet, never a literal run of its text.
    const rule = (selector: string) => {
      const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      return stylesheet.match(new RegExp(`(?:^|\\n)\\s*${escaped} \\{([^}]*)\\}`))?.[1] ?? "";
    };
    // The declaration that wins: an unlayered rule outranks every layered one in
    // either layer order, and the member rules are restated unlayered at the end.
    const effectiveRule = (selector: string) => {
      const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      return [...stylesheet.matchAll(new RegExp(`(?:^|\\n)\\s*${escaped} \\{([^}]*)\\}`, "g"))].at(-1)?.[1] ?? "";
    };
    return { read, rule, effectiveRule };
  };
  const px = (text: string, property: string) => Number(text.match(new RegExp(`(?:^|[;\\s])${property}:\\s*(-?\\d+)px`))?.[1] ?? NaN);

  it("lays a member card out as an ordinary card inside the indent, with the session list's own class names", async () => {
    const { read, rule } = await styles();
    const html = render(false, row(), true, undefined, { member: true, depth: 1, teamFirst: true });
    expect(html).toMatch(/class="scard tiled[^"]*sc-member[^"]*sc-member-first/);
    // The Team tab's .team-member (flex, padding, border, background) squeezed it into a 14 px bar.
    expect(html).not.toMatch(/class="[^"]*team-member/);
    expect(html).not.toMatch(/style="[^"]*left:\s*2px/);
    const member = rule(".scard.sc-member");
    expect(px(member, "left")).toBe(26);
    expect(px(member, "right")).toBe(2);
    expect(member).not.toMatch(/padding|max-width|display/);
    // Narrow lists: the badge and line 3 shrink inside the card instead of overflowing it.
    expect(rule(".scard.sc-member .agent-badge")).toMatch(/min-width:\s*0/);
    expect(rule(".scard.sc-member .sc-title")).toMatch(/min-width:\s*0/);
    // No other stylesheet styles these names, so no rule of another view reaches them.
    for (const name of ["teams", "plan", "newteam", "app", "base"]) {
      expect(read(name), name).not.toMatch(/\.sc-member|\.sc-team-strip|\.sc-team-status/);
    }
  });

  it("draws one continuous line from the lead's card to the last member, with an elbow into each card", async () => {
    const { rule } = await styles();
    // A card spans its 104 px row from 4 to 98 (top 4, bottom 6), and the lead's card ends 6 px above its row's end:
    // the line reaches past the gaps on both sides, so consecutive rows join with no break.
    const card = rule(".scard");
    expect(px(card, "top")).toBe(4);
    expect(px(card, "bottom")).toBe(6);
    const line = rule(".scard.sc-member::before");
    expect(px(line, "top")).toBeLessThanOrEqual(-(4 + 6));
    expect(px(line, "bottom")).toBeLessThanOrEqual(-6);
    const elbow = rule(".scard.sc-member::after");
    // The elbow starts on the line and reaches into the card.
    expect(px(elbow, "left")).toBe(px(line, "left"));
    expect(px(elbow, "left") + px(elbow, "width")).toBeGreaterThan(0);
    // The last member's line stops at its elbow.
    const last = rule(".scard.sc-member.sc-member-last::before");
    expect(px(line, "top") + px(last, "height")).toBeGreaterThanOrEqual(px(elbow, "top"));
    expect(px(line, "top") + px(last, "height")).toBeLessThanOrEqual(px(elbow, "top") + 2);
  });

  it("puts the fold control at the strip's right end: a small icon button with the member count and Fold or Unfold team (task 452)", async () => {
    const { rule } = await styles();
    const team = { name: "T", status: "running", phase: 9, phase_done: 257, phase_total: 258, phases_done: 8, phases_total: 9, attention: 3,
      members: Array.from({ length: 14 }, (_, i) => `m${i}`) };
    const strip = (html: string) => html.match(/<div[^>]*class="sc-team-strip"[\s\S]*?<\/button>\s*(?:<!--[^>]*-->\s*)*<\/div>/)?.[0] ?? "";
    const toggle = (html: string) => html.match(/<button[^>]*class="icon-btn small sc-team-toggle"[^>]*>/)?.[0] ?? "";
    const open = render(false, row(), true, undefined, { teamSummary: team, expanded: true });
    expect(open).not.toMatch(/class="team-strip"|class="team-members"|class="team-status/);
    // The button is the strip's last element, after the count: its right end.
    const openStrip = strip(open);
    expect(openStrip).not.toBe("");
    expect(openStrip.indexOf("14 members")).toBeGreaterThan(openStrip.indexOf("Running"));
    expect(openStrip.indexOf('class="icon-btn small sc-team-toggle"')).toBeGreaterThan(openStrip.indexOf("14 members"));
    expect(toggle(open)).toContain('type="button"');
    expect(toggle(open)).toContain('aria-expanded="true"');
    expect(toggle(open)).toContain('aria-label="Fold team"');
    expect(toggle(open)).toContain('title="Fold team"');
    const folded = render(false, row(), true, undefined, { teamSummary: team, expanded: false });
    expect(strip(folded)).toContain("14 hidden");
    expect(strip(folded)).toContain('data-folded="1"');
    expect(toggle(folded)).toContain('aria-expanded="false"');
    expect(toggle(folded)).toContain('aria-label="Unfold team"');
    // The normal small icon-button size, with hover and keyboard focus states; the chevron turns when folded.
    const button = rule(".sc-team-toggle.icon-btn.small");
    expect(px(button, "width")).toBe(22);
    expect(px(button, "height")).toBe(22);
    expect(rule(".sc-team-toggle.icon-btn.small:hover")).toMatch(/background:/);
    expect(rule(".sc-team-toggle.icon-btn.small:focus-visible")).toMatch(/outline:\s*2px solid/);
    expect(rule('.sc-team-toggle[aria-expanded="false"] .chev')).toMatch(/rotate:\s*-90deg/);
    // The strip holds the button (26 px), stays inside the card and folds on a click anywhere on it.
    const box = rule(".sc-team-strip");
    expect(px(box, "height")).toBe(26);
    expect(px(box, "top") + px(box, "height")).toBeLessThanOrEqual(104 + 26 - 4 - 6);
    expect(box).toMatch(/box-sizing:\s*border-box/);
    expect(box).toMatch(/overflow:\s*hidden/);
    expect(box).toMatch(/cursor:\s*pointer/);
    const { readFileSync } = await import(/* @vite-ignore */ ["node", "fs"].join(":")) as { readFileSync(path: URL): string };
    const source = String(readFileSync(new URL("./SessionCard.tsx", import.meta.url)));
    expect(source).toMatch(/class="sc-team-strip"[^>]*\n?[^>]*onClick=\{\(e\) => \{ e\.stopPropagation\(\); props\.onToggle\(\); \}\}/);
  });

  it("fits a member card inside the indent: the chip and the badge stay whole, the title and branch ellipsize (tasks 452, 455)", async () => {
    const { read, rule, effectiveRule } = await styles();
    // A member card is 26 px narrower for the tree's indent. The container queries cap every
    // card's badge (128/104 px) and chip (30 %), so a member's chip and badge keep their whole
    // box instead: the machine alias is never cut to 'ms' and the caption keeps its cooling bar.
    const badge = effectiveRule(".scard.sc-member .sc-l1 > .sbadge");
    expect(badge).toMatch(/flex:\s*0 0 auto/);
    expect(badge).toMatch(/max-width:\s*100%/);
    expect(badge).not.toMatch(/flex-shrink:\s*1/);
    const chip = effectiveRule(".scard.sc-member .sc-l2 > .mbadge");
    expect(chip).toMatch(/flex:\s*0 0 auto/);
    expect(chip).toMatch(/max-width:\s*100%/);
    // The title and the branch are the lines that give way, each with its own ellipsis.
    expect(rule(".scard.sc-member .sc-title")).toMatch(/min-width:\s*0/);
    const html = render(false, row(), true, undefined, { member: true, depth: 1, teamFirst: true });
    const title = html.match(/<span class="sc-title ell"[^>]*>/)?.[0] ?? "";
    // No 72 px floor inside the indent: the title shrinks before the chip or the badge lose a pixel.
    expect(title).toMatch(/min-width:\s*0px/);
    expect(html).toMatch(/class="sc-title ell"/);
    expect(html).toMatch(/class="sc-meta ell"/);
    expect(html).toMatch(/class="entity mbadge/);
    expect(html).toMatch(/class="sbadge k-/);
    // The unlayered restatements come after the container queries, so they win in either layer order.
    const css = read("sessions");
    for (const selector of [".scard.sc-member .sc-l1 > .sbadge { flex: 0 0 auto", ".scard.sc-member .sc-l2 > .mbadge { flex: 0 0 auto"]) {
      expect(css.lastIndexOf(selector)).toBeGreaterThan(css.indexOf("@container (max-width: 360px)"));
    }
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

    const member = render(false, row(), true, undefined, { member: true, depth: 1, teamFirst: true });
    expect(member).toContain("sc-member");
    const lead = render(false, row(), true, undefined, { teamSummary: { name: "T", status: "running", phase: 1, phase_done: 1, phase_total: 2,
      phases_done: 1, phases_total: 2, attention: 0, members: [] } });
    expect(lead).toContain("sc-team-strip");
  });
});

describe("supervised and muted children (owner 2026-11-08)", () => {
  const parent = { machine: "11111111-1111-4111-8111-111111111111", session_id: "22222222-2222-4222-8222-222222222222",
    name: "codex/project/lead", conversation_id: "c", run_id: "r", state: "live" };
  // The order of the arrow, the name and the crossed bell in the first line.
  const order = (html: string) => [...html.matchAll(/class="(sc-supervised|sc-title ell|sc-mute|sbadge)[" ]/g)].map((m) => m[1].split(" ")[0]);
  it("shows the supervised arrow whether muted or not, and the bell right after the name", () => {
    for (const compact of [false, true]) {
      // The status pill follows them (review note, 2026-11-08).
      expect(order(render(compact, row({ parent, muted: true })))).toEqual(["sc-supervised", "sc-title", "sc-mute", "sbadge"]);
      expect(order(render(compact, row({ parent })))).toEqual(["sc-supervised", "sc-title", "sbadge"]);
      expect(order(render(compact, row({ muted: true })))).toEqual(["sc-title", "sc-mute", "sbadge"]);
      // A parent that is not live supervises nothing.
      expect(order(render(compact, row({ parent: { ...parent, state: "missing" }, muted: true })))).toEqual(["sc-title", "sc-mute", "sbadge"]);
    }
  });

  it("names who started a launched session instead of claiming it is supervised (task 467)", () => {
    const started = { fromParent: { name: parent.name, key: "\nrun:lead" } };
    const full = render(false, row({ parent: { ...parent, source: "launch" } }), true, undefined, started);
    expect(order(full)).toEqual(["sc-title", "sbadge"]);
    expect(full).toContain("sc-from");
    expect(full).toContain("Started by codex/project/lead");
    // A compact row keeps its own row too; it has no room for the hint.
    expect(order(render(true, row({ parent: { ...parent, source: "launch" } }), true, undefined, started))).toEqual(["sc-title", "sbadge"]);
  });
});

describe("subagent rows (owner 2026-11-08)", () => {
  const child: Subagent = { id: "a", name: "general-purpose", state: "working", summary: "Reading the plan" };
  const parts = (html: string) => [...html.matchAll(/class="(cc-tile|cc-mark|sc-title ell|sbadge|cc-expand cc-slot)[" ]/g)].map((m) => m[1]);
  it("shows the parent's account mark, and keeps the expand button's slot empty", () => {
    const html = renderChild(row({ accountId: "claude-a1" }), child);
    expect(parts(html)).toEqual(["cc-tile", "cc-mark", "sc-title ell", "sbadge", "cc-expand cc-slot"]);
    expect(html).toContain("acct-mark");
    // The slot is no control: hidden from assistive technology, not a button.
    expect(html).toMatch(/<span class="cc-expand cc-slot" aria-hidden="true">/);
    expect(html).not.toContain("<button");
    // A parent without a mark gives its subagents none.
    expect(parts(renderChild(row(), child))).toEqual(["cc-tile", "sc-title ell", "sbadge", "cc-expand cc-slot"]);
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
