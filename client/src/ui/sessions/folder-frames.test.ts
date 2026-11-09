import { describe, expect, it } from "vitest";
import { FRAME_INSET, FRAME_TOP, headerFrames } from "./folder-frames";
import type { Row } from "./model";

// Task #457 (owner, 2026-11-09): a project's colour frames its header row alone,
// not the folder with its cards.
const header = (project: string, extra: Record<string, unknown> = {}): Row =>
  ({ type: "header", id: `h:${project}`, project, name: project, color: null, palette: 0, collapsed: false, total: 0, attention: 0, unread: 0, working: 0, ...extra }) as unknown as Row;
const card = (name: string): Row => ({ type: "card", id: `\nrun:${name}`, session: {} as never, expandable: false, expanded: false, compactRow: false });

describe("project frames in the session list (task #457)", () => {
  it("frames a project's header row alone, in the folder's own colour", () => {
    const rows = [header("alpha", { color: "#3fb950" }), card("a0"), card("a1"), header("beta"), card("b0")];
    const frames = headerFrames(rows, [40, 104, 104, 40, 104]);
    expect(frames).toHaveLength(2);
    // alpha: its 40 px header row only, 4 px down and 2 px inside.
    expect(frames[0]).toEqual({ id: "h:alpha", top: FRAME_TOP, height: 40 - FRAME_TOP - FRAME_INSET * 2, frame: "#3fb950" });
    // beta follows alpha's two cards: the frame covers its own header, not theirs.
    expect(frames[1].id).toBe("h:beta");
    expect(frames[1].top).toBe(40 + 104 + 104 + FRAME_TOP);
    expect(frames[1].height).toBe(40 - FRAME_TOP - FRAME_INSET * 2);
    // beta has no colour of its own: the neutral border token.
    expect(frames[1].frame).toBe("var(--z-color-border)");
  });

  it("draws the same frame for a folded project, whose header is its only row", () => {
    const frames = headerFrames([header("alpha", { color: "#3fb950", collapsed: true })], [40]);
    expect(frames).toHaveLength(1);
    expect(frames[0].top).toBe(FRAME_TOP);
    expect(frames[0].height).toBe(32);
  });

  it("draws no frame for a list without project headers", () => {
    expect(headerFrames([card("a0")], [104])).toEqual([]);
  });
});
