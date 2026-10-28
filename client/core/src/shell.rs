//! Shell quoting for copied and displayed `hgs` commands: a port of the Qt
//! GUI's `SessionCommands` (`tray/src/SessionCommands.h`) and
//! `WindowsCommandLine` (`tray/src/WindowsCommandLine.h`), so a copied command
//! addresses the same CLI and session as the terminal action.
use serde::{Deserialize, Serialize};

/// The shell a copied command is written for: POSIX sh on Unix, PowerShell
/// (the Windows Terminal default) on Windows. A parameter rather than a
/// `cfg` in the formatting, so both forms are tested on every platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shell {
    Posix,
    PowerShell,
}

/// PowerShell on Windows, POSIX elsewhere (`SessionCommands::currentShell`).
pub fn current_shell() -> Shell {
    if cfg!(windows) {
        Shell::PowerShell
    } else {
        Shell::Posix
    }
}

fn all_in(value: &str, allowed: impl Fn(char) -> bool) -> bool {
    !value.is_empty() && value.chars().all(allowed)
}

/// A POSIX sh word: bare when it only has safe characters, else single-quoted.
pub fn shell_quote(value: &str) -> String {
    let safe = |c: char| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c);
    if all_in(value, safe) {
        return value.to_owned();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// A PowerShell literal: single quotes, with every quote PowerShell accepts as
/// a single quote doubled. Bare words exclude `@` (splatting), commas, `$`,
/// backticks, parentheses and braces.
pub fn power_shell_quote(value: &str) -> String {
    let safe = |c: char| c.is_ascii_alphanumeric() || "_./\\:=+-".contains(c);
    if all_in(value, safe) {
        return value.to_owned();
    }
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('\'');
    for c in value.chars() {
        escaped.push(c);
        if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}') {
            escaped.push(c);
        }
    }
    escaped.push('\'');
    escaped
}

/// Windows PowerShell 5.1 puts a native argument with a space in double quotes
/// without escaping quotes or a final backslash in it, and drops an empty one;
/// PowerShell 7.3 and later pass such arguments correctly, so no quoting suits
/// both (`powerShellPassesUnchanged`).
fn power_shell_passes_unchanged(value: &str) -> bool {
    if value.is_empty() || value.contains('"') {
        return false;
    }
    let spaced = value.contains([' ', '\t']);
    !(spaced && value.ends_with('\\'))
}

/// One argument as `QProcess` writes it into a Windows command line
/// (`WindowsCommandLine::argument`): a quote gets a backslash and doubles the
/// backslashes before it; an empty value or one with a space or tab is quoted,
/// its trailing backslashes outside the closing quote.
fn windows_argument(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    let mut backslashes = 0usize;
    for c in value.chars() {
        if c == '"' {
            escaped.extend(std::iter::repeat_n('\\', backslashes + 1));
        }
        backslashes = if c == '\\' { backslashes + 1 } else { 0 };
        escaped.push(c);
    }
    if !(escaped.is_empty() || escaped.contains([' ', '\t'])) {
        return escaped;
    }
    let kept = escaped.trim_end_matches('\\').len();
    format!("\"{}\"{}", &escaped[..kept], &escaped[kept..])
}

/// `WindowsCommandLine::build` with an empty program: every argument is
/// preceded by a space.
fn windows_command_line(arguments: &[String]) -> String {
    arguments
        .iter()
        .map(|argument| format!(" {}", windows_argument(argument)))
        .collect()
}

/// The command line for `hgs` with `arguments` in `shell`; an empty `hgs`
/// means the bare name found on PATH.
pub fn command(hgs: &str, arguments: &[String], shell: Shell) -> String {
    let program = if hgs.is_empty() { "hgs" } else { hgs };
    if shell == Shell::PowerShell {
        let quoted = power_shell_quote(&program.replace('/', "\\"));
        // Start-Process hands the exact Windows command line to the program in
        // every PowerShell edition.
        if !arguments.iter().all(|a| power_shell_passes_unchanged(a)) {
            let line = windows_command_line(arguments);
            let line = line.strip_prefix(' ').unwrap_or(&line);
            return format!(
                "Start-Process -FilePath {quoted} -ArgumentList {} -NoNewWindow -Wait",
                power_shell_quote(line)
            );
        }
        // A quoted program needs the call operator to run.
        let mut result = if quoted.starts_with('\'') {
            format!("& {quoted}")
        } else {
            quoted
        };
        for argument in arguments {
            result.push(' ');
            result.push_str(&power_shell_quote(argument));
        }
        return result;
    }
    let mut result = shell_quote(program);
    for argument in arguments {
        result.push(' ');
        result.push_str(&shell_quote(argument));
    }
    result
}

