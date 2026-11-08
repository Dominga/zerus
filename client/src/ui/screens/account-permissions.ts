import type { PermissionCheck, PermissionChoice, PermissionEditor } from "../../bridge";

// Task 68: the account permissions dialog's state (Qt AccountsPage::
// permissions with PermissionCatalogDialog). client/core supplies the
// choices, the editor's rows and each selection's problem; this only keeps
// which row is chosen and the editor's selection between them.

/** The editor choice before a native selection exists; never saved. */
export const EDITOR = "__editor__";

export type SaveAction = "save" | "editor" | "blocked";

/** What Save does for the chosen row: a saved choice no longer offered blocks it; the bare editor row opens the editor. */
export function saveAction(choices: readonly PermissionChoice[], value: string): SaveAction {
  const choice = choices.find((c) => c.value === value);
  if (!choice || choice.unavailable) return "blocked";
  return value === EDITOR ? "editor" : "save";
}

/** The row the dialog opens on, which is the saved one (always present: the core keeps it visible). */
export const initialChoice = (editor: PermissionEditor) => editor.initial;

/** One control's value in the editor's selection; "" is the harness's own setting. */
export function withValue(selection: Readonly<Record<string, string>>, id: string, value: string): Record<string, string> {
  const next = { ...selection };
  if (value) next[id] = value; else delete next[id];
  return next;
}

/** After the editor's Save: an empty map is the harness's own settings; otherwise the editor row carries the map. */
export function afterEditor(choices: readonly PermissionChoice[], check: PermissionCheck): { choices: PermissionChoice[]; value: string } {
  if (check.mode === "provider") return { choices: [...choices], value: "provider" };
  const at = choices.findIndex((c) => c.editor);
  const row: PermissionChoice = {
    label: check.choice_label, value: check.mode, bypass: false, unavailable: false, editor: true,
    tooltip: at >= 0 ? choices[at].tooltip : { source: "Choose each native permission option." },
    explanation: at >= 0 ? choices[at].explanation : { source: "Choose each native permission option for this account." },
  };
  const next = [...choices];
  if (at >= 0) next[at] = row; else next.push(row);
  return { choices: next, value: check.mode };
}
