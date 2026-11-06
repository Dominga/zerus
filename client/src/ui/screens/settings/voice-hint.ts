// Task 102: Qt's chosen microphone arrives as its name (`speech/microphoneName`,
// imported by client/core), never as Qt's device id, which names no device of
// this client's. When the Voice page lists the microphones and none is chosen
// yet, the one with that name is chosen and the hint is dropped. A hint whose
// device is not connected waits for it; a microphone chosen anyway drops it.

export const MICROPHONE_HINT = "speech/microphoneName";

export interface ListedMicrophone { id: string; name: string }

/** What to do with the hint: the microphone id to choose, and whether to drop the hint. */
export function hintedMicrophone(listed: readonly ListedMicrophone[], current: string, hint: string): { choose?: string; drop: boolean } {
  const wanted = hint.trim();
  if (!wanted) return { drop: false };
  if (current) return { drop: true };
  const found = listed.find((microphone) => microphone.name.trim() === wanted);
  return found ? { choose: found.id, drop: true } : { drop: false };
}
