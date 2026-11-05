import { teamAction } from "../bridge";
import type { CatalogPort } from "../ui/teams/TeamCatalog";
import type { TeamOutcome } from "../ui/teams/types";

/** The catalog actions through the guarded Tauri command. The UI asks the
 * owner before Apply and Discard, so those calls are confirmed here. */
export const catalogPort: CatalogPort = {
  decide: (proposal: string, decision: "keep" | "reject" | "undo"): Promise<TeamOutcome> =>
    teamAction({
      confirmed: true,
      action: { kind: "catalog_decide", proposal, decision },
    }),
  apply: (): Promise<TeamOutcome> =>
    teamAction({ confirmed: true, action: { kind: "catalog_apply" } }),
  discard: (): Promise<TeamOutcome> =>
    teamAction({ confirmed: true, action: { kind: "catalog_discard" } }),
};
