import type { Target } from "../../types";

export interface CleanupOffer { id: string; checked: boolean }
export interface CleanupOfferState { offer?: CleanupOffer; error?: string }
/** Read-only offer/cancel ports deliberately have no effect or fallback call. */
export async function readCleanupOffer(
  target: Target,
  ports: { offer(target: Target): Promise<CleanupOffer | null>; cancel(id: string): Promise<unknown> },
  live: () => boolean,
  publish: (state: CleanupOfferState) => void,
) {
  try {
    const offer = await ports.offer(target);
    if (!live()) { if (offer) void ports.cancel(offer.id); return; }
    publish({ offer: offer ?? undefined });
  } catch (error) {
    if (live()) publish({ error: error instanceof Error ? error.message : String(error) });
  }
}
export function unavailableServiceRemoval(error: string): boolean {
  return error === "termination_native_relay_unavailable" || error === "termination_native_image_unavailable";
}
