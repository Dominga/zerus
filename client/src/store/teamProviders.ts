import { invoke } from "@tauri-apps/api/core";
import { inTauri } from "../bridge";
export interface ValidationProvider { name: string; provider: string; kind: string; scopes: string[]; reports: string[]; disabled: boolean }
export interface ValidationConfig { schema: number; provider: ValidationProvider[] }
export interface ValidationSelection { source_digest: string; digest: string; override_config: ValidationConfig | null }
export interface ProviderInspection {
  schema: number; team: string; revision: number; project: string; home: string; checkout: string;
  source: string; source_digest: string; digest: string; valid: boolean; providers: ValidationProvider[];
  diagnostics: Array<{code: string; message: string}>; override_config: ValidationConfig | null;
  saved: ValidationSelection | null; saved_current: boolean; unsupported: string[];
}
export interface ProviderRead { team: string; candidate?: { override_config: ValidationConfig | null } }
export type ProviderPort = (read: ProviderRead) => Promise<ProviderInspection>;
export const readProviderConfig: ProviderPort = (read) => {
  if (!inTauri()) return Promise.reject(new Error("Provider inspection needs this computer's hgs."));
  return invoke<ProviderInspection>("teams_provider_config", { read });
};
