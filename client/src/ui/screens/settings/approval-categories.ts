import { trNoop } from "../../../i18n";

// Automatic-approval categories (zerus-sz42x). The native worker classifies
// each verified one-time permission by its tool name
// (client/core/src/approvals/automatic/categories.rs); this list only names
// the per-computer switches, in the core's order, with the core's defaults.

export interface ApprovalCategory { id: string; key: string; label: string; hint: string; on: boolean }

const category = (id: string, label: string, hint: string, on = true): ApprovalCategory =>
  ({ id, key: `automaticApprovals/category/${id}`, label, hint, on });

export const APPROVAL_CATEGORIES: readonly ApprovalCategory[] = [
  category("read", trNoop("Read files and search"), trNoop("Read files, list folders and search file contents.")),
  category("edit", trNoop("Edit files"), trNoop("Create, change or delete files.")),
  category("shell", trNoop("Shell commands"), trNoop("Run any command in a shell or terminal, including commands that delete data. Zerus cannot tell safe commands from destructive ones.")),
  category("web", trNoop("Web pages"), trNoop("Fetch web pages and other addresses.")),
  category("mcp", trNoop("MCP tools"), trNoop("Tools and resources of connected MCP servers. Some servers bill per call.")),
  category("agents", trNoop("Subagents and skills"), trNoop("Start subagents, workflows and skills, and message other agents.")),
  category("other", trNoop("Other tools"), trNoop("Tools Zerus cannot classify, such as new or unlisted ones. Some of them may be paid.")),
  category("paid", trNoop("Paid usage"), trNoop("Web searches, image generation and other tools billed per use, and any request answered after the prompt cache expired, when the agent processes the whole conversation again. Paid requests need this category even when another one matches."), false),
];

export const CATEGORIES_HINT = trNoop("Requests in a category that is off wait for you. The categories apply only while automatic approvals are on.");
export const SERVICE_CATEGORIES_UNKNOWN = trNoop("The background service has not reported its approval categories. They can be changed once it does.");

/** The labels of a journal entry's category IDs, in list order; unknown IDs are left out. */
export const categoryLabels = (ids: readonly string[] | undefined) =>
  APPROVAL_CATEGORIES.filter((c) => ids?.includes(c.id)).map((c) => c.label);
