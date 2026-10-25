//! Phase-one text-only command guard. Keep the algorithm in sync with
//! dsh/command-guard.mjs through the shared acceptance corpus. This deliberately
//! does not evaluate shell code, read environment variables or resolve paths.
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

const LIMIT: usize = 256 * 1024;
const READONLY: &[&str] = &[
    "home",
    "pid",
    "pshome",
    "host",
    "executioncontext",
    "psversiontable",
    "psculture",
    "psuiculture",
    "psedition",
    "shellid",
    "true",
    "false",
    "error",
    "consolefilename",
];
const DESTRUCTIVE: &[&str] = &[
    "rm",
    "rmdir",
    "rd",
    "del",
    "erase",
    "remove-item",
    "ri",
    "move-item",
    "mi",
    "mv",
    "move",
    "robocopy",
    "[system.io.directory]::delete",
    "[io.directory]::delete",
];
macro_rules! re {
    ($pattern:literal) => {{
        static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new($pattern).unwrap());
        &*RE
    }};
}

#[derive(Debug)]
pub(super) struct Refusal {
    pub rule: &'static str,
    pub message: String,
}

fn reject(rule: &'static str, detail: &str) -> Option<Refusal> {
    Some(Refusal {
        rule,
        message: format!("Zerus blocked this command: {detail}"),
    })
}
fn too_complex() -> Option<Refusal> {
    reject(
        "protected-path",
        "the command exceeds the text inspection limit. Split it into smaller, explicit commands.",
    )
}
fn readonly(name: &str) -> Option<Refusal> {
    reject("readonly-variable", &format!("writing PowerShell automatic variable ${name} is unsafe. Use another variable name, such as $appHome."))
}
fn boundary(text: &str) -> bool {
    [";", "\n", "|", "&", "(", ")", "{", "}"].contains(&text)
}
fn base(text: &str) -> String {
    text.replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or("")
        .trim_end_matches(".exe")
        .into()
}
fn variable(text: &str) -> Option<String> {
    re!(r"(?i)^\$(?:\{)?(?:(?:global|script|local|private):)?([a-z_][a-z0-9_]*)(?:\})?$")
        .captures(text)
        .map(|c| c[1].to_ascii_lowercase())
}

#[derive(Default)]
struct Token {
    text: String,
    quoted: bool,
}

fn lex(source: &str) -> Vec<Token> {
    let chars: Vec<_> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut word = Token::default();
    let mut active = false;
    let flush = |tokens: &mut Vec<Token>, word: &mut Token, active: &mut bool| {
        if *active {
            tokens.push(std::mem::take(word));
        }
        *active = false;
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '#' && !active {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            flush(&mut tokens, &mut word, &mut active);
            tokens.push(Token {
                text: "\n".into(),
                quoted: false,
            });
        } else if c == '"' || c == '\'' {
            active = true;
            word.quoted = true;
            i += 1;
            while i < chars.len() {
                if chars[i] == c {
                    if c == '\'' && chars.get(i + 1) == Some(&c) {
                        word.text.push(c);
                        i += 2;
                        continue;
                    }
                    break;
                }
                if c == '"'
                    && (chars[i] == '`'
                        || chars[i] == '\\'
                            && !re!(r"(?i)^(?:[a-z]:|[~$%]|\\\\)").is_match(&word.text))
                    && chars.get(i + 1) == Some(&'"')
                {
                    i += 1;
                }
                word.text.push(chars[i]);
                i += 1;
            }
        } else if c == '$' && chars.get(i + 1) == Some(&'{') {
            active = true;
            word.text.push_str("${");
            i += 2;
            while i < chars.len() && chars[i] != '}' {
                word.text.push(chars[i]);
                i += 1;
            }
            if i < chars.len() {
                word.text.push('}');
            }
        } else if c == '`' && i + 1 < chars.len() {
            if chars.get(i + 1) == Some(&'\r') {
                i += 1;
            }
            if chars.get(i + 1) == Some(&'\n') {
                i += 1;
            } else if i + 1 < chars.len() {
                i += 1;
                active = true;
                word.text.push(chars[i]);
            }
        } else if c == '\\' && chars.get(i + 1) == Some(&'\n') {
            i += 1;
        } else if ";|&(){}=,\n".contains(c) {
            flush(&mut tokens, &mut word, &mut active);
            tokens.push(Token {
                text: c.to_string(),
                quoted: false,
            });
        } else if c.is_whitespace() {
            flush(&mut tokens, &mut word, &mut active);
        } else {
            active = true;
            word.text.push(c);
        }
        i += 1;
    }
    flush(&mut tokens, &mut word, &mut active);
    tokens
}

