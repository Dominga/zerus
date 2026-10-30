export function lastCheckedText(raw: string): string {
  if (raw === "Never") return raw;
  const at = new Date(raw);
  if (Number.isNaN(at.getTime())) return raw;
  return new Intl.DateTimeFormat(undefined, { dateStyle: "short", timeStyle: "short" }).format(at);
}
