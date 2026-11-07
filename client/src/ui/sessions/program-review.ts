import { tr } from "../../i18n";
import type { ProgramRetirement } from "../../store/terminationCleanup";

/** Closed native observations, never execution or physical-drain evidence. */
export function programReviewMessage(reason: ProgramRetirement | null | undefined): string | undefined {
  switch (reason) {
    case "expired": return tr("This Force stop offer expired. Review the working folder again.");
    case "source_unavailable": return tr("The original program review is no longer current. Review the working folder again.");
    case "holder_unavailable": return tr("The retained program helper is unavailable. Review the working folder again.");
    case "offer_unavailable": return tr("The program list changed or could not be checked. Review the working folder again.");
    case "stop_unconfirmed": return tr("The program stop is unconfirmed. Check the original result before continuing.");
    case "cleanup_unconfirmed": return tr("Removal could not be confirmed. Check the original result before continuing.");
    default: return;
  }
}
