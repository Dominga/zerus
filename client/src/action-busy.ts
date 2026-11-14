import { renderText, type Text } from "./text";
import type { TranslationArgument } from "./i18n";

const PREFIX = "zerus-action-busy-v1:";
const MESSAGE = "This session is busy with “%1” (%2 elapsed, since %3 UTC). Wait for it to finish.";

function validText(value: unknown, depth = 0): value is Text {
  if (!value || typeof value !== "object" || depth > 2) return false;
  const text = value as Text;
  return typeof text.source === "string" && text.source.length <= 256
    && (text.args === undefined || (Array.isArray(text.args) && text.args.length <= 3 && text.args.every((arg) =>
      arg?.kind === "text" ? validText(arg.value, depth + 1)
        : arg?.kind === "number" ? Number.isSafeInteger(arg.value)
          : arg?.kind === "verbatim" && typeof arg.value === "string" && arg.value.length <= 128)));
}

/** Only native action-admission errors carry this bounded typed-prose prefix. */
export function formatActionBusy(error: unknown, translate: (source: string, ...args: TranslationArgument[]) => string): string | undefined {
  const value = typeof error === "string" ? error : error instanceof Error ? error.message : "";
  if (!value.startsWith(PREFIX) || value.length > 4096) return;
  try {
    const message: unknown = JSON.parse(value.slice(PREFIX.length)).message;
    if (validText(message) && message.source === MESSAGE) return renderText(message, translate);
  } catch { /* Other failures keep their original error and classification. */ }
}
