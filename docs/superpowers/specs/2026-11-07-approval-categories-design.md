# Automatic approvals by category

Bead: zerus-sz42x. Owner request, 2026-11-07, on Settings > Automatic
approvals: "вот здесь я бы разделил по категориям. потому что я хочу
авто-подтверждение, но кроме paid usage например!" A person can auto-approve
most tool requests and keep some categories manual, for example everything
except paid usage.

Unchanged rules: automatic approvals stay a separate, off-by-default dangerous
opt-in. They grant only verified one-time native permissions (`once()` in
`client/core/src/approvals/automatic.rs`). Each grant keeps its exact
run/conversation/question identity and reserves its journal entry durably
before delivery. An unknown delivery is never replayed. Ordinary questions,
folder trust and permanent permission changes stay manual. The category check
narrows `once()`. It never widens it.

## 1. Categories

A request gets its category from the tool name in the verified native
question, as the provider reports it. Matching is exact against a fixed table.
Zerus never parses commands, paths or free text to pick a category.

Coordinator correction, 2026-11-07 (design-tima2): an eighth category,
**Other tools**, covers every verified one-time permission whose tool is not
in the table, is ambiguous or cannot be read. It is on by default, so
"everything except paid" keeps approving new or unlisted tools. Paid always
wins: a request that is paid by tool, by a paid name hint or by the cold-cache
rule also needs `paid`, whatever else it matches.

| ID | Label | Claude (`questions[0].header` = hook `tool_name`) | OpenCode 2 (permission `action`) | DeepSeek dsh (`toolName`) |
|---|---|---|---|---|
| `read` | Read files and search | Read, Glob, Grep, LS, LSP, NotebookRead | read, glob, grep, list, lsp | read, read_image, glob, grep, terminal_list, terminal_read, job_list, job_output |
| `edit` | Edit files | Edit, Write, MultiEdit, NotebookEdit, EnterWorktree | edit | edit, write, apply_patch, str_replace_editor |
| `shell` | Shell commands | Bash, PowerShell, Monitor | shell, execute | bash, pwsh, terminal_open, terminal_send, terminal_signal, terminal_close, job_kill |
| `web` | Web pages | WebFetch | webfetch | web_fetch |
| `mcp` | MCP tools | `mcp__<server>__<tool>`, ListMcpResourcesTool, ReadMcpResourceTool | none (see below) | `mcp__<server>__<tool>` |
| `agents` | Subagents and skills | Agent, Task, Skill, Workflow | subagent, skill | skill, subagent, workflow, spawn_teammate, send_message, interrupt_agent, wait_agent |
| `other` | Other tools | any other verified tool | any other action, every underscored action, a card without `permission_action` | any other tool, an unreadable card |
| `paid` | Paid usage | WebSearch | websearch | web_search |

Where each name comes from:

- **Claude.** Both card builders in `src/state/claude_approval.rs` set
  `header` to the PermissionRequest hook's `tool_name`. The tool list follows
  the Claude Code tools reference (code.claude.com/docs/en/tools-reference),
  which marks Bash, Edit, Write, NotebookEdit, PowerShell, Monitor, Skill,
  WebFetch, WebSearch, Workflow and EnterWorktree as needing permission. Read
  tools ask only outside the working folder or under `ask` rules. MCP tools
  are named `mcp__server__tool` in that reference and in Claude Code's
  permission rules.
- **OpenCode 2.** Taken from the 2.2.6 bundle's `Permission.assert` calls and
  the 2.0.32 OpenAPI `Permission.Request` schema (`action`, `resources`). MCP
  tool actions are `${server}_${tool}` with no prefix. They can equal a
  built-in name that contains `_` (an MCP server `external` with a tool
  `directory` yields `external_directory`). So only underscore-free OpenCode
  actions get a specific category. `external_directory`,
  `experimental_image_generate`, `opencode_*` and all MCP tools are Other.
  Today the card has the action only as the first line of `prompt`. The CLI
  will add an exact `permission_action` field (`src/state/opencode/forms.rs`).
  A specific category needs that field, and the prompt's first line must equal
  it. A card from an older peer, without the field, is Other. Either the field
  or the prompt's first line naming a paid action (`websearch`,
  `experimental_image_generate`) also requires `paid`.
