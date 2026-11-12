import { tr } from "../i18n";
import type { Account, AccountsView, CatalogModel, ComputerId, Profile } from "../types";

export interface AccountRef {
  host: ComputerId;
  provider: string;
  profileId: string;
}

export type AccountSelection =
  | { kind: "default"; host: ComputerId; provider: string }
  | { kind: "profile"; ref: AccountRef };

export interface AccountOption {
  ref: AccountRef;
  groupKey: string;
  label: string;
  profileLabel: string;
  email: string;
  organization: string;
  native: boolean;
  isDefault: boolean;
  signedIn: boolean;
}

const text = (value: unknown) => typeof value === "string" ? value.trim() : "";
export const providerId = (provider: string) => provider === "deepseek" ? "dsh" : provider;

export function accountDisplayName(group: Account | undefined, profile: Profile | undefined, cachedEmail = ""): string {
  return text(group?.label)
    || text(profile?.label)
    || text(group?.usage?.identity?.email)
    || text(profile?.usage?.identity?.email)
    || text(cachedEmail)
    || text(profile?.id)
    || tr("Unnamed account");
}

export function accountGroupForProfile(view: Pick<AccountsView, "accounts">, profile: Profile | undefined): Account | undefined {
  if (!profile) return undefined;
  return view.accounts.find((group) => group.members.some((member) => member.key === profile.key))
    ?? view.accounts.find((group) => group.members.some((member) => member.host === profile.host
      && providerId(member.provider) === providerId(profile.provider) && member.id === profile.id));
}

export function accountNameForProfile(view: Pick<AccountsView, "accounts">, profile: Profile | undefined): string | undefined {
  if (!profile) return undefined;
  return accountDisplayName(accountGroupForProfile(view, profile), profile);
}

export function accountNameForId(
  view: Pick<AccountsView, "accounts">,
  host: ComputerId,
  provider: string,
  profileId: string,
): string | undefined {
  for (const group of view.accounts) {
    const profile = group.members.find((member) => member.host === host
      && providerId(member.provider) === providerId(provider) && member.id === profileId);
    if (profile) return accountDisplayName(group, profile);
  }
  return undefined;
}

export function accountOptions(
  view: Pick<AccountsView, "accounts"> | undefined,
  criteria: { host: ComputerId; provider?: string; includeNative?: boolean },
): AccountOption[] {
  const out: AccountOption[] = [];
  const seen = new Set<string>();
  for (const group of view?.accounts ?? []) {
    for (const profile of group.members) {
      const provider = providerId(profile.provider);
      if (profile.host !== criteria.host || (criteria.provider && provider !== providerId(criteria.provider))
        || (criteria.includeNative === false && profile.native) || !profile.id) continue;
      const key = `${provider}\n${profile.id}`;
      if (seen.has(key)) continue;
      seen.add(key);
      out.push({
        ref: { host: profile.host, provider, profileId: profile.id },
        groupKey: group.key,
        label: accountDisplayName(group, profile),
        profileLabel: profile.label,
        email: text(group.usage?.identity?.email) || text(profile.usage?.identity?.email),
        organization: text(group.usage?.identity?.organization) || text(profile.usage?.identity?.organization),
        native: profile.native,
        isDefault: profile.is_default,
        signedIn: profile.signed_in,
      });
    }
  }
  return out;
}

export function accountOptionLabel(option: AccountOption, siblings: readonly AccountOption[]): string {
  const collision = siblings.some((other) => other.groupKey !== option.groupKey
    && other.ref.provider === option.ref.provider && other.label === option.label);
  if (!collision) return option.label;
  return `${option.label} (${option.ref.host || tr("This computer")} / ${option.ref.profileId})`;
}

export interface EffortOption { id: string; label: string }
export interface ModelOption {
  id: string;
  label: string;
  efforts: EffortOption[];
  defaultEffort?: string;
  unavailable?: boolean;
}
export interface ModelDefaults { model: string; effort: string }
export interface ModelCatalog {
  models: ModelOption[];
  effortSupported: boolean;
  defaults?: ModelDefaults;
  providerDefaults?: ModelDefaults;
  status: "idle" | "loading" | "ready" | "error";
  error?: string;
  truncated: boolean;
}
export interface SelectionCapabilities {
  chooseAccount: boolean;
  chooseModel: boolean;
  chooseEffort: boolean;
  allowCustomModel: boolean;
}
export interface ProviderSelection {
  account: AccountSelection;
  modelId: string;
  effortId: string;
}

