//! U62: a Cargo package's module graph, read from its sources: which module
//! each file is, which modules it uses (`use`, paths, re-exports) and which
//! tests it holds. A change in a module runs its tests and, widened by the
//! provider, the tests of the modules that use it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct Module {
    /// `state::teams::ops`; empty for the crate root.
    pub path: String,
    /// Relative to the project, with `/`.
    pub file: String,
    /// The crate it belongs to: `--bin hgs`, `--lib` or `--test name`.
    pub crate_arg: String,
    /// Its tests' names below the module, such as `tests::a_case`.
    pub tests: Vec<String>,
    /// The modules it uses, by key.
    pub deps: BTreeSet<String>,
    /// Its child modules, by key.
    pub children: Vec<String>,
    /// Other files it reads at build time (`include_str!` and friends).
    pub includes: Vec<String>,
}

#[derive(Debug, Default)]
pub struct Graph {
    /// By key: `<crate_arg> <path>`.
    pub modules: BTreeMap<String, Module>,
}

pub fn key(crate_arg: &str, path: &str) -> String {
    format!("{crate_arg} {path}")
}

impl Graph {
    /// The modules that use `key`, directly or to the end.
    #[cfg(test)]
    pub fn dependants(&self, key: &str, transitive: bool) -> BTreeSet<String> {
        let mut found = BTreeSet::new();
        let mut queue = vec![key.to_owned()];
        while let Some(current) = queue.pop() {
            for (other, module) in &self.modules {
                if module.deps.contains(&current) && found.insert(other.clone()) && transitive {
                    queue.push(other.clone());
                }
            }
        }
        found.remove(key);
        found
    }

    /// The modules `key` uses, directly or to the end.
    pub fn uses(&self, key: &str, transitive: bool) -> BTreeSet<String> {
        let mut found = BTreeSet::new();
        let mut queue = vec![key.to_owned()];
        while let Some(current) = queue.pop() {
            for dep in self.modules.get(&current).map(|module| &module.deps).into_iter().flatten() {
                if found.insert(dep.clone()) && transitive {
                    queue.push(dep.clone());
                }
            }
        }
        found.remove(key);
        found
    }
}

/// The crates of the package whose manifest is `manifest` (absolute), files
/// named relative to `project`.
pub fn read(project: &Path, manifest: &Path) -> anyhow::Result<Graph> {
    let text = std::fs::read_to_string(manifest).map_err(|error| anyhow::anyhow!("Cannot read {}: {error}", manifest.display()))?;
    let value = super::config::parse_toml(&text)?;
    let dir = manifest.parent().unwrap_or(Path::new("."));
    let package = value["package"]["name"].as_str().unwrap_or_default().to_owned();
    let mut roots: Vec<(String, PathBuf)> = Vec::new();
    let lib = value["lib"]["path"].as_str().map(|path| dir.join(path)).unwrap_or_else(|| dir.join("src/lib.rs"));
    let has_lib = lib.is_file();
    if has_lib {
        roots.push(("--lib".to_owned(), lib));
    }
    let bins = value["bin"].as_array().cloned().unwrap_or_default();
    for bin in &bins {
        let name = bin["name"].as_str().unwrap_or(&package).to_owned();
        let path = bin["path"].as_str().map(|path| dir.join(path)).unwrap_or_else(|| dir.join(format!("src/bin/{name}.rs")));
        roots.push((format!("--bin {name}"), path));
    }
    if bins.is_empty() && dir.join("src/main.rs").is_file() {
        roots.push((format!("--bin {package}"), dir.join("src/main.rs")));
    }
    for entry in std::fs::read_dir(dir.join("tests")).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "rs") {
            let name = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or_default().to_owned();
            roots.push((format!("--test {name}"), path));
        }
    }
    let mut graph = Graph::default();
    let library = package.replace('-', "_");
    for (crate_arg, root) in roots {
        if !root.is_file() {
            continue;
        }
        let mut parsed = Vec::new();
        walk(project, &root, &crate_arg, "", true, &mut graph, &mut parsed);
        // A test crate reaches the library by its name, a binary by `crate`.
        let external = if crate_arg.starts_with("--test") && has_lib { Some(library.as_str()) } else { None };
        resolve(&mut graph, &crate_arg, &parsed, external);
    }
    Ok(graph)
}

