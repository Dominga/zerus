import { beforeEach, describe, expect, it } from "vitest";
import { PROFILE_PREVIEW_ROWS, profileSectionFolded, resetProfileSections, setProfileSectionFolded } from "./profileSections";

// Task #410: both Profile sections start folded to their first rows, and the
// owner's choice is remembered per view while the app runs.
beforeEach(() => resetProfileSections());

describe("the profile sections' folded state (task #410)", () => {
  it("folds both sections by default and remembers the choice per view", () => {
    expect(PROFILE_PREVIEW_ROWS).toBe(5);
    expect(profileSectionFolded("team-1", "losses")).toBe(true);
    expect(profileSectionFolded("team-1", "roles")).toBe(true);
    setProfileSectionFolded("team-1", "losses", false);
    expect(profileSectionFolded("team-1", "losses")).toBe(false);
    // The other section keeps its own state.
    expect(profileSectionFolded("team-1", "roles")).toBe(true);
    // Another view starts folded; the first keeps its choice.
    expect(profileSectionFolded("team-2", "losses")).toBe(true);
    expect(profileSectionFolded("team-1", "losses")).toBe(false);
    setProfileSectionFolded("team-1", "losses", true);
    expect(profileSectionFolded("team-1", "losses")).toBe(true);
  });
});
