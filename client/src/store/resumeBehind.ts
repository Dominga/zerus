import { createSignal } from "solid-js";
import type { ActionRow } from "../ui/sessions/actions-ui";
import type { TranscriptBehind } from "../ui/sessions/resume-behind";

// Task #247: the one transcript-behind question a failed resume raised, for
// SessionsWindow::resumeBehind's dialog. The dialog (ui/sessions/
// ResumeBehindDialog.tsx) sends the chosen flag through the same guarded
// `perform` lane; closing without an answer cancels.

export interface BehindRequest {
  row: ActionRow;
  behind: TranscriptBehind;
  detail: string;
}

const [state, setState] = createSignal<BehindRequest>();

export const resumeBehind = {
  state,
  open: (row: ActionRow, behind: TranscriptBehind, detail: string) => setState({ row, behind, detail }),
  close: () => setState(undefined),
};
