import { describe, expect, it } from "vitest";

// Task 317, found by the side-by-side check 205 (docs/qa/205-side-by-side.md):
// the lead's Activity as Qt draws it. F7: the lead's proposal cards show in
// full above the composer; F8: the wait strip keeps "N wait for you" on one
// line; F13: the lead's session carries its team bar and a strip naming the
// team outside the Team tab.
const read = async (path: string) => {
  const fs = await import(/* @vite-ignore */ ["node", "fs"].join(":")) as { readFileSync(path: URL, encoding: "utf8"): string };
  return fs.readFileSync(new URL(path, import.meta.url), "utf8").replace(/\r\n/g, "\n");
};
const rule = (css: string, selector: string) => {
  const at = css.indexOf(`${selector} {`);
  expect(at, selector).toBeGreaterThan(-1);
  return css.slice(at, css.indexOf("}", at));
};

describe("the lead's Activity (task 317)", () => {
  it("F7: the cards never shrink into a short inner scroll and scroll once, past 60% of the column", async () => {
    const css = await read("../styles/teams.css");
    const box = rule(css, ".activity-wrap > .lead-proposals");
    expect(box).toContain("flex: none");
    expect(box).toContain("max-height: 60%");
    expect(box).toContain("overflow-y: auto");
    const list = rule(css, ".lead-proposals .team-proposals");
    expect(list).toContain("max-height: none");
    expect(list).toContain("overflow: visible");
    // The scroll container hides its bar like every other one (popups rule).
    expect(await read("../styles/base.css")).toContain(".team-proposals, .lead-proposals,");
  });

  it("F8: the strip's count and button keep one line; only the title gives way", async () => {
    const css = await read("../styles/teams.css");
    const fixed = rule(css, ".team-waiting-banner-head > b, .team-waiting-banner-head > .btn");
    expect(fixed).toContain("flex: none");
    expect(fixed).toContain("white-space: nowrap");
    expect(rule(css, ".team-waiting-banner-head > .ell")).toContain("min-width: 0");
  });

  it("F13: the lead's session shows its team bar and strip above the tabs, except in the Team tab", async () => {
    const detail = await read("./SessionDetail.tsx");
    // Task #61: the header loads on first use, inside its LazyBoundary.
    const header = detail.indexOf('<Show when={detailTab() !== "team"}><LazyBoundary><LeadTeamHeader /></LazyBoundary></Show>');
    expect(header).toBeGreaterThan(-1);
    expect(header).toBeLessThan(detail.indexOf('<div class="detail-tabs tabs" role="tablist">'));
    const tab = await read("../teams/TeamTab.tsx");
    const start = tab.indexOf("export function LeadTeamHeader()");
    const body = tab.slice(start, tab.indexOf("/** The canvas until the plan view takes it", start));
    // Only the session that leads the team, from the live read (a member shows none).
    expect(body).toContain("leadsTeam(view, now)");
    expect(body).toContain('summary.status === "draft"');
    // The same bar as the Team tab. Owner, 2026-11-09: the "Team <name> Lead /
    // Open team panel" strip is gone; the lead's role is one pill beside the
    // session title (LeadTitleBadge) that opens the Team tab.
    expect(body).toContain("<TeamBar computer={props.computer} team={props.team}");
    expect(body).not.toContain("data-lead-team-strip");
    expect(detail).toContain("<LeadTitleBadge />");
    expect(tab).toContain("data-lead-title");
    expect(body).toContain("<TeamSettingsDialog");
  });
});
