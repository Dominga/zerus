import { describe, expect, it } from "vitest";
import source from "./Activity.tsx?raw";

// zerus-ig5q: Activity keeps its content column hidden until it is placed and
// shows a loading heading over it meanwhile. A descendant that sets its own
// visibility to "visible" shows through that hidden column: the heading was
// drawn twice, a few pixels apart, while every session opened. The native
// regression is scripts/activity-open-smoke.html; this keeps the rule in CI.
// zerus-isrzi: scripts/child-visit-smoke.html is the native regression; this keeps the wiring in CI.
// zerus-soudn: scripts/activity-pending-smoke.html is the native regression for
// the pending states drawn over the hidden column; this keeps their place in CI.
describe("Activity projections", () => {
  it("builds through the open child's projection or the session's, so a visit can't reset the parent's", () => {
    expect(source).toContain("project(!!subagentOpen(), displayTarget(),");
  });
});

describe("Activity visibility", () => {
  it("only the content column itself turns visible; everything inside it inherits", () => {
    const visible = [...source.matchAll(/visibility:[^;}]*"visible"/g)].map((match) => match[0]);
    expect(visible).toEqual(['visibility: placed() ? "visible"']);
  });
});

describe("Activity pending states (zerus-soudn)", () => {
  it("draw in the loading overlay, above the hidden column and its scroller, and take clicks", () => {
    const overlay = source.slice(source.indexOf("<Show when={!placed()}>"), source.indexOf("<LocalDeleteDialog />"));
    expect(overlay).toContain('"pointer-events": "none", "z-index": "1"');
    expect(overlay).toContain('<div style={{ "pointer-events": "auto" }}><ActivityPending source={source()} session={s()} /></div>');
  });
});
