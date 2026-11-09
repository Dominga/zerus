import type { ProgramForceOffer } from "../../bridge";
import { completeProgramList, PROGRAM_LIST_CHANGED } from "./termination-offer";

/** Read-only offer: there is deliberately no confirm/effect port here. */
export async function readForceOffer(
  id: string,
  ports: { offer(id: string): Promise<ProgramForceOffer>; cancel(id: string, confirmation: string): Promise<unknown> },
  live: () => boolean,
): Promise<ProgramForceOffer | undefined> {
  const offer = await ports.offer(id);
  if (!live() || !offer || typeof offer.confirmation !== "string" || !offer.confirmation
    || !Number.isSafeInteger(offer.remaining_millis) || offer.remaining_millis < 0 || offer.remaining_millis > 300_000
    || !Number.isSafeInteger(offer.revision) || offer.revision < 0
    || !Array.isArray(offer.programs) || offer.programs.length === 0 || !completeProgramList(offer.programs)) {
    if (offer?.confirmation) await ports.cancel(id, offer.confirmation);
    if (live()) throw Error(PROGRAM_LIST_CHANGED);
    return;
  }
  return offer;
}
