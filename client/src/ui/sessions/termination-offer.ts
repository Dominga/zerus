import type { Target } from "../../types";

export const PROGRAM_LIST_CHANGED = "The program list changed or could not be checked. Review the working folder again.";

export interface CleanupProgram { pid: number; label: string }
export interface CleanupOffer { id: string; checked: boolean; programs?: readonly CleanupProgram[] }
/** A partial or malformed list must not become a confirmation of another list. */
export function completeProgramList(value: readonly CleanupProgram[] | undefined): boolean {
  if (value === undefined) return true; // The existing offer has no program-stop lane.
  return Array.isArray(value) && value.length <= 64 && value.every((program, i) => program != null && Number.isInteger(program.pid)
    && program.pid > 1 && program.pid <= 0x7fffffff
    && typeof program.label === "string" && program.label.length > 0
    && new TextEncoder().encode(program.label).length <= 512 && !/[\x00-\x1f\x7f-\x9f]/.test(program.label)
    && (i === 0 || value[i - 1].pid < program.pid));
}
export interface CleanupOfferState { offer?: CleanupOffer; error?: string }
/** A slow optional review cannot park Stop forever. After this UI wait,
 * ordinary End still goes through the native claim/confirmation guards and
 * reports the live holder if custody has not released it. */
export async function waitForCleanupOffer(pending: Promise<string | undefined>): Promise<string | undefined> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([pending, new Promise<undefined>((resolve) => { timer = setTimeout(() => resolve(undefined), 5000); })]);
  } finally { clearTimeout(timer); }
}
/** Read-only offer/cancel ports deliberately have no effect or fallback call. */
export async function readCleanupOffer(
  target: Target,
  ports: { offer(target: Target): Promise<CleanupOffer | null>; cancel(id: string): Promise<unknown> },
  live: () => boolean,
  publish: (state: CleanupOfferState) => void,
) {
  let cancelling = false;
  try {
    const offer = await ports.offer(target);
    if (!live()) { if (offer) { cancelling = true; await ports.cancel(offer.id); } return; }
    if (offer && !completeProgramList(offer.programs)) {
      cancelling = true;
      await ports.cancel(offer.id);
      throw Error(PROGRAM_LIST_CHANGED);
    }
    publish({ offer: offer ?? undefined });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    if (live()) publish({ error: message });
    else if (cancelling) return message;
  }
}
export function unavailableServiceRemoval(error: string): boolean {
  return error === "termination_native_image_unavailable";
}
