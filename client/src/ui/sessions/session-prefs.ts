import { flag } from "../../store/settings";

// Settings > Sessions (SettingsPage.h:232-233): the owner's session-list
// preferences, under Qt's QSettings names. Compact is off by default (Qt's
// normal 104 px card), hover expansion and hiding empty projects are on.

/** "Compact session rows": 94 px cards instead of 104. */
export const compactSessionRows = () => flag("workspace/compact");
/** "Expand the collapsed session list on hover": the strip peeks under the pointer. */
export const expandSessionsOnHover = () => flag("workspace/expandSessionsOnHover");
/** "Hide projects without active sessions": an empty project header is left out. */
export const hideEmptyProjects = () => flag("workspace/hideEmptyProjects");