/// Arguments of `hgs [@computer] a NAME`, or of resuming an archived
/// conversation (`SessionCommands::openArguments`).
pub fn open_arguments(computer: &str, name: &str, archive_id: Option<&str>) -> Vec<String> {
    let mut arguments = Vec::new();
    if !computer.is_empty() {
        arguments.push(format!("@{computer}"));
    }
    match archive_id.filter(|id| !id.is_empty()) {
        None => arguments.extend(["a".to_owned(), name.to_owned()]),
        Some(id) => arguments.extend([
            "resume".to_owned(),
            name.to_owned(),
            "--archive".to_owned(),
            id.to_owned(),
        ]),
    }
    arguments
}

/// The command that opens (or resumes) a session (`SessionCommands::open`).
pub fn open_command(
    hgs: &str,
    computer: &str,
    name: &str,
    archive_id: Option<&str>,
    shell: Shell,
) -> String {
    command(hgs, &open_arguments(computer, name, archive_id), shell)
}

#[cfg(test)]
mod tests {
    use super::*;
    use Shell::{Posix, PowerShell};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn open(hgs: &str, computer: &str, name: &str, archive: Option<&str>, shell: Shell) -> String {
        open_command(hgs, computer, name, archive, shell)
    }

    #[test]
    fn posix_quotes_only_what_needs_it() {
        assert_eq!(
            shell_quote("claude/sample-project/review"),
            "claude/sample-project/review"
        );
        assert_eq!(shell_quote("release planning"), "'release planning'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(shell_quote(""), "''");
        assert_eq!(
            open(
                "/home/example/.local/bin/hgs",
                "mac",
                "claude/sample-project/review",
                None,
                Posix
            ),
            "/home/example/.local/bin/hgs @mac a claude/sample-project/review"
        );
        assert_eq!(
            open("hgs", "", "claude/sample-project/review", Some("0f3c2b1a-1111-4222-8333-944445555666"), Posix),
            "hgs resume claude/sample-project/review --archive 0f3c2b1a-1111-4222-8333-944445555666"
        );
    }

    #[test]
    fn power_shell_uses_the_call_operator_or_start_process() {
        assert_eq!(
            command(
                "C:/Program Files/Zerus/hgs.exe",
                &args(&["a", "claude/sample-project/review"]),
                PowerShell
            ),
            "& 'C:\\Program Files\\Zerus\\hgs.exe' a claude/sample-project/review"
        );
        assert_eq!(power_shell_quote("it\u{2019}s"), "'it\u{2019}\u{2019}s'");
        assert!(command(
            "hgs",
            &args(&["a", "claude/sample-project/release notes\\"]),
            PowerShell
        )
        .starts_with("Start-Process -FilePath hgs -ArgumentList "));
    }

    // Ports of test_terminalbackend.cpp.
    #[test]
    fn session_commands_preserve_identity_and_quoting() {
        assert_eq!(
            open("", "", "codex/docs/review", None, Posix),
            "hgs a codex/docs/review"
        );
        assert_eq!(
            open("/opt/hgs", "mac", "codex/проект/Новый план", None, Posix),
            "/opt/hgs @mac a 'codex/проект/Новый план'"
        );
        assert_eq!(
            open(
                "/tmp/hgs's bin/hgs",
                "mac",
                "codex/docs/it's $(exit 7); `exit 8`",
                None,
                Posix
            ),
            "'/tmp/hgs'\\''s bin/hgs' @mac a 'codex/docs/it'\\''s $(exit 7); `exit 8`'"
        );
        assert_eq!(
            command(
                "",
                &args(&["codex", "/work/a project", "--new", "-n", "plan"]),
                Posix
            ),
            "hgs codex '/work/a project' --new -n plan"
        );
    }

    #[test]
    fn archive_commands_restore_the_selected_instance() {
        assert_eq!(
            open(
                "/usr/local/bin/hgs",
                "mac",
                "claude/infra/review",
                Some("archive-one"),
                Posix
            ),
            "/usr/local/bin/hgs @mac resume claude/infra/review --archive archive-one"
        );
        assert_ne!(
            open("", "", "codex/docs/review", Some("older"), Posix),
            open("", "", "codex/docs/review", Some("newer"), Posix)
        );
        assert_eq!(
            open("", "", "codex/docs/review", Some("id with ' quotes"), Posix),
            "hgs resume codex/docs/review --archive 'id with '\\'' quotes'"
        );
    }

    #[test]
    fn power_shell_commands_quote_every_non_plain_argument() {
        assert_eq!(
            open(
                "C:/Program Files/Zerus/hgs.exe",
                "mac",
                "codex/проект/Новый план",
                None,
                PowerShell
            ),
            "& 'C:\\Program Files\\Zerus\\hgs.exe' '@mac' a 'codex/проект/Новый план'"
        );
        assert_eq!(
            open(
                "C:\\Zerus\\hgs.exe",
                "",
                "codex/docs/review",
                None,
                PowerShell
            ),
            "C:\\Zerus\\hgs.exe a codex/docs/review"
        );
        assert_eq!(
            command(
                "",
                &args(&[
                    "it's",
                    "$HOME",
                    "`x`",
                    "a,b",
                    "(1)",
                    "50%",
                    "C:\\dir\\",
                    "--new",
                    "-n"
                ]),
                PowerShell
            ),
            "hgs 'it''s' '$HOME' '`x`' 'a,b' '(1)' '50%' C:\\dir\\ --new -n"
        );
        assert_eq!(
            open(
                "C:/Program Files/Zerus/hgs.exe",
                "mac",
                "claude/p/fix \"login\" it's",
                None,
                PowerShell
            ),
            "Start-Process -FilePath 'C:\\Program Files\\Zerus\\hgs.exe' \
             -ArgumentList '@mac a \"claude/p/fix \\\"login\\\" it''s\"' -NoNewWindow -Wait"
        );
        assert_eq!(
            command("", &args(&["codex", "/work/my dir\\", "-n", ""]), PowerShell),
            "Start-Process -FilePath hgs -ArgumentList 'codex \"/work/my dir\"\\ -n \"\"' -NoNewWindow -Wait"
        );
        assert_eq!(power_shell_quote("don\u{2019}t"), "'don\u{2019}\u{2019}t'");
        assert_eq!(
            open("", "", "codex/p", Some("id with ' quotes"), PowerShell),
            "hgs resume codex/p --archive 'id with '' quotes'"
        );
    }

    #[test]
    fn open_arguments_put_the_computer_first() {
        assert_eq!(
            open_arguments("mac", "codex/p", None),
            args(&["@mac", "a", "codex/p"])
        );
        assert_eq!(
            open_arguments("", "codex/p", Some("old")),
            args(&["resume", "codex/p", "--archive", "old"])
        );
    }

    // Ports of test_windowsdesktop.cpp.
    #[test]
    fn template_program_command_is_one_powershell_line() {
        assert_eq!(
            command(
                "C:/Zerus/hgs.exe",
                &args(&["@mac", "a", "codex/p"]),
                PowerShell
            ),
            "C:\\Zerus\\hgs.exe '@mac' a codex/p"
        );
    }

    // Port of test_terminalview.cpp (the program part is empty here).
    #[test]
    fn windows_command_line_matches_qprocess_quoting() {
        assert_eq!(
            windows_command_line(&args(&["@mac", "a", "codex/p/my name"])),
            " @mac a \"codex/p/my name\""
        );
        assert_eq!(
            windows_command_line(&args(&[
                "",
                "say \"hi\"",
                "C:\\dir\\",
                "C:\\my dir\\",
                "a\\\"b"
            ])),
            " \"\" \"say \\\"hi\\\"\" C:\\dir\\ \"C:\\my dir\"\\ a\\\\\\\"b"
        );
    }

    #[test]
    fn current_shell_follows_the_platform() {
        assert_eq!(
            current_shell(),
            if cfg!(windows) { PowerShell } else { Posix }
        );
    }
}
