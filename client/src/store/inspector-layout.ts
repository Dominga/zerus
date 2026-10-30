export function panelGeometry(available: number, requested: number) {
  const floating = available < 360 + 240 + 16;
  const width = Math.max(240, Math.min(900, requested, available - (floating ? 16 : 376)));
  return { floating, width };
}
export function closedCards(value: unknown): string[] {
  if (typeof value !== "string" || value.length > 65536) return [];
  try { const parsed: unknown = JSON.parse(value); return Array.isArray(parsed) ? parsed.filter((v): v is string => typeof v === "string" && v.length <= 1024).slice(-128) : []; }
  catch { return []; }
}