fn normalize(value: &str) -> String {
    let path = value.replace('\\', "/").to_lowercase();
    let path = re!(r"^//[?.]/").replace(&path, "");
    let windows = re!(r"^[a-z]:").is_match(&path);
    let mut parts: Vec<&str> = Vec::new();
    for part in path.trim_end_matches('/').split('/') {
        if part == "." {
            continue;
        }
        if part == ".."
            && parts
                .last()
                .is_some_and(|p| !p.is_empty() && !p.ends_with(':'))
        {
            parts.pop();
        } else if part != ".." {
            let part = if windows {
                part.trim_end_matches(['.', ' '])
            } else {
                part
            };
            if !part.is_empty() || parts.is_empty() || path.starts_with("//") && parts.len() < 2 {
                parts.push(part);
            }
        }
    }
    parts.join("/")
}

fn protected_path(value: &str, home: &str) -> bool {
    let p = value.replace('\\', "/").to_lowercase();
    let p = re!(r"^(?:~|\$(?:\{(?:(?:env|global|script|local):)?(?:home|userprofile)\}|(?:(?:env|global|script|local):)?(?:home|userprofile))|%(?:userprofile|home)%|%homedrive%%homepath%|\$env:homedrive\$env:homepath)($|/)")
        .replace(&p, |c: &regex::Captures<'_>| format!("{home}{}", &c[1]));
    let p = re!(r"^(?:\$\{?env:(?:windir|systemroot)\}?|%(?:windir|systemroot)%)($|/)")
        .replace(&p, |c: &regex::Captures<'_>| format!("c:/windows{}", &c[1]));
    let mut p = normalize(&p);
    while p.ends_with("/*") {
        p.truncate(p.len() - 2);
    }
    if p.is_empty() || re!(r"^[a-z]:$").is_match(&p) {
        return true;
    }
    if re!(r"^//[^/]+/[^/]+$").is_match(&p) {
        return true;
    }
    if [
        "/bin",
        "/boot",
        "/dev",
        "/etc",
        "/home",
        "/lib",
        "/lib64",
        "/opt",
        "/proc",
        "/root",
        "/run",
        "/sbin",
        "/sys",
        "/usr",
        "/var",
        "/users",
        "/system",
        "/library",
        "/applications",
    ]
    .contains(&p.as_str())
        || re!(r"^/(?:home|users)/[^/]+$").is_match(&p)
    {
        return true;
    }
    if re!(r"^/(?:etc|usr|bin|sbin|boot|lib|lib64|system)(?:/|$)").is_match(&p) {
        return true;
    }
    if re!(r"^[a-z]:/(?:windows|program files(?: \(x86\))?|programdata)(?:/|$)").is_match(&p)
        || re!(r"^[a-z]:/users(?:/[^/]+)?$").is_match(&p)
    {
        return true;
    }
    if p == home {
        return true;
    }
    if let Some(tail) = p.strip_prefix(&format!("{home}/")) {
        if !tail.contains('/') && tail.contains(['*', '?']) {
            return true;
        }
        if re!(r"^(?:\.ssh|\.gnupg|\.aws|\.azure|\.config|\.local|\.claude|\.codex|\.dsh|appdata)(?:/|$)").is_match(tail)
            || ["documents", "desktop", "downloads", "pictures", "music", "videos", "library", ".cache"].contains(&tail) { return true; }
    }
    false
}

fn assigned_value(assigned: &HashMap<String, bool>, name: &str) -> Option<bool> {
    assigned
        .get(&format!("ps:{}", name.to_ascii_lowercase()))
        .or_else(|| assigned.get(&format!("sh:{name}")))
        .copied()
}

fn path_rule(value: &str, home: &str, assigned: &HashMap<String, bool>) -> Option<&'static str> {
    if protected_path(value, home) {
        return Some("protected-path");
    }
    if let Some(c) =
        re!(r"(?i)^(?:\$\{([a-z_][a-z0-9_]*)\}|\$([a-z_][a-z0-9_]*)|%([a-z_][a-z0-9_]*)%)($|[\\/])")
            .captures(value)
    {
        let name = c
            .get(1)
            .or_else(|| c.get(2))
            .or_else(|| c.get(3))
            .unwrap()
            .as_str();
        if assigned_value(assigned, name) == Some(false) {
            return Some("protected-path");
        }
    }
    if let Some(c) = re!(r"(?i)^(\$(?:\{([a-z_][a-z0-9_]*)(:\?)?\}|([a-z_][a-z0-9_]*))|%([a-z_][a-z0-9_]*)%)[\\/]\**[\\/]*$").captures(value) {
        let name = c.get(2).or_else(|| c.get(4)).or_else(|| c.get(5)).unwrap().as_str();
        if c.get(3).is_none() && assigned_value(assigned, name).is_none() { return Some("empty-variable"); }
    }
    None
}

fn inspect(command: &str, home: &str, depth: usize) -> Option<Refusal> {
    if depth > 8 {
        return too_complex();
    }
    let tokens = lex(command);
    let mut assigned = HashMap::new();
    let mut flow = false;
    let (mut head, mut param_depth, mut nesting) = (true, 0, 0usize);
    for (i, t) in tokens.iter().enumerate() {
        let s = t.text.to_lowercase();
        let next = tokens.get(i + 1).map(|t| t.text.as_str()).unwrap_or("");
        let typed = re!(r"^(?:\[[^\]]+\])+").replace(&s, "");
        let v = (!t.quoted)
            .then(|| {
                variable(
                    typed
                        .trim_start_matches("++")
                        .trim_start_matches("--")
                        .trim_end_matches(['+', '*', '/', '%', '-']),
                )
            })
            .flatten();
        let compound = ["+", "-", "*", "/", "%"].contains(&next)
            && tokens.get(i + 2).is_some_and(|t| t.text == "=");
        let write = next == "=" && tokens.get(i + 2).is_none_or(|t| t.text != "=")
            || compound
            || ["++", "--"]
                .iter()
                .any(|op| s.starts_with(op) || s.ends_with(op));
        let name = v.clone().or_else(|| {
            (!t.quoted)
                .then(|| variable(s.trim_end_matches(['+', '-'])))
                .flatten()
        });
        if let Some(name) = name {
            if READONLY.contains(&name.as_str())
                && (write || next.eq_ignore_ascii_case("in") || param_depth != 0)
            {
                return readonly(&name);
            }
        }
        if !t.quoted && (next == "=" || compound) {
            if let Some(key) = v.map(|v| format!("ps:{v}")).or_else(|| {
                re!(r"^[a-z_][a-z0-9_]*$")
                    .is_match(&s)
                    .then(|| format!("sh:{}", t.text))
            }) {
                assigned.remove(&key);
                if head
                    && !compound
                    && !s.ends_with(['+', '*', '/', '%', '-'])
                    && !flow
                    && nesting == 0
                    && tokens
                        .get(i + 2)
                        .is_some_and(|v| !v.text.is_empty() && !v.text.contains(['$', '%', '`']))
                    && tokens.get(i + 3).is_none_or(|t| boundary(&t.text))
                {
                    assigned.insert(key, !protected_path(&tokens[i + 2].text, home));
                }
            }
        }
        if !t.quoted && boundary(&s) {
            head = true;
            if ["(", "{"].contains(&s.as_str()) {
                nesting += 1;
                assigned.clear();
            }
            if [")", "}"].contains(&s.as_str()) {
                nesting = nesting.saturating_sub(1);
                assigned.clear();
                if param_depth > nesting {
                    param_depth = 0;
                }
            }
            continue;
        }
        if head && s == "param" && next == "(" {
            param_depth = nesting + 1;
        }
        if !head {
            continue;
        }
        let cmd = base(&s);
        if [
            "if", "elif", "for", "foreach", "while", "until", "case", "function", "try", "catch",
        ]
        .contains(&cmd.as_str())
        {
            flow = true;
            assigned.clear();
        }
        if [
            "set-variable",
            "sv",
            "new-variable",
            "nv",
            "clear-variable",
            "clv",
            "remove-variable",
            "rv",
            "unset",
            "read",
        ]
        .contains(&cmd.as_str())
        {
            assigned.clear();
        }
        if ["sudo", "command", "exec", "env", "then", "do", "else"].contains(&cmd.as_str())
            || s.starts_with('-')
        {
            continue;
        }
        head = false;
        let mut end = i + 1;
        if cmd.ends_with("::delete") && next == "(" {
            end += 1;
        }
        let start = end;
        while end < tokens.len() && (tokens[end].quoted || !boundary(&tokens[end].text)) {
            end += 1;
        }
        let args = &tokens[start..end];
        if ["sh", "bash", "zsh", "dash", "pwsh", "powershell", "cmd"].contains(&cmd.as_str()) {
            if let Some(at) = args
                .iter()
                .position(|a| re!(r"(?i)^(?:-c|-command|/c|-lc|-ic)$").is_match(&a.text))
            {
                let nested = args[at + 1..]
                    .iter()
                    .map(|a| a.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                if let Some(result) = inspect(&nested, home, depth + 1) {
                    return Some(result);
                }
            }
        }
        if ["set-variable", "sv", "new-variable", "nv"].contains(&cmd.as_str()) {
            let at = args
                .iter()
                .position(|a| re!(r"(?i)^-n(?:ame)?$").is_match(&a.text))
                .map_or(0, |at| at + 1);
            if let Some(arg) = args.get(at) {
                let name = arg.text.to_lowercase();
                let name = re!(r"^(?:global|script|local|private):").replace(&name, "");
                if READONLY.contains(&name.as_ref()) {
                    return readonly(&name);
                }
            }
        }
        if !DESTRUCTIVE.contains(&cmd.as_str()) {
            continue;
        }
        let mut paths = Vec::new();
        let mut a = 0;
        while a < args.len() {
            let value = re!(r"(?i)^-(?:literalpath|path):").replace(&args[a].text, "");
            a += 1;
            if re!(r"(?i)^-(?:destination|filter|include|exclude|erroraction|warningaction|ev|ea)$")
                .is_match(&value)
            {
                a += 1;
                continue;
            }
            if value == ","
                || value == "="
                || value.starts_with('-')
                || re!(r"(?i)^/(?:s|q|f|mir|purge|e)$").is_match(&value)
            {
                continue;
            }
            paths.push(value);
        }
        let targets: &[_] = if cmd == "robocopy" {
            if args
                .iter()
                .any(|a| re!(r"(?i)^/(?:mir|purge)$").is_match(&a.text))
            {
                paths.get(1..2).unwrap_or(&[])
            } else {
                &[]
            }
        } else if ["mv", "move", "move-item", "mi"].contains(&cmd.as_str())
            && paths.len() > 1
            && !args
                .iter()
                .any(|a| a.text.eq_ignore_ascii_case("-destination"))
        {
            &paths[..paths.len() - 1]
        } else {
            &paths
        };
        for target in targets {
            if let Some(rule) = path_rule(target, home, &assigned) {
                return reject(
                    rule,
                    if rule == "empty-variable" {
                        "a deletion path can become a filesystem root when its variable is empty. Assign a nonempty literal path first or use an explicit ${VAR:?} check."
                    } else {
                        "deleting, moving, or mirroring a protected home, root, system, or profile directory is unsafe. Target an explicit disposable subdirectory instead."
                    },
                );
            }
        }
    }
    None
}

pub(super) fn check(command: &str, home: &str) -> Option<Refusal> {
    if command.len() > LIMIT {
        return too_complex();
    }
    inspect(command, &normalize(home), 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn acceptance_corpus() {
        let corpus: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/command-guard.json")).unwrap();
        let home = corpus["home"].as_str().unwrap();
        for row in corpus["blocked"].as_array().unwrap() {
            let command = row[1].as_str().unwrap();
            let refusal = check(command, home).unwrap_or_else(|| panic!("allowed: {command}"));
            assert_eq!(refusal.rule, row[0].as_str().unwrap(), "{command}");
            assert!(refusal.message.starts_with("Zerus blocked this command:"));
        }
        for row in corpus["allowed"].as_array().unwrap() {
            let command = row.as_str().unwrap();
            assert!(check(command, home).is_none(), "refused: {command}");
        }
    }
    #[test]
    fn bounded_work() {
        assert_eq!(
            check(&"x".repeat(LIMIT + 1), "/home/example").unwrap().rule,
            "protected-path"
        );
        assert_eq!(
            check(
                &format!("echo {}; rm -rf ~", "x".repeat(17000)),
                "/home/example"
            )
            .unwrap()
            .rule,
            "protected-path"
        );
        assert_eq!(
            check(
                &format!("{}echo safe", "sh -c ".repeat(10)),
                "/home/example"
            )
            .unwrap()
            .rule,
            "protected-path"
        );
    }

    #[test]
    fn supplied_home_is_used_on_every_platform() {
        for home in ["/home/alice", "/Users/alice", r"D:\profiles\fixture"] {
            assert_eq!(
                check(&format!("Remove-Item '{home}' -Recurse"), home)
                    .unwrap()
                    .rule,
                "protected-path"
            );
            assert!(check("rm -rf ~/.cache/pip", home).is_none());
        }
    }
}
