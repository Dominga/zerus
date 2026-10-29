// The value a settings number field keeps (ui.tsx NumberField).

/** As QDoubleSpinBox keeps it: rounded to its decimals, within its range. */
export function fieldValue(value: number, min: number, max: number, decimals = 0): number {
  const scale = 10 ** decimals;
  return Math.min(max, Math.max(min, Math.round(value * scale) / scale));
}