- **DeepSeek.** Our bridge (`src/dsh/bridge.mjs`) turns each `approval/request`
  into item `decision` with header `Approval required` and question
  `Allow ${toolName}?`. The dsh pipeline (`dsh-tools`, `serviceAsk`) sets
  `toolName` to the executed tool's name. Tool names are DeepSeek function
  names, and MCP tools are `mcp__<server>__<raw>`
  (`@deepseek-ai/dsh-mcp-client`). The classifier accepts only that exact
  format, with the name matching `^[A-Za-z0-9_-]{1,128}$`. Changing the
  bridge would need a DeepSeek host restart, and live hosts are never
  hot-reloaded. Parsing our own fixed format works for running hosts too.
  A card that does not match it is Other.
- **Any other tool is Other.** That covers tools missing from the table,
  ambiguous and malformed names. In Claude it includes Artifact,
  ShareOnboardingGuide, Cron*, RemoteTrigger and SendUserFile. In OpenCode it
  includes question, todowrite and every underscored action. With Other off,
  such requests stay manual.

**Shell commands are one category.** Zerus does not try to detect destructive
commands. Quoting, aliases, scripts and `eval` make text checks unreliable,
and the UI says so.

## 2. Paid usage: what it is and what reaches auto-approval

These **do** arrive as verified one-time permission requests, so this feature
can keep them manual:

