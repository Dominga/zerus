import { tr } from "../../i18n";
import type { CatalogModel } from "../../types";
import { effortOptions as sharedEffortOptions, modelOptions as sharedModelOptions, normalizeModelCatalog } from "../provider-selection";
import type { CatalogProposal } from "./catalog";

// Task 50: Qt TeamCatalogEditor's role controls as pure rules - the palette's
// 16 swatches and 10 role icons, the prompt order blocks (move, include,
// switched off), the model and effort menus from the account's model
// catalog, and the per-item proposal plate with its field diff. The editor
// (TeamCatalogEditor.tsx) draws them; every change still goes through core's
// schema-checked edits.

type Item = Record<string, unknown>;
const str = (value: unknown) => (typeof value === "string" ? value : value == null ? "" : String(value));
const words = (value: unknown) => (Array.isArray(value) ? value.map(str).filter(Boolean) : []);

/** U25: the role palette's 16 colours in the prototype's order (Qt WorkspaceSkin role tokens), dark and light. */
export const SWATCHES: readonly { name: string; dark: string; light: string }[] = [
  { name: "lead", dark: "#b79cff", light: "#6f4fd8" }, { name: "architect", dark: "#8e9bff", light: "#3d4bc4" },
  { name: "backend", dark: "#6fa8ff", light: "#2f66c9" }, { name: "frontend", dark: "#f08bb4", light: "#b83a72" },
  { name: "tester", dark: "#56c8e0", light: "#137f95" }, { name: "reviewer", dark: "#d9b38c", light: "#8a5a2b" },
  { name: "security", dark: "#e48de0", light: "#a2349c" }, { name: "integrator", dark: "#a3b1c6", light: "#4d5d75" },
  { name: "researcher", dark: "#b9d36b", light: "#5f7a12" }, { name: "secretary", dark: "#7fcf9a", light: "#2f7d4a" },
  { name: "coder", dark: "#e8a75a", light: "#a35f10" }, { name: "qa", dark: "#4fd1c5", light: "#0f7a72" },
  { name: "designer", dark: "#ef7d8d", light: "#b8384b" }, { name: "leadgpt", dark: "#9aa5ff", light: "#4652c4" },
  { name: "expert", dark: "#e0b070", light: "#8a5d16" }, { name: "proexpert", dark: "#ef7d6d", light: "#b8402f" },
];

/** U25: the 10 role icons; a role stores `r_<icon>`. */
export const ROLE_ICONS = ["lead", "architect", "backend", "frontend", "tester", "reviewer", "security", "integrator", "researcher", "secretary"] as const;

/** The workspace icon each role icon is drawn with in the webview. */
export const ICON_GLYPH: Record<(typeof ROLE_ICONS)[number], string> = {
  lead: "star", architect: "layers", backend: "cpu", frontend: "monitor", tester: "check-check", reviewer: "eye",
  security: "shield-check", integrator: "git-merge", researcher: "search", secretary: "file-text",
};

/** Qt roleColorKey: the role's letters and digits, lower case. */
export const colorKey = (text: string) => text.toLowerCase().replace(/[^\p{L}\p{N}]/gu, "");

/** The swatch a role shows as chosen: its own, else the one its name keys to. */
export function chosenSwatch(role: Item): string {
  const color = str(role.color);
  if (color) return color;
  const key = colorKey(str(role.name));
  return SWATCHES.find((swatch) => swatch.name === key)?.name ?? key;
}

/** A swatch name's colour (an older catalog's #hex shows as it is). */
export function swatchColor(name: string, dark: boolean): string {
  if (name.startsWith("#")) return name;
  const swatch = SWATCHES.find((found) => found.name === name);
  return swatch ? (dark ? swatch.dark : swatch.light) : "";
}

/** Qt iconOf: the role's own icon (`r_backend`), else the one its id or name names, else none. */
export function chosenIcon(role: Item): string {
  let icon = str(role.icon);
  if (icon.startsWith("r_")) icon = icon.slice(2);
  if ((ROLE_ICONS as readonly string[]).includes(icon)) return icon;
  const key = `${colorKey(str(role.id))} ${colorKey(str(role.name))}`;
  return ROLE_ICONS.find((name) => key.includes(name)) ?? "";
}

/** The value a picked icon is stored as. */
export const iconValue = (icon: string) => `r_${icon}`;

// ---------------------------------------------------------------------------
// Prompt order

