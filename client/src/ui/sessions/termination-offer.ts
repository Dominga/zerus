import type { Target } from "../../types";

export const PROGRAM_LIST_CHANGED = "The program list changed or could not be checked. Review the working folder again.";

export interface CleanupProgram { pid: number; label: string }
export interface CleanupOffer { id: string; checked: boolean; programs?: readonly CleanupProgram[] }
/** A partial or malformed list must not become a confirmation of another list. */
export function completeProgramList(value: readonly CleanupProgram[] | undefined): boolean {
  if (value === undefined) return true; // The existing offer has no program-stop lane.
  return Array.isArray(value) && value.length <= 64 && value.every((program, i) => program != null && Number.isInteger(program.pid)
    && program.pid > 0 && program.pid <= 0x7fffffff
    && typeof program.label === "string" && program.label.length > 0
    && new TextEncoder().encode(program.label).length <= 512 && !/[\x00-\x1f\x7f-\x9f]/.test(program.label)
    && (i === 0 || value[i - 1].pid < program.pid));
}
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
    if (offer && !completeProgramList(offer.programs)) {
      void ports.cancel(offer.id);
      throw Error(PROGRAM_LIST_CHANGED);
    }
    publish({ offer: offer ?? undefined });
  } catch (error) {
    if (live()) publish({ error: error instanceof Error ? error.message : String(error) });
  }
}
export function unavailableServiceRemoval(error: string): boolean {
  return error === "termination_native_image_unavailable";
}
