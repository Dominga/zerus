import type { Settings } from "./settings";

/** Qt's exact machine-keyed local preferences; no alias or identity guessing. */
export function machineAppearance(values: Readonly<Settings>, alias: string, fallback: string) {
  const color = values[`machines/colors/${alias}`];
  return {
    color: typeof color === "string" && /^#(?:[\da-f]{3}|[\da-f]{6})$/iu.test(color) ? color : fallback,
    vivid: values[`machines/vivid/${alias}`] === true,
  };
}