/// What one file says, before its paths are resolved.
struct Parsed {
    key: String,
    /// Paths it names, each with the inline module it was named in.
    paths: Vec<(String, Vec<String>)>,
    /// `use` names in scope: name to path.
    aliases: BTreeMap<String, Vec<String>>,
    /// Re-exports: `pub use` name to path, and glob re-exports.
    exports: BTreeMap<String, Vec<String>>,
    globs: Vec<Vec<String>>,
}

fn walk(project: &Path, file: &Path, crate_arg: &str, path: &str, owns_folder: bool, graph: &mut Graph, parsed: &mut Vec<Parsed>) {
    let key = key(crate_arg, path);
    if graph.modules.contains_key(&key) {
        return;
    }
    let Ok(text) = std::fs::read_to_string(file) else {
        return;
    };
    let source = lex(&text);
    let relative = relative(project, file);
    let folder = file.parent().unwrap_or(Path::new("")).to_path_buf();
    // `foo.rs` keeps its children in `foo/`; `mod.rs`, `main.rs` and `lib.rs`
    // in their own folder.
    let child_folder = if owns_folder { folder.clone() } else { folder.join(file.file_stem().unwrap_or_default()) };
    let scan = scan(&source);
    let mut module = Module { path: path.to_owned(), file: relative, crate_arg: crate_arg.to_owned(), tests: scan.tests, ..Module::default() };
    for include in &scan.includes {
        let target = folder.join(include);
        module.includes.push(relative_lexical(project, &target));
    }
    let mut children = Vec::new();
    for declaration in &scan.declarations {
        let child_path = join(path, &declaration.inline.iter().chain([&declaration.name]).cloned().collect::<Vec<_>>().join("::"));
        let base = declaration.inline.iter().fold(child_folder.clone(), |dir, name| dir.join(name));
        let candidates = match &declaration.path_attr {
            Some(attr) => vec![(folder.join(attr), attr.ends_with("mod.rs"))],
            None => vec![(base.join(format!("{}.rs", declaration.name)), false), (base.join(&declaration.name).join("mod.rs"), true)],
        };
        if let Some((child, owns)) = candidates.into_iter().find(|(candidate, _)| candidate.is_file()) {
            children.push(super::cargo_graph::key(crate_arg, &child_path));
            walk(project, &child, crate_arg, &child_path, owns, graph, parsed);
        }
    }
    module.children = children;
    graph.modules.insert(key.clone(), module);
    parsed.push(Parsed { key, paths: scan.paths, aliases: scan.aliases, exports: scan.exports, globs: scan.globs });
}

fn resolve(graph: &mut Graph, crate_arg: &str, parsed: &[Parsed], external: Option<&str>) {
    let exports: Exports =
        parsed.iter().map(|file| (file.key.clone(), (file.exports.clone(), file.globs.clone()))).collect();
    for file in parsed {
        let module_path = graph.modules[&file.key].path.clone();
        let mut deps = BTreeSet::new();
        for (raw, inline) in &file.paths {
            let segments: Vec<String> = raw.split("::").map(str::to_owned).collect();
            let scope: Vec<String> = split(&module_path).into_iter().chain(inline.iter().cloned()).collect();
            let Some(absolute) = absolute(graph, crate_arg, &scope, &segments, &file.aliases, external) else {
                continue;
            };
            target_modules(graph, &exports, crate_arg, &absolute, 0, &mut deps);
        }
        deps.remove(&file.key);
        if let Some(module) = graph.modules.get_mut(&file.key) {
            module.deps = deps;
        }
    }
}

