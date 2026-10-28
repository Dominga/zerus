// Sizes on Settings > Files (DownloadCacheSettings.cpp).

export const GIB = 1024 ** 3;
const UNITS = ["KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
/** QLocale::formattedDataSize: bytes below 1 KiB, otherwise two decimals of the largest IEC unit. */
export function formatDataSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} bytes`;
  const power = Math.min(UNITS.length, Math.floor(Math.log(bytes) / Math.log(1024)));
  return `${(bytes / 1024 ** power).toFixed(2)} ${UNITS[power - 1]}`;
}

/** The limit the controls choose: none when never cleared, else the size in whole bytes. */
export function selectedLimit(mode: string, gib: number): number {
  return mode === "never" ? 0 : Math.round(gib * GIB);
}
