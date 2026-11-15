export function recordedDate(atMs: number): { iso: string; text: string } | null {
  if (!Number.isSafeInteger(atMs)) return null;
  const date = new Date(atMs);
  if (!Number.isFinite(date.getTime())) return null;
  return { iso: date.toISOString(), text: date.toLocaleString() };
}