/// A path as absolute segments in the crate (or the library, for a test
/// crate), or None for an outside crate.
fn absolute(
    graph: &Graph,
    crate_arg: &str,
    scope: &[String],
    segments: &[String],
    aliases: &BTreeMap<String, Vec<String>>,
    external: Option<&str>,
) -> Option<Vec<String>> {
    let first = segments.first()?.as_str();
    let rest = segments[1..].to_vec();
    match first {
        "crate" => Some(rest),
        "self" => Some(scope.iter().cloned().chain(rest).collect()),
        "super" => {
            let mut base = scope.to_vec();
            let mut rest = segments;
            while rest.first().is_some_and(|segment| segment == "super") {
                base.pop();
                rest = &rest[1..];
            }
            Some(base.into_iter().chain(rest.iter().cloned()).collect())
        }
        name if external == Some(name) => Some(std::iter::once(EXTERNAL.to_owned()).chain(rest).collect()),
        name => {
            if let Some(alias) = aliases.get(name) {
                return absolute(graph, crate_arg, scope, &alias.iter().cloned().chain(rest).collect::<Vec<_>>(), &BTreeMap::new(), external);
            }
            // A child of the current module, or of the root at the root.
            let local: Vec<String> = scope.iter().cloned().chain(segments.iter().cloned()).collect();
            if graph.modules.contains_key(&key(crate_arg, &local[..scope.len() + 1].join("::"))) {
                return Some(local);
            }
            None
        }
    }
}

const EXTERNAL: &str = "\u{0}lib";

/// Each module's re-exports: names to paths, and glob re-exports.
type Exports = BTreeMap<String, (BTreeMap<String, Vec<String>>, Vec<Vec<String>>)>;

fn target_modules(
    graph: &Graph,
    exports: &Exports,
    crate_arg: &str,
    segments: &[String],
    depth: usize,
    out: &mut BTreeSet<String>,
) {
    if depth > 8 {
        return;
    }
    let (crate_arg, segments) = match segments.first() {
        Some(first) if first == EXTERNAL => ("--lib", &segments[1..]),
        _ => (crate_arg, segments),
    };
    let mut current = key(crate_arg, "");
    let mut walked: Vec<String> = Vec::new();
    for (index, segment) in segments.iter().enumerate() {
        let next: Vec<String> = walked.iter().cloned().chain([segment.clone()]).collect();
        let next_key = key(crate_arg, &next.join("::"));
        if graph.modules.contains_key(&next_key) {
            current = next_key;
            walked = next;
            continue;
        }
        // Not a module: an item of `current`, or one it re-exports.
        if let Some((names, globs)) = exports.get(&current) {
            let rest = &segments[index + 1..];
            if let Some(target) = names.get(segment) {
                let target = rebase(&walked, target);
                target_modules(graph, exports, crate_arg, &target.into_iter().chain(rest.iter().cloned()).collect::<Vec<_>>(), depth + 1, out);
            }
            for glob in globs {
                let target = rebase(&walked, glob);
                let full: Vec<String> = target.into_iter().chain([segment.clone()]).chain(rest.iter().cloned()).collect();
                target_modules(graph, exports, crate_arg, &full, depth + 1, out);
            }
        }
        break;
    }
    if graph.modules.contains_key(&current) && !walked.is_empty() {
        out.insert(current);
    }
}

/// A re-export's path as absolute segments, from the module that declares it.
fn rebase(module: &[String], path: &[String]) -> Vec<String> {
    match path.first().map(String::as_str) {
        Some("crate") => path[1..].to_vec(),
        Some("self") => module.iter().cloned().chain(path[1..].iter().cloned()).collect(),
        Some("super") => {
            let mut base = module.to_vec();
            let mut rest = path;
            while rest.first().is_some_and(|segment| segment == "super") {
                base.pop();
                rest = &rest[1..];
            }
            base.into_iter().chain(rest.iter().cloned()).collect()
        }
        _ => module.iter().cloned().chain(path.iter().cloned()).collect(),
    }
}

fn split(path: &str) -> Vec<String> {
    if path.is_empty() {
        Vec::new()
    } else {
        path.split("::").map(str::to_owned).collect()
    }
}

fn join(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_owned()
    } else {
        format!("{parent}::{child}")
    }
}

pub fn relative(project: &Path, file: &Path) -> String {
    file.strip_prefix(project).unwrap_or(file).to_string_lossy().replace('\\', "/")
}