- **Claude WebSearch.** It uses Anthropic's web search backend. One call can
  run up to eight searches (Claude Code tools reference, "WebSearch tool
  behavior"). On the API this costs $10 per 1,000 searches plus tokens
  (docs.claude.com pricing). Under a subscription it counts against plan
  usage.
- **OpenCode websearch.** It searches through "the user's selected search
  integration" (`websearch:provider`), an external provider. OpenCode's image
  generation (`experimental_image_generate`, billed per image) is paid as
  well. Its underscored name cannot be told apart from an MCP tool, so it is
  Other and, because its name is a paid hint, also needs `paid`.
- **DeepSeek web_search.** `@deepseek-ai/dsh-web-search-deepseek` runs
  DeepSeek's server-side search, and its documentation says: "Each search
  costs a model turn".
- **Any approval while the prompt cache is cold.** Answering resumes the
  agent, and with an expired cache the whole conversation is processed again
  at the uncached price. That is the existing warning "Resuming after an
  expired cache may process the full conversation again". Manual answers
  already ask "Submit with a cold cache?" (`questions::cold_answer`).
  Detection uses `cache::expired` on the same inspection that produced the
  question. Such a request needs `paid` in addition to its tool category: a
  cold-cache Bash request needs both `shell` and `paid`. An unknown cache
  state does not count as cold. Otherwise providers that report no cache data
  could never be auto-approved.

These **do not** arrive as one-time tool permissions, so automatic approvals
can never answer them and they stay manual by construction:

- Claude usage-limit, extra-usage and credit prompts. Claude cards are built
  only from a PermissionRequest hook tool request, which needs a `tool_name`.
- Provider sign-in, model or plan switches, compaction and clear actions.
- OpenCode and DeepSeek have no credit-prompt permission action.

MCP tools that bill per call cannot be told apart from free ones, so the MCP
category's description says some servers bill per call.

## 3. Defaults and migration

- The master switch keeps its key, its default (off) and its confirmation
  dialog. The category choices are separate. They can be set while the switch
  is off and take effect only while it is on.
- Default categories: all except `paid`, as in the owner's example. `other`
  is on by default.
- Migration: existing enabled users get the default (all except `paid`), not
  "all on". This matches what the owner asked for and is the narrower choice.
  The existing warning already tells people paid usage is risky. With Other
  on, an upgrade only stops paid and cold-cache requests from auto-approving.
- Category flags are stored per category, so a category added later defaults
  to off unless it is explicitly given another default.

## 4. Where the check runs (fail closed)

`automatic::question(target, details, allowed, now)` returns the first
pending question that passes `once()` and whose required categories are all
enabled, together with those categories. A request in a manual category is
skipped. Another pending request of the same session may still be approved.
Then:

1. The worker reads the current category set again just before the journal
   `begin` and re-runs the check. Nothing is reserved if it no longer passes.
2. `begin` carries `categories` (for example `["shell","paid"]`).
3. After the reservation, the pre-delivery `current` check also re-checks the
   categories. If the request is no longer allowed, the entry finishes as
   `cancelled` and nothing is sent, exactly like turning the switch off today.

The window worker (`client/src-tauri/src/approvals/automatic.rs`) and the
background service (`src/desktop/delivery.rs`) both call this core function.

## 5. Storage and sync

- **Window mode (default for the Tauri GUI).** Settings keys
  `automaticApprovals/category/<id>`, of kind `Flag(default)`, live in the same
  per-computer store and scope as `automaticApprovals/enabled`. They are never
  seeded from Qt. The worker reads them on every attempt.
- **Background service mode (`ZERUS_CLIENT_DESKTOP_SERVICE=1`).** The service
  owns the policy. Schema leaf 33 adds
  `desktop_delivery_policy.categories`, a JSON array of IDs that defaults to
  all except `paid`; a malformed value fails closed as
  `approval_policy_unavailable`. The authenticated `automatic_approvals`
  request gains an optional `categories` field holding the full enabled set,
  and the receipt and fingerprint include it. A category request must carry
  the current durable `enabled` value, otherwise it is refused as
  `approval_policy_changed`, so a category change can never turn the switch
  on. As with disable, a change that only removes categories is accepted
  whenever the authenticated route and store are writable. A change that adds
  a category while approvals are on needs full readiness. A revision change
  cancels in-flight inspections, as it does today. The status projection adds
  `automatic_approval_categories`. `settings_set` refuses the category keys in
  service mode.

## 6. UI and journal

- Under the master switch is a "Categories" list with one switch per category
  (label plus a one-line description, using the existing `Switch` and
  keyboard handling). The descriptions say what is included. Shell commands
  includes any command, including ones that delete data. MCP tools notes that
  some servers bill per call. Other tools says it includes tools Zerus cannot
  classify, possibly paid ones. Paid usage covers web search, image
  generation and cold-cache resumes, and says it is needed even when another
  category matches. The group's own hint explains categories; the existing
  switch hint, warning and confirmation texts stay as reviewed, with their
  translations. docs/reference.md lists the final categories.
  The Russian catalog is regenerated. No dot separators.
- Journal: `begin` stores `categories` inside the existing JSON `question`
  column (no journal schema change). `list` returns it through
  `json_extract`, and `detail` shows it. The journal table gains a Category
  column. Older CLIs ignore the field and show it as empty. Activity is not
  changed, because it is not cheap there.

## 7. Delivery slices

1. **PR 1 (zerus-sz42x.1).** Core classifier and policy, OpenCode
   `permission_action`, window worker, settings keys, UI, journal category and
   i18n. Until PR 2, service mode enforces the default categories and shows
   them read-only with a note.
2. **PR 2 (zerus-sz42x.2).** Background service policy: schema leaf, request
   field, receipts, readiness rules, status projection and UI wiring.

## 8. Tests

- Core: the real card shapes from each provider (Claude hook and TUI cards,
  the OpenCode 2 `permissions.json` fixture card, the dsh bridge card) for
  every table row. Also: unknown tool, underscored OpenCode action, missing
  `permission_action`, a prompt that does not match it, malformed dsh
  question, MCP name forms, and a cold cache adding `paid`.
- Window worker, with fake calls: a manual category makes no journal `begin`;
  turning a category off between selection and `begin` makes no `begin`;
  turning it off after reservation finishes as `cancelled`; the `begin`
  payload carries `categories`.
- Service: schema migration and its default, receipts and fingerprints with
  categories, narrowing accepted while passive, widening refused while
  degraded, and a stale `enabled` value refused.
- Frontend: the category list, service intent and vitest; i18n `--check`,
  `check-ui-separators.py` and `check-source-privacy.py`.
