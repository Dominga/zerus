import { describe, expect, it } from "vitest";
import { FRAME_INSET, groupFrames } from "./folder-frames";
import type { Row } from "./model";

// Task #457: a project folder is framed in its own colour around its header and
// every card, folded or unfolded.
const header = (project: string, extra: Record<string, unknown> = {}): Row =>
  ({ type: "header", id: `h:${project}`, project, name: project, color: null, palette: 0, collapsed: false, total: 0, attention: 0, unread: 0, working: 0, ...extra }) as unknown as Row;
const card = (name: string): Row => ({ type: "card", id: `\nrun:${name}`, session: {} as never, expandable: false, expanded: false, compactRow: false });

describe("project frames in the session list (task #457)", () => {
  it("wraps a project's header and every card, in the folder's own colour", () => {
    const rows = [header("alpha", { color: "#3fb950" }), card("a0"), card("a1"), header("beta"), card("b0")];
    const frames = groupFrames(rows, [40, 94, 94, 40, 94]);
    expect(frames).toHaveLength(2);
    // alpha: header 40 + two 94 px cards, 2 px of padding above and below.
    expect(frames[0]).toEqual({ id: "h:alpha", top: 2, height: 228 - FRAME_INSET * 2, frame: "#3fb950" });
    // beta: the header and its single card, in the neutral border: no colour set.
    expect(frames[1]).toEqual({ id: "h:beta", top: 228 + 2, height: 40 + 94 - FRAME_INSET * 2, frame: "var(--z-color-border)" });
    // Frames sit edge to edge with the rows they wrap.
    expect(frames[1].top).toBe(frames[0].top + frames[0].height + FRAME_INSET * 2);
  });

  it("wraps a folded project's header alone", () => {
    const rows = [header("alpha", { color: "#3fb950", collapsed: true }), header("beta"), card("b0")];
    const frames = groupFrames(rows, [40, 40, 94]);
    expect(frames[0]).toEqual({ id: "h:alpha", top: 2, height: 40 - FRAME_INSET * 2, frame: "#3fb950" });
    expect(frames[1].height).toBe(40 + 94 - FRAME_INSET * 2);
  });

  it("draws no frame for a list without project headers", () => {
    expect(groupFrames([card("a0")], [94])).toEqual([]);
  });
});
