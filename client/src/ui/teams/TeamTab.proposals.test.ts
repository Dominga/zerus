import { describe, expect, it } from "vitest";
import source from "./TeamTab.tsx?raw";
import modeRow from "./WorkingModeRow.tsx?raw";

// Task #338 (owner, 2026-11-08, screenshot of package 161): on the lead's Team
// tab the owner's cards showed in three places: the wait strip, a nested
// proposals window inside the working-mode notice, and the coordinator's
// conversation. Qt shows the full cards once in the coordinator's conversation
// (TeamWorkspace setCards) and, for the lead's session, above the question card
// in its Activity (SessionsWindow mainProposalCards); the wait strip only counts
// and links (TeamWaitingBanner rows). The Team tab must keep exactly the
// coordinator window.
describe("the Team tab's proposal cards (task #338)", () => {
  it("draws them once, in the coordinator's conversation", () => {
    const sites = [...source.matchAll(/<TeamProposals\b/g)];
    expect(sites.length).toBe(1);
    const coordinator = source.slice(source.indexOf("function Coordinator"));
    expect(coordinator.indexOf("<LeadConversation")).toBeGreaterThan(0);
    expect(coordinator.indexOf("<TeamProposals")).toBeGreaterThan(coordinator.indexOf("<LeadConversation"));
  });

  it("keeps the working-mode notice free of a second window", () => {
    // Task #408: the notice is the WorkingModeRow component now.
    const start = modeRow.indexOf('class="team-working-mode"');
    expect(start).toBeGreaterThan(0);
    const notice = modeRow.slice(start, modeRow.indexOf("</section>", start));
    expect(notice).not.toContain("TeamProposals");
    expect(notice).not.toContain("ProposalCardList");
    // The notice itself stays: it names the disagreement and offers the switch.
    expect(notice).toContain("data-working-mode-switch");
    expect(notice).toContain("Propose the switch");
  });

  // Task #451 (owner, 2026-11-09): "I press the button and got no result". The
  // block's buttons now answer: the row takes what hgs did or why it refused
  // (dismissible) and rests while the call runs; Keep records the choice only
  // after hgs kept it; Propose asks for the profile as it is now, never a cached
  // preset.
  it("answers every press of the working-mode block", () => {
    expect(source).toContain("notice={modeNotice()}");
    expect(source).toContain('onDismissNotice={() => setModeNotice("")}');
    expect(source).toContain("busy={modeBusy()}");
    const keep = source.slice(source.indexOf("const keepMode"), source.indexOf("const proposeMode"));
    expect(keep).toContain('outcome.outcome === "done"');
    expect(keep.indexOf('outcome.outcome === "done"')).toBeLessThan(keep.indexOf("markWorkingModeKept"));
    expect(source).toContain("proposeWorkingMode()");
    expect(source).not.toContain("proposeWorkingMode(props.team");
    expect(modeRow).toContain("data-working-mode-note");
    expect(modeRow).toContain("data-working-mode-notice-close");
  });
});
