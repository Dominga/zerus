//! Automatic-approval categories (zerus-sz42x, design
//! docs/superpowers/specs/2026-11-07-approval-categories-design.md). A
//! request's category comes only from the tool name in its verified native
//! question, matched exactly against a fixed per-provider table. A tool that
//! is not in the table, is ambiguous or cannot be read is Other. Paid usage
//! always wins: a paid tool, a paid name hint or a cold prompt cache also
//! requires Paid, whatever else the request matches.
use serde_json::Value;

/// One category, in the order the Settings page lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Read,
    Edit,
    Shell,
    Web,
    Mcp,
    Agents,
    Other,
    Paid,
}

impl Category {
    pub const ALL: [Category; 8] = [
        Category::Read,
        Category::Edit,
        Category::Shell,
        Category::Web,
        Category::Mcp,
        Category::Agents,
        Category::Other,
        Category::Paid,
    ];

    /// The stable ID used in settings, the service policy and the journal.
    pub fn id(self) -> &'static str {
        match self {
            Category::Read => "read",
            Category::Edit => "edit",
            Category::Shell => "shell",
            Category::Web => "web",
            Category::Mcp => "mcp",
            Category::Agents => "agents",
            Category::Other => "other",
            Category::Paid => "paid",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|category| category.id() == id)
    }

    /// The per-computer setting that keeps this category on or off.
    pub fn setting(self) -> &'static str {
        match self {
            Category::Read => "automaticApprovals/category/read",
            Category::Edit => "automaticApprovals/category/edit",
            Category::Shell => "automaticApprovals/category/shell",
            Category::Web => "automaticApprovals/category/web",
            Category::Mcp => "automaticApprovals/category/mcp",
            Category::Agents => "automaticApprovals/category/agents",
            Category::Other => "automaticApprovals/category/other",
            Category::Paid => "automaticApprovals/category/paid",
        }
    }

    /// Every category except paid usage is on until the person changes it.
    pub fn default_on(self) -> bool {
        self != Category::Paid
    }

    /// The label the journal detail shows.
    pub fn label(self) -> &'static str {
        match self {
            Category::Read => "Read files and search",
            Category::Edit => "Edit files",
            Category::Shell => "Shell commands",
            Category::Web => "Web pages",
            Category::Mcp => "MCP tools",
            Category::Agents => "Subagents and skills",
            Category::Other => "Other tools",
            Category::Paid => "Paid usage",
        }
    }

    fn bit(self) -> u8 {
        1 << Self::ALL.iter().position(|c| *c == self).unwrap_or(0)
    }
}

/// A set of categories.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Set(u8);

impl Set {
    pub const EMPTY: Set = Set(0);

    /// All except paid usage: the default, and what an enabled switch from
    /// before categories migrates to.
    pub fn defaults() -> Self {
        Self::from_fn(Category::default_on)
    }

    pub fn from_fn(mut on: impl FnMut(Category) -> bool) -> Self {
        Category::ALL
            .into_iter()
            .filter(|category| on(*category))
            .fold(Set::EMPTY, Set::with)
    }

    /// The categories a settings store keeps on.
    pub fn from_settings(settings: &crate::settings::Settings) -> Self {
        Self::from_fn(|category| settings.flag(category.setting()))
    }

    pub fn with(self, category: Category) -> Self {
        Set(self.0 | category.bit())
    }

    pub fn contains(self, category: Category) -> bool {
        self.0 & category.bit() != 0
    }

    /// Whether this set holds every category of `other`.
    pub fn contains_all(self, other: Set) -> bool {
        self.0 & other.0 == other.0
    }

    /// A set from category IDs: None for an unknown or repeated ID.
    pub fn from_ids<S: AsRef<str>>(ids: &[S]) -> Option<Self> {
        ids.iter().try_fold(Set::EMPTY, |set, id| {
            Category::from_id(id.as_ref())
                .filter(|c| !set.contains(*c))
                .map(|c| set.with(c))
        })
    }

    /// Whether every category of `required` is in this set. An empty
    /// requirement is never met: nothing is approved without a category.
    pub fn permits(self, required: Set) -> bool {
        required.0 != 0 && self.0 & required.0 == required.0
    }

    pub fn categories(self) -> impl Iterator<Item = Category> {
        Category::ALL
            .into_iter()
            .filter(move |category| self.contains(*category))
    }

    pub fn ids(self) -> Vec<&'static str> {
        self.categories().map(Category::id).collect()
    }
}

/// What the tool name says: its category, and whether it also hints at
/// paid usage (a paid tool, or an ambiguous name equal to a paid tool's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tool {
    pub category: Category,
    pub paid: bool,
}

impl Tool {
    fn of(category: Category) -> Self {
        Tool {
            category,
            paid: category == Category::Paid,
        }
    }
    fn other() -> Self {
        Tool::of(Category::Other)
    }
}