/// `a/b/../c` as `a/c`, relative to the project.
fn relative_lexical(project: &Path, file: &Path) -> String {
    let text = relative(project, file);
    let mut parts: Vec<&str> = Vec::new();
    for part in text.split('/') {
        match part {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    parts.join("/")
}

/// Source text with comments blanked and literals kept aside: every string
/// literal's text by its start, the code with each literal as `""`.
struct Lexed {
    code: String,
    strings: BTreeMap<usize, String>,
}

fn lex(text: &str) -> Lexed {
    let bytes = text.as_bytes();
    let mut code = String::with_capacity(text.len());
    let mut strings = BTreeMap::new();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
        } else if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
            let mut depth = 0;
            while index < bytes.len() {
                if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
                    depth += 1;
                    index += 2;
                } else if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    depth -= 1;
                    index += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    index += 1;
                }
            }
            code.push(' ');
        } else if byte == b'r' && matches!(bytes.get(index + 1), Some(b'"') | Some(b'#')) && !ident_byte(bytes.get(index.wrapping_sub(1)).copied()) {
            // A raw string: r"..." or r#"..."#.
            let mut hashes = 0;
            let mut cursor = index + 1;
            while bytes.get(cursor) == Some(&b'#') {
                hashes += 1;
                cursor += 1;
            }
            if bytes.get(cursor) != Some(&b'"') {
                code.push('r');
                index += 1;
                continue;
            }
            let start = cursor + 1;
            let closing: Vec<u8> = std::iter::once(b'"').chain(std::iter::repeat_n(b'#', hashes)).collect();
            let end = find(bytes, start, &closing).unwrap_or(bytes.len());
            strings.insert(code.len(), String::from_utf8_lossy(&bytes[start..end]).into_owned());
            code.push_str("\"\"");
            index = (end + closing.len()).min(bytes.len());
        } else if byte == b'"' {
            let start = index + 1;
            let mut cursor = start;
            while cursor < bytes.len() && bytes[cursor] != b'"' {
                cursor += if bytes[cursor] == b'\\' { 2 } else { 1 };
            }
            let end = cursor.min(bytes.len());
            strings.insert(code.len(), String::from_utf8_lossy(&bytes[start..end]).into_owned());
            code.push_str("\"\"");
            index = end + 1;
        } else if byte == b'\'' {
            // A char literal ('x', '\n', '\u{..}') or a lifetime ('a).
            let close = if bytes.get(index + 1) == Some(&b'\\') {
                (index + 3..bytes.len().min(index + 14)).find(|cursor| bytes[*cursor] == b'\'')
            } else {
                let width = text[index + 1..].chars().next().map(char::len_utf8).unwrap_or(1);
                (bytes.get(index + 1 + width) == Some(&b'\'')).then_some(index + 1 + width)
            };
            match close {
                Some(close) => {
                    code.push_str("' '");
                    index = close + 1;
                }
                None => {
                    code.push('\'');
                    index += 1;
                }
            }
        } else {
            let width = text[index..].chars().next().map(char::len_utf8).unwrap_or(1);
            code.push_str(&text[index..index + width]);
            index += width;
        }
    }
    Lexed { code, strings }
}

fn find(haystack: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    (from..haystack.len().saturating_sub(needle.len() - 1)).find(|start| haystack[*start..].starts_with(needle))
}

fn ident_byte(byte: Option<u8>) -> bool {
    byte.is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

struct Declaration {
    name: String,
    /// Inline modules around it.
    inline: Vec<String>,
    path_attr: Option<String>,
}

#[derive(Default)]
struct Scan {
    declarations: Vec<Declaration>,
    tests: Vec<String>,
    paths: Vec<(String, Vec<String>)>,
    aliases: BTreeMap<String, Vec<String>>,
    exports: BTreeMap<String, Vec<String>>,
    globs: Vec<Vec<String>>,
    includes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Word(String),
    /// A string literal, by its start in the code.
    Text(usize),
    Punct(char),
    PathSep,
}

fn tokens(code: &str) -> Vec<Token> {
    let bytes = code.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80 {
            let start = index;
            while index < bytes.len() && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_' || bytes[index] >= 0x80) {
                index += 1;
            }
            out.push(Token::Word(code[start..index].to_owned()));
        } else if byte.is_ascii_digit() {
            while index < bytes.len() && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_' || bytes[index] == b'.') {
                index += 1;
            }
        } else if byte == b':' && bytes.get(index + 1) == Some(&b':') {
            out.push(Token::PathSep);
            index += 2;
        } else if byte == b'"' && bytes.get(index + 1) == Some(&b'"') {
            out.push(Token::Text(index));
            index += 2;
        } else if byte.is_ascii_whitespace() {
            index += 1;
        } else {
            out.push(Token::Punct(byte as char));
            index += 1;
        }
    }
    out
}

