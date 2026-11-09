import { createSignal } from "solid-js";

// Task #410: the Profile page's two lower sections fold to their first rows by
// default; the owner's choice is remembered per view (the team's Profile page)
// while the app runs, so a poll or a switch to another tab and back keeps it.
export type ProfileSection = "losses" | "roles";

/** Rows shown while a section is folded. */
export const PROFILE_PREVIEW_ROWS = 5;

const [folded, setFolded] = createSignal<Record<string, boolean>>({});
const key = (view: string, section: ProfileSection) => `${view}\u001f${section}`;

/** Folded unless the owner opened it; a new view starts folded. */
export const profileSectionFolded = (view: string, section: ProfileSection) => folded()[key(view, section)] ?? true;

export function setProfileSectionFolded(view: string, section: ProfileSection, value: boolean) {
  setFolded({ ...folded(), [key(view, section)]: value });
}

/** Tests and a fresh replay stay independent of a previous run's choices. */
export function resetProfileSections() {
  setFolded({});
}
