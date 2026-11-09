// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import type { Row } from "./model";

// Task #457: a project folder is a frame in its own colour around its header
// and its session cards, folded or unfolded.
const fixture = vi.hoisted(() => ({ rows: () => [] as Row[] }));
vi.mock("../../bridge", async (original) => ({ ...await original<object>(), gitRows: vi.fn().mockResolvedValue(undefined) }));
vi.mock("./model", async (original) => ({ ...await original<object>(), listRows: () => fixture.rows() }));
vi.mock("@tanstack/solid-virtual", () => ({
  createVirtualizer: (options: { count: number; getItemKey(n: number): string }) => ({
    measure() {}, _willUpdate() {}, scrollToIndex() {}, getTotalSize: () => options.count * 104,
    getVirtualItems: () => options.count ? [{ key: options.getItemKey(0), start: 0, index: 0 }] : [],
  }),
}));
import { SessionList } from "./SessionList";
import { NORMAL_CARD_HEIGHT, ROW_HEIGHT } from "./model";
import { applyBatch, catalog } from "../../store/catalog";
import { emptyBatch } from "../../types";
import { setPage, setStripMode } from "../../store/ui";

const KEYS = ["a0", "a1", "b0"];
const key = (name: string) => `\nrun:${name}`;
const header = (project: string, color: string | null, collapsed = false): Row =>
  ({ type: "header", id: `h:${project}`, project, name: project, color, palette: 0, collapsed, total: 0, attention: 0, unread: 0, working: 0 });
const card = (name: string): Row => ({ type: "card", id: key(name), session: catalog.rows[key(name)]!, expandable: false, expanded: false, compactRow: false });

let dispose: (() => void) | undefined;
const settle = async () => { for (let i = 0; i < 12; i++) await Promise.resolve(); };

/** Mount the real list over a fixture of rows, and return its frames. */
async function mount(rows: Row[]): Promise<HTMLElement[]> {
  fixture.rows = () => rows;
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(SessionList, {}), host);
  await settle();
  return [...host.querySelectorAll<HTMLElement>(".group-frame")];
}

beforeEach(() => {
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  setPage("sessions"); setStripMode(false);
  applyBatch({ ...emptyBatch(), replaces: [{ computer: "", rows: KEYS.map((name) => ({
    computer: "", key: `run:${name}`, row: { name: `codex/fixture/${name}`, agent: "codex", status: "ready", state: "running", cwd: `/fixture/${name}`, run_id: name },
  })) }] });
});
afterEach(async () => {
  dispose?.(); dispose = undefined; await settle();
  document.body.replaceChildren(); vi.clearAllMocks(); vi.unstubAllGlobals();
});

describe("a project's frame in the session list (task #457)", () => {
  it("frames the project's header row alone, in the folder's own colour", async () => {
    const [alpha, beta] = await mount([header("alpha", "#3fb950"), card("a0"), card("a1"), header("beta", null), card("b0")]);
    const group = ROW_HEIGHT.header + NORMAL_CARD_HEIGHT * 2;
    const height = ROW_HEIGHT.header - 8;
    expect(alpha.dataset.frame).toBe("h:alpha");
    expect(alpha.style.getPropertyValue("--frame")).toBe("#3fb950");
    expect(alpha.style.height).toBe(`${height}px`);
    expect(alpha.style.transform).toBe("translateY(4px)");
    // beta follows alpha's rows, in the neutral border: the folder has no colour.
    expect(beta.dataset.frame).toBe("h:beta");
    expect(beta.style.getPropertyValue("--frame")).toBe("var(--z-color-border)");
    expect(beta.style.transform).toBe(`translateY(${group + 4}px)`);
    expect(beta.style.height).toBe(`${height}px`);
  });

  it("draws the same frame for a folded project's header", async () => {
    const [alpha, beta] = await mount([header("alpha", "#3fb950", true), header("beta", null), card("b0")]);
    expect(alpha.style.height).toBe(`${ROW_HEIGHT.header - 8}px`);
    expect(beta.style.transform).toBe(`translateY(${ROW_HEIGHT.header + 4}px)`);
    expect(beta.style.height).toBe(`${ROW_HEIGHT.header - 8}px`);
  });

  it("draws the frames under the rows and lets the pointer through", async () => {
    const [alpha] = await mount([header("alpha", "#3fb950"), card("a0")]);
    const container = alpha.parentElement!;
    const firstRow = container.querySelector(".vrow");
    expect(firstRow).not.toBeNull();
    // The frames come first in the same absolute container, so rows paint above them.
    expect(alpha.compareDocumentPosition(firstRow!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(alpha.getAttribute("aria-hidden")).toBe("true");
  });
});

// Task 468 (owner, 2026-11-09: "прозрачность каймы выстави не более 50%"): the
// folder header's colour frame is the folder's colour at half, the neutral
// fallback included, in both appearances and every skin.
describe("the folder frame's stroke is at most half the colour (task 468)", () => {
  /** The alpha a border colour carries: 1 when it is opaque or unreadable here. */
  const alphaOf = (colour: string): number => {
    const rgba = /rgba?\(\s*([^)]*)\)/i.exec(colour);
    if (rgba) {
      const parts = rgba[1].split(/[\s,/]+/).filter(Boolean);
      return parts.length > 3 ? Number(parts[3]) : 1;
    }
    // color-mix(in srgb, <colour> N%, transparent) keeps N% of the colour, so the
    // result's alpha is N/100.
    const mix = /color-mix\(\s*in srgb\s*,\s*[^,]+?\s([\d.]+)%\s*,\s*transparent\s*\)/i.exec(colour);
    return mix ? Number(mix[1]) / 100 : 1;
  };

  it("states the stroke as half the frame colour in the real stylesheet", async () => {
    // happy-dom gives import.meta.url an http scheme, so the stylesheet is read
    // from the project folder the tests run in (client/).
    const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as { readFileSync(path: string, encoding: "utf8"): string };
    const nodePath = (await import(/* @vite-ignore */ ["node", "path"].join(":"))) as { resolve(...parts: string[]): string };
    const css = fs.readFileSync(nodePath.resolve(process.cwd(), "src/ui/styles/sessions.css"), "utf8");
    const rule = css.split("\n").find((line) => line.trim().startsWith(".group-frame {")) ?? "";
    expect(rule).not.toBe("");
    const border = /border:\s*([^;]+);/.exec(rule)?.[1] ?? "";
    expect(border).toContain("var(--frame)");
    expect(alphaOf(border)).toBeLessThanOrEqual(0.5);
    // No second, opaque border for the same frame anywhere in the stylesheet.
    for (const line of css.split("\n")) {
      if (!line.includes(".group-frame") || !line.includes("border")) continue;
      const value = /border[^:]*:\s*([^;]+);/.exec(line)?.[1] ?? "";
      expect(alphaOf(value)).toBeLessThanOrEqual(0.5);
    }
  });

  it("covers a project's own colour and the neutral fallback with that one stroke", async () => {
    const [alpha, beta] = await mount([header("alpha", "#3fb950"), card("a0"), header("beta", null), card("b0")]);
    // A project's colour and the neutral border both only set --frame; the stroke
    // that reads it is the single half-opacity rule above.
    expect(alpha.style.getPropertyValue("--frame")).toBe("#3fb950");
    expect(beta.style.getPropertyValue("--frame")).toBe("var(--z-color-border)");
  });
});