const record = (value: unknown): Record<string, unknown> => value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {};
const list = (value: unknown): unknown[] => Array.isArray(value) ? value : [];
const short = (value: unknown, limit: number) => {
  const found = text(value);
  return found.length <= limit && !/\p{Cc}/u.test(found) ? found : "";
};
const defaults = (value: unknown): ModelDefaults => ({
  model: short(record(value).model, 256),
  effort: short(record(value).effort, 40),
});

export function normalizeModelCatalog(
  raw: unknown,
  options: { status?: ModelCatalog["status"]; effortSupported?: boolean; truncated?: boolean; error?: string } = {},
): ModelCatalog {
  const container = record(raw);
  const rows = Array.isArray(raw) ? raw : Array.isArray(container.models) ? container.models : list(container.model_options);
  const models: ModelOption[] = [];
  const seen = new Set<string>();
  for (const entry of rows.slice(0, 256)) {
    const model = record(entry) as Partial<CatalogModel> & Record<string, unknown>;
    const id = short(model.value ?? model.id, 256);
    if (!id || seen.has(id)) continue;
    seen.add(id);
    const effortIds = list(model.effort_options ?? model.efforts);
    const effortNames = list(model.effort_labels ?? model.effortLabels);
    const efforts: EffortOption[] = [];
    for (const [index, rawEffort] of effortIds.slice(0, 64).entries()) {
      const effort = short(rawEffort, 40);
      if (!effort || efforts.some((option) => option.id === effort)) continue;
      efforts.push({ id: effort, label: short(effortNames[index], 80) || effort });
    }
    const defaultEffort = short(model.default_effort ?? model.defaultEffort, 40);
    models.push({
      id,
      label: short(model.label, 160) || id,
      efforts,
      ...(efforts.some((effort) => effort.id === defaultEffort) ? { defaultEffort } : {}),
    });
  }
  const error = options.error ?? (short(container.error, 512) || undefined);
  return {
    models,
    effortSupported: options.effortSupported ?? container.effort_supported !== false,
    ...(container.defaults && typeof container.defaults === "object" ? { defaults: defaults(container.defaults) } : {}),
    ...(container.provider_defaults && typeof container.provider_defaults === "object" ? { providerDefaults: defaults(container.provider_defaults) } : {}),
    status: options.status ?? "ready",
    ...(error ? { error } : {}),
    truncated: options.truncated ?? (container.truncated === true || container.model_options_truncated === true),
  };
}

export function modelOptions(catalog: ModelCatalog, current = ""): ModelOption[] {
  const options = [...catalog.models];
  if (current && !options.some((model) => model.id === current)) options.push({ id: current, label: current, efforts: [], unavailable: true });
  return options;
}

export function effortOptions(catalog: ModelCatalog, modelId: string, current = "", defaultLabel = tr("Provider default")): EffortOption[] {
  const model = catalog.models.find((option) => option.id === modelId);
  const options: EffortOption[] = [{ id: "", label: defaultLabel }, ...(model?.efforts ?? [])];
  if (current && !options.some((option) => option.id === current)) options.push({ id: current, label: current });
  return options;
}

export function selectAccount(state: ProviderSelection, account: AccountSelection): ProviderSelection {
  return { ...state, account, modelId: "", effortId: "" };
}

export function selectModel(catalog: ModelCatalog, modelId: string, currentEffort = ""): Pick<ProviderSelection, "modelId" | "effortId"> {
  const model = catalog.models.find((option) => option.id === modelId);
  const effortId = model?.efforts.some((effort) => effort.id === currentEffort) ? currentEffort : model?.defaultEffort ?? "";
  return { modelId, effortId };
}

export function selectionCatalogKey(scope: AccountRef & { folder?: string }): string {
  return JSON.stringify([scope.host, scope.provider, scope.profileId, scope.folder ?? ""]);
}