fn word(tokens: &[Token], index: usize) -> Option<&str> {
    match tokens.get(index) {
        Some(Token::Word(word)) => Some(word),
        _ => None,
    }
}

fn scan(source: &Lexed) -> Scan {
    let tokens = tokens(&source.code);
    let mut scan = Scan::default();
    // Inline modules: name and the brace depth inside them.
    let mut inline: Vec<(String, usize)> = Vec::new();
    let mut depth = 0usize;
    let mut path_attr: Option<String> = None;
    let mut test_attr = false;
    let mut index = 0;
    while index < tokens.len() {
        match &tokens[index] {
            Token::Punct('{') => depth += 1,
            Token::Punct('}') => {
                depth = depth.saturating_sub(1);
                if inline.last().is_some_and(|(_, inside)| *inside > depth) {
                    inline.pop();
                }
            }
            Token::Punct('#') if tokens.get(index + 1) == Some(&Token::Punct('[')) => {
                // `#[path = "x"]`, `#[test]`, `#[tokio::test]` and the like.
                let end = (index + 2..tokens.len()).find(|cursor| tokens[*cursor] == Token::Punct(']')).unwrap_or(tokens.len());
                let inside = &tokens[index + 2..end];
                if word(inside, 0) == Some("path") {
                    if let Some(Token::Text(at)) = inside.get(2) {
                        path_attr = source.strings.get(at).cloned();
                    }
                }
                let last = inside.iter().rev().find_map(|token| match token {
                    Token::Word(word) => Some(word.as_str()),
                    _ => None,
                });
                if last == Some("test") && inside.len() <= 3 {
                    test_attr = true;
                }
                index = end + 1;
                continue;
            }
            Token::Word(keyword) if keyword == "mod" => {
                if let Some(name) = word(&tokens, index + 1) {
                    let name = name.to_owned();
                    match tokens.get(index + 2) {
                        Some(Token::Punct(';')) => scan.declarations.push(Declaration {
                            name,
                            inline: inline.iter().map(|(name, _)| name.clone()).collect(),
                            path_attr: path_attr.take(),
                        }),
                        Some(Token::Punct('{')) => {
                            inline.push((name, depth + 1));
                            depth += 1;
                            index += 3;
                            path_attr = None;
                            continue;
                        }
                        _ => {}
                    }
                }
            }
            Token::Word(keyword) if keyword == "fn" && test_attr => {
                if let Some(name) = word(&tokens, index + 1) {
                    let prefix: Vec<String> = inline.iter().map(|(name, _)| name.clone()).collect();
                    scan.tests.push(prefix.into_iter().chain([name.to_owned()]).collect::<Vec<_>>().join("::"));
                }
                test_attr = false;
            }
            Token::Word(keyword) if keyword == "use" => {
                let public = index > 0
                    && (word(&tokens, index - 1) == Some("pub")
                        || (tokens.get(index - 1) == Some(&Token::Punct(')')) && (1..5).any(|back| word(&tokens, index.saturating_sub(back)) == Some("pub"))));
                let end = (index + 1..tokens.len()).find(|cursor| tokens[*cursor] == Token::Punct(';')).unwrap_or(tokens.len());
                let scope: Vec<String> = inline.iter().map(|(name, _)| name.clone()).collect();
                for (path, alias) in use_tree(&tokens[index + 1..end]) {
                    let joined = path.join("::");
                    scan.paths.push((joined, scope.clone()));
                    match alias {
                        Some(name) if name == "*" => {
                            if public {
                                scan.globs.push(path);
                            }
                        }
                        Some(name) => {
                            if public {
                                scan.exports.insert(name.clone(), path.clone());
                            }
                            scan.aliases.insert(name, path);
                        }
                        None => {}
                    }
                }
                index = end + 1;
                continue;
            }
            Token::Word(name) if name == "include_str" || name == "include_bytes" || name == "include" => {
                if tokens.get(index + 1) == Some(&Token::Punct('!')) {
                    if let Some(Token::Text(at)) = tokens.get(index + 3) {
                        if let Some(text) = source.strings.get(at) {
                            scan.includes.push(text.clone());
                        }
                    }
                }
            }
            Token::Word(first) if tokens.get(index + 1) == Some(&Token::PathSep) && (index == 0 || tokens[index - 1] != Token::PathSep) => {
                // An expression or type path: `crate::a::b`, `super::x`, `teams::ops`.
                let mut segments = vec![first.clone()];
                let mut cursor = index + 1;
                while tokens.get(cursor) == Some(&Token::PathSep) {
                    match word(&tokens, cursor + 1) {
                        Some(next) => {
                            segments.push(next.to_owned());
                            cursor += 2;
                        }
                        None => break,
                    }
                }
                scan.paths.push((segments.join("::"), inline.iter().map(|(name, _)| name.clone()).collect()));
                index = cursor;
                continue;
            }
            _ => {}
        }
        // A `#[test]` belongs to the next `fn`; any body or item end drops it.
        if matches!(tokens[index], Token::Punct('{') | Token::Punct('}') | Token::Punct(';')) {
            test_attr = false;
        }
        index += 1;
    }
    scan
}

