import * as organization from "../../store/organization";
import { catalog } from "../../store/catalog";
import { archive } from "../../store/archive";
import { notify } from "../../store/actions";
import type { Session } from "./types";
import { createOrgDialogs } from "./organization-ui";

// The app's one organization dialog state, kept out of the .tsx host so that
// non-UI modules (the selection bar) can open it without loading components.
// A selection's move re-checks its rows against the live list and the loaded
// archive (zerus-5wmh.18.1).
export const dialogs = createOrgDialogs(organization, { live: (key) => catalog.rows[key] ?? archive.row(key), notify: (tone, text) => notify(tone, text) });

/** Move to project for a selection (zerus-5wmh.18.1). */
export const openMoveSelection = (rows: readonly Session[]): void => dialogs.openMoveSelection(rows);