/** The blocks every role has; a note block is the role's own. */
export const FIXED_BLOCKS = ["rules", "role", "protocol", "task"] as const;
/** C1: the task block's text when the role has none. */
export const DEFAULT_TASK_TEMPLATE = "#{id} {title}\n{description}\nAcceptance:\n{criteria}\nReport with: hgs team report --task {id}";

export interface PromptBlock { key: string; title: string; off: boolean; fixed: boolean }

const noteKeys = (role: Item) => (role.notes && typeof role.notes === "object" && !Array.isArray(role.notes) ? Object.keys(role.notes as Item) : []);

/** Qt promptOrder: the role's order, else the fixed blocks then its notes; a block it lacks goes last. */
export function promptOrder(role: Item): string[] {
  const order = words(role.prompt_order);
  const out = order.length ? [...order] : [...FIXED_BLOCKS, ...noteKeys(role)];
  for (const key of [...FIXED_BLOCKS, ...noteKeys(role)]) if (!out.includes(key)) out.push(key);
  return out;
}

/** The blocks as Qt titles them, in order, each with whether it is switched off. */
export function promptBlocks(role: Item): PromptBlock[] {
  const off = new Set(words(role.prompt_off));
  return promptOrder(role).map((key) => ({
    key, off: off.has(key), fixed: (FIXED_BLOCKS as readonly string[]).includes(key),
    title: key === "rules" ? tr("Project rules") : key === "role" ? tr("Role %1", str(role.name)) : key === "protocol" ? tr("Team protocol")
      : key === "task" ? tr("Task (per attempt)") : tr("Note"),
  }));
}

/** What one change of the order writes (Qt commit): the whole order, the switched-off blocks, and the task text a role without one sends. */
export interface PromptChange { prompt_order: string[]; prompt_off: string[]; task_template?: string }

function commit(role: Item, order: string[], off: string[]): PromptChange {
  const change: PromptChange = { prompt_order: order, prompt_off: off };
  if (!str(role.task_template)) change.task_template = DEFAULT_TASK_TEMPLATE;
  return change;
}

/** Moves a block up (-1) or down (+1); none past either end. */
export function moveBlock(role: Item, index: number, by: number): PromptChange | undefined {
  const order = promptOrder(role);
  const target = index + by;
  if (index < 0 || index >= order.length || target < 0 || target >= order.length) return undefined;
  [order[index], order[target]] = [order[target], order[index]];
  return commit(role, order, words(role.prompt_off));
}

/** Drops block `from` at `to` (a drag of the row). */
export function dropBlock(role: Item, from: number, to: number): PromptChange | undefined {
  const order = promptOrder(role);
  if (from === to || from < 0 || to < 0 || from >= order.length || to >= order.length) return undefined;
  const [moved] = order.splice(from, 1);
  order.splice(to, 0, moved);
  return commit(role, order, words(role.prompt_off));
}

/** Include (on) or switch off one block. */
export function includeBlock(role: Item, key: string, on: boolean): PromptChange {
  const off = words(role.prompt_off).filter((found) => found !== key);
  if (!on) off.push(key);
  return commit(role, promptOrder(role), off);
}

// ---------------------------------------------------------------------------
// Model and effort menus

export type ModelChoice = CatalogModel;

/** Qt Teams::knownModels: offered until the account's catalog answers. */
export const KNOWN_MODELS: Record<string, string[]> = {
  claude: ["opus", "sonnet", "haiku"],
  codex: ["gpt-5.5-codex", "gpt-5.5"],
  kimi: ["kimi-code/kimi-for-coding", "kimi-code/kimi-for-coding-highspeed", "kimi-code/k3", "kimi-code/k3-256k", "kimi-code/k3.1-pro", "kimi-code/k3.1-pro-highspeed"],
};

/** The fallback efforts while no account catalog is available. */
export const EFFORTS = ["low", "medium", "high", "xhigh"];

/** Qt modelKey: one catalog per computer and account (or the agent's native account). */
export const modelKey = (role: Item) => `${str(role.computer)}\n${str(role.account) || `native-${str(role.agent) || "claude"}`}`;

/** Qt fillModels: the account's models, else the known ones; the role's own model stays offered. */
export function modelOptions(role: Item, catalog: readonly ModelChoice[] | undefined): { id: string; label: string }[] {
  if (catalog?.length) return sharedModelOptions(normalizeModelCatalog(catalog), str(role.model)).map((model) => ({ id: model.id, label: model.label }));
  const options = (KNOWN_MODELS[str(role.agent) || "claude"] ?? []).map((id) => ({ id, label: id }));
  const own = str(role.model);
  if (own && !options.some((option) => option.id === own)) options.push({ id: own, label: own });
  return options;
}