/// A `use` tree's leaves: each path with its name in scope (`*` for a glob,
/// None for `self` of a group).
fn use_tree(tokens: &[Token]) -> Vec<(Vec<String>, Option<String>)> {
    let mut out = Vec::new();
    let mut index = 0;
    tree(tokens, &mut index, Vec::new(), &mut out);
    out
}

fn tree(tokens: &[Token], index: &mut usize, prefix: Vec<String>, out: &mut Vec<(Vec<String>, Option<String>)>) {
    let mut path = prefix;
    loop {
        match tokens.get(*index) {
            Some(Token::Word(word)) if word == "as" => {
                *index += 1;
                if let Some(Token::Word(alias)) = tokens.get(*index) {
                    out.push((path.clone(), Some(alias.clone())));
                    *index += 1;
                }
                return;
            }
            Some(Token::Word(word)) => {
                if word == "self" && !path.is_empty() {
                    let name = path.last().cloned();
                    out.push((path.clone(), name));
                    *index += 1;
                    return;
                }
                path.push(word.clone());
                *index += 1;
            }
            Some(Token::PathSep) => *index += 1,
            Some(Token::Punct('*')) => {
                *index += 1;
                out.push((path.clone(), Some("*".to_owned())));
                return;
            }
            Some(Token::Punct('{')) => {
                *index += 1;
                loop {
                    tree(tokens, index, path.clone(), out);
                    match tokens.get(*index) {
                        Some(Token::Punct(',')) => *index += 1,
                        Some(Token::Punct('}')) => {
                            *index += 1;
                            return;
                        }
                        _ => return,
                    }
                    if tokens.get(*index) == Some(&Token::Punct('}')) {
                        *index += 1;
                        return;
                    }
                }
            }
            _ => {
                if !path.is_empty() {
                    let name = path.last().cloned();
                    out.push((path, name));
                }
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (path, text) in files {
            let path = dir.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        dir
    }

    const MAIN: &str = r#"
mod state;
mod cli;
// mod commented;
fn main() { let s = "mod fake;"; cli::run(); }
#[cfg(test)]
mod tests {
    #[test]
    fn root_case() {}
}
"#;

    fn graph() -> (tempfile::TempDir, Graph) {
        let dir = package(&[
            ("Cargo.toml", "[package]\nname = \"demo\"\n"),
            ("src/main.rs", MAIN),
            ("src/cli.rs", "use crate::state::teams::{self, ops::apply as go};\npub fn run() { teams::model::Team::new(); go(); }\n"),
            ("src/state/mod.rs", "pub mod teams;\nmod store;\npub use store::Store;\n"),
            ("src/state/store.rs", "pub struct Store;\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn keeps() {}\n    #[tokio::test]\n    async fn waits() {}\n}\n"),
            ("src/state/teams.rs", "pub mod model;\npub mod ops;\n#[path = \"teams/odd_name.rs\"]\nmod odd;\n"),
            ("src/state/teams/model.rs", "pub struct Team;\nconst CONTRACT: &str = include_str!(\"../../../fixtures/contract.json\");\n"),
            ("src/state/teams/ops.rs", "use super::model::Team;\nuse crate::state::Store;\npub fn apply() {}\n#[cfg(test)]\nmod tests;\n"),
            ("src/state/teams/ops/tests.rs", "use super::*;\n#[test]\nfn applies() { let x = r#\"use crate::cli;\"#; }\n"),
            ("src/state/teams/odd_name.rs", "fn odd() { super::model::Team::new(); }\n"),
        ]);
        let graph = read(dir.path(), &dir.path().join("Cargo.toml")).unwrap();
        (dir, graph)
    }

    fn deps(graph: &Graph, path: &str) -> Vec<String> {
        graph.modules[&key("--bin demo", path)].deps.iter().map(|dep| dep.trim_start_matches("--bin demo ").to_owned()).collect()
    }

    #[test]
    fn modules_are_found_from_their_declarations_and_path_attributes() {
        let (_dir, graph) = graph();
        let paths: Vec<&str> = graph.modules.values().map(|module| module.path.as_str()).collect();
        assert_eq!(
            paths,
            vec!["", "cli", "state", "state::store", "state::teams", "state::teams::model", "state::teams::odd", "state::teams::ops", "state::teams::ops::tests"]
        );
        let odd = &graph.modules[&key("--bin demo", "state::teams::odd")];
        assert_eq!(odd.file, "src/state/teams/odd_name.rs");
        assert_eq!(graph.modules[&key("--bin demo", "state::teams::ops::tests")].file, "src/state/teams/ops/tests.rs");
        assert_eq!(graph.modules[&key("--bin demo", "state::teams::model")].includes, vec!["fixtures/contract.json"]);
    }

    #[test]
    fn tests_are_named_below_their_module() {
        let (_dir, graph) = graph();
        assert_eq!(graph.modules[&key("--bin demo", "")].tests, vec!["tests::root_case"]);
        assert_eq!(graph.modules[&key("--bin demo", "state::store")].tests, vec!["tests::keeps", "tests::waits"]);
        assert_eq!(graph.modules[&key("--bin demo", "state::teams::ops::tests")].tests, vec!["applies"]);
    }

    #[test]
    fn uses_resolve_through_aliases_relative_paths_and_re_exports() {
        let (_dir, graph) = graph();
        // `teams::model::Team` through the `self` alias, `go` to ops.
        assert_eq!(deps(&graph, "cli"), vec!["state::teams", "state::teams::model", "state::teams::ops"]);
        // `crate::state::Store` is store's, re-exported by state.
        assert_eq!(deps(&graph, "state::teams::ops"), vec!["state", "state::store", "state::teams::model"]);
        // `use super::*` in a test file is its parent; a string is no use.
        assert_eq!(deps(&graph, "state::teams::ops::tests"), vec!["state::teams::ops"]);
        assert_eq!(deps(&graph, "state::teams::odd"), vec!["state::teams::model"]);
        assert_eq!(deps(&graph, ""), vec!["cli"]);
    }

    #[test]
    fn dependants_widen_directly_or_to_the_end() {
        let (_dir, graph) = graph();
        let short = |set: BTreeSet<String>| set.into_iter().map(|dep| dep.trim_start_matches("--bin demo ").to_owned()).collect::<Vec<_>>();
        assert_eq!(short(graph.dependants(&key("--bin demo", "state::store"), false)), vec!["state", "state::teams::ops"]);
        assert_eq!(
            short(graph.dependants(&key("--bin demo", "state::store"), true)),
            vec!["", "cli", "state", "state::teams::ops", "state::teams::ops::tests"]
        );
    }
}
