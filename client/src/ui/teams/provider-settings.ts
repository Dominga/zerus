// UI ideas adapted from WhiteWh's testsSettings.ts. No executable templates.
import { tr } from "../../i18n";
import type { ProviderInspection, ValidationConfig } from "../../store/teamProviders";
export function parseOverride(text: string): { value: ValidationConfig | null; error: string } {
  if (!text.trim()) return { value: null, error: "" };
  try {
    const value: unknown = JSON.parse(text);
    if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error(tr("The provider override must be a JSON object."));
    return { value: value as ValidationConfig, error: "" };
  } catch { return { value: null, error: tr("Enter a valid provider configuration as JSON.") }; }
}
export const overrideText = (value: ValidationConfig | null) => value ? JSON.stringify(value, null, 2) : "";
export function sourceLine(config: ProviderInspection): string {
  const base = config.source === "file" ? tr("From the project's .zerus/tests.toml") : config.source === "detected" ? tr("Detected from project metadata") : tr("No known provider configuration");
  return config.override_config ? tr("%1, with your override", base) : base;
}