/** The selected model's variants; the static fallback is only for a missing/empty catalog. */
export function effortOptions(model: string, catalog: readonly ModelChoice[] | undefined, current = ""): string[] {
  if (!catalog?.length) {
    const options = ["", ...EFFORTS];
    if (current && !options.includes(current)) options.push(current);
    return options;
  }
  return sharedEffortOptions(normalizeModelCatalog(catalog), model, current, "—").map((effort) => effort.id);
}

/** Another agent: its first known model and the default account (spec 11.3). */
export const agentChange = (agent: string) => ({ agent, account: "", model: KNOWN_MODELS[agent]?.[0] ?? "" });

/**
 * The account model catalogs, read once per computer and account (Qt
 * requestModels): agents without a catalog read nothing; a failed read keeps
 * the known models.
 */
export function createModelCatalogs(read: (computer: string, agent: string, account: string) => Promise<ModelChoice[]>, changed: () => void = () => {}) {
  const catalogs = new Map<string, ModelChoice[]>();
  const pending = new Set<string>();
  return {
    get: (role: Item) => catalogs.get(modelKey(role)),
    request(role: Item) {
      const agent = str(role.agent) || "claude";
      if (!["claude", "codex", "kimi", "dsh"].includes(agent)) return;
      const key = modelKey(role);
      if (catalogs.has(key) || pending.has(key)) return;
      pending.add(key);
      read(str(role.computer), agent, str(role.account)).then(
        (models) => { catalogs.set(key, models); },
        () => { catalogs.set(key, []); },
      ).finally(() => { pending.delete(key); changed(); });
    },
  };
}

// ---------------------------------------------------------------------------
// Proposal plates

/** The designer's proposal on one item, if any (Qt proposalOf). */
export function proposalOf(proposals: readonly CatalogProposal[] | undefined, kind: string, id: string): CatalogProposal | undefined {
  return proposals?.find((proposal) => proposal.kind === kind && proposal.target === id);
}

/** Qt mark: NEW, REMOVED or CHANGED, with "kept" once kept; "rejected" for a rejected one. */
export function proposalMark(proposal: Pick<CatalogProposal, "action" | "state">): { text: string; tone: "accent" | "danger" | "folder" | "dim" } {
  if (proposal.state === "rejected") return { text: tr("rejected"), tone: "dim" };
  const text = proposal.action === "add" ? tr("NEW") : proposal.action === "remove" ? tr("REMOVED") : tr("CHANGED");
  return { text: proposal.state === "kept" ? tr("%1, kept", text) : text, tone: proposal.action === "add" ? "accent" : proposal.action === "remove" ? "danger" : "folder" };
}

/** The plate's answers: Keep and Reject while proposed, Undo once decided. */
export const plateAnswers = (state: string): ("keep" | "reject" | "undo")[] => (state === "proposed" ? ["reject", "keep"] : ["undo"]);

/** A field the proposal changes: its saved value and the proposed one, shown short. */
export interface FieldDiff { field: string; from: string; to: string }

const shown = (value: unknown): string => {
  if (value == null || value === "") return "";
  if (Array.isArray(value)) return value.map((entry) => (typeof entry === "object" ? JSON.stringify(entry) : str(entry))).join(", ");
  if (typeof value === "object") return JSON.stringify(value);
  return str(value);
};

/** The proposal's diff against the item as it is (Qt pipelineDiff, for every kind): only the fields that differ, by name. */
export function fieldDiff(item: Item | undefined, proposal: Pick<CatalogProposal, "action" | "value">): FieldDiff[] {
  if (proposal.action === "remove") return [];
  const value = proposal.value && typeof proposal.value === "object" ? (proposal.value as Item) : {};
  const base = item ?? {};
  const fields = [...new Set([...Object.keys(value), ...(proposal.action === "add" ? [] : Object.keys(base))])].filter((field) => field !== "id");
  return fields
    .filter((field) => JSON.stringify(base[field] ?? null) !== JSON.stringify(value[field] ?? null) && field in value)
    .sort()
    .map((field) => ({ field, from: shown(base[field]), to: shown(value[field]) }));
}