/// Native function-style tool names: no spaces, punctuation or controls.
fn plain(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// `mcp__<server>__<tool>`, as Claude Code and dsh name MCP tools.
fn mcp(name: &str) -> bool {
    plain(name)
        && name
            .strip_prefix("mcp__")
            .and_then(|rest| rest.split_once("__"))
            .is_some_and(|(server, tool)| !server.is_empty() && !tool.is_empty())
}

/// Claude Code's built-in tool names (code.claude.com/docs/en/tools-reference).
fn claude(name: &str) -> Tool {
    let category = match name {
        "Read" | "Glob" | "Grep" | "LS" | "LSP" | "NotebookRead" => Category::Read,
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" | "EnterWorktree" => Category::Edit,
        "Bash" | "PowerShell" | "Monitor" => Category::Shell,
        "WebFetch" => Category::Web,
        "ListMcpResourcesTool" | "ReadMcpResourceTool" => Category::Mcp,
        "Agent" | "Task" | "Skill" | "Workflow" => Category::Agents,
        "WebSearch" => Category::Paid,
        _ if mcp(name) => Category::Mcp,
        _ => Category::Other,
    };
    Tool::of(category)
}

/// OpenCode 2 permission actions that are paid usage. The image generator's
/// underscored name cannot be told from an MCP tool, so it stays Other, but
/// its name still requires Paid.
const OPENCODE_PAID: [&str; 2] = ["websearch", "experimental_image_generate"];

/// OpenCode 2 permission actions. MCP tools ask as `${server}_${tool}` with no
/// prefix and can equal an underscored built-in name, so only underscore-free
/// actions get a specific category.
fn opencode(action: &str) -> Tool {
    let category = if action.contains('_') || !plain(action) {
        Category::Other
    } else {
        match action {
            "read" | "glob" | "grep" | "list" | "lsp" => Category::Read,
            "edit" => Category::Edit,
            "shell" | "execute" => Category::Shell,
            "webfetch" => Category::Web,
            "subagent" | "skill" => Category::Agents,
            "websearch" => Category::Paid,
            _ => Category::Other,
        }
    };
    Tool {
        category,
        paid: category == Category::Paid || OPENCODE_PAID.contains(&action),
    }
}

/// DeepSeek dsh tool names (`@deepseek-ai/dsh` tool packages).
fn dsh(name: &str) -> Tool {
    let category = match name {
        "read" | "read_image" | "glob" | "grep" | "terminal_list" | "terminal_read"
        | "job_list" | "job_output" => Category::Read,
        "edit" | "write" | "apply_patch" | "str_replace_editor" => Category::Edit,
        "bash" | "pwsh" | "terminal_open" | "terminal_send" | "terminal_signal"
        | "terminal_close" | "job_kill" => Category::Shell,
        "web_fetch" => Category::Web,
        "skill" | "subagent" | "workflow" | "spawn_teammate" | "send_message"
        | "interrupt_agent" | "wait_agent" => Category::Agents,
        "web_search" => Category::Paid,
        _ if mcp(name) => Category::Mcp,
        _ => Category::Other,
    };
    Tool::of(category)
}

/// The tool of a verified one-time permission question. Callers check
/// [`super::once`] first; this only names what is being permitted.
pub fn tool(provider: &str, question: &Value) -> Tool {
    let item = &question["questions"][0];
    match provider {
        // Both Claude card builders put the hook's `tool_name` in `header`.
        "claude" => match item["header"].as_str() {
            Some(name) if plain(name) => claude(name),
            _ => Tool::other(),
        },
        // The CLI copies the native action into `permission_action`; the
        // prompt's first line, written from the same action, must agree.
        "opencode" => {
            let first = item["prompt"]
                .as_str()
                .map(|prompt| prompt.split('\n').next().unwrap_or_default());
            let hinted = first.is_some_and(|line| OPENCODE_PAID.contains(&line));
            let tool = match question["permission_action"].as_str() {
                Some(action) if first == Some(action) => opencode(action),
                Some(action) => Tool {
                    paid: opencode(action).paid,
                    ..Tool::other()
                },
                None => Tool::other(),
            };
            Tool {
                paid: tool.paid || hinted,
                ..tool
            }
        }
        // src/dsh/bridge.mjs words every approval as `Allow ${toolName}?`.
        "dsh" => {
            let name = (item["id"] == "decision" && item["header"] == "Approval required")
                .then(|| item["question"].as_str())
                .flatten()
                .and_then(|text| text.strip_prefix("Allow "))
                .and_then(|text| text.strip_suffix('?'))
                .filter(|name| plain(name));
            name.map_or_else(Tool::other, dsh)
        }
        _ => Tool::other(),
    }
}

/// What a request needs regardless of time: its tool's category, plus Paid
/// for a paid tool or a paid name hint.
pub fn tool_requirement(provider: &str, question: &Value) -> Set {
    let tool = tool(provider, question);
    let set = Set::EMPTY.with(tool.category);
    if tool.paid {
        set.with(Category::Paid)
    } else {
        set
    }
}

/// The prompt-cache fields of one inspection that [`crate::cache::expired`]
/// reads, kept so the same rule can be applied again at a later time: a warm
/// cache can expire between selection and delivery.
pub fn cache_witness(details: &Value) -> Value {
    let cache = &details["session_usage"]["prompt_cache"];
    serde_json::json!({
        "cache_hint": {"status": details["cache_hint"]["status"]},
        "session_usage": {"prompt_cache": {"status": cache["status"], "expires_at": cache["expires_at"]}},
    })
}

/// Every category the request needs at `now`: its tool requirement, plus Paid
/// once the prompt cache is cold. Answering after the cache expired processes
/// the whole conversation again. An unknown cache state is not cold.
pub fn required(provider: &str, question: &Value, details: &Value, now: f64) -> Set {
    with_cache(tool_requirement(provider, question), details, now)
}

/// `tool` plus Paid when `cache` (inspection details or their witness) is
/// cold at `now`.
pub fn with_cache(tool: Set, cache: &Value, now: f64) -> Set {
    if crate::cache::expired(cache, now) {
        tool.with(Category::Paid)
    } else {
        tool
    }
}

/// The journal's text for a set: labels joined by commas.
pub fn labels(set: Set) -> String {
    set.categories()
        .map(Category::label)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests;
