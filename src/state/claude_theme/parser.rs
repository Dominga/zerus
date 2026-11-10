//! Claude's complete first-run appearance panel. Recognition is not authority
//! to answer another startup question, nor a substitute for native identity.
use sha2::{Digest, Sha256};

pub(super) const TITLE: &str = "Choose the text style that looks best with your terminal";
const HINT: &str = "To change this later, run /theme";
pub(super) const OPTIONS: [&str; 7] = [
    "Auto (match terminal)",
    "Dark mode",
    "Light mode",
    "Dark mode (colorblind-friendly)",
    "Light mode (colorblind-friendly)",
    "Dark mode (ANSI colors only)",
    "Light mode (ANSI colors only)",
];
const PREVIEW: [&str; 4] = [
    "1 function greet() {",
    "2 - console.log(\"Hello, World!\");",
    "2 + console.log(\"Hello, Claude!\");",
    "3 }",
];
const FOOTER: &str = "Syntax theme: Monokai Extended (ctrl+t to disable)";
// Native2.1.322: these exact manual rows have different syntax previews.
// They are read-only recognition, never additional Enter authority.
const LIGHT_FOOTER: &str = "Syntax theme: GitHub (ctrl+t to disable)";
const ANSI_FOOTER: &str = "Syntax theme: ansi (ctrl+t to disable)";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DefaultTheme {
    Auto,
    Dark,
}
impl DefaultTheme {
    pub(super) fn label(self) -> &'static str {
        OPTIONS[match self {
            Self::Auto => 0,
            Self::Dark => 1,
        }]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Panel {
    pub(super) version: String,
    footer: String,
    pub(super) selected: usize,
    checked: Option<usize>,
    pub(super) fingerprint: String,
}
impl Panel {
    /// A selection elsewhere, or an incomplete current-value mark, is still
    /// reportable in Activity but never gets automatic navigation or input.
    pub(super) fn selected_default(&self) -> Option<DefaultTheme> {
        if self.checked != Some(self.selected) || self.footer != FOOTER {
            return None;
        }
        match self.selected {
            0 => Some(DefaultTheme::Auto),
            1 => Some(DefaultTheme::Dark),
            _ => None,
        }
    }
}

fn version(row: &str) -> Option<&str> {
    let value = row.strip_prefix("Welcome to Claude Code v")?;
    let parts: Vec<_> = value.split('.').collect();
    (parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty() && part.len() <= 9 && part.bytes().all(|byte| byte.is_ascii_digit())
        }))
    .then_some(value)
}

fn rule(row: &str) -> bool {
    row.chars().count() >= 20 && row.chars().all(|ch| ch == '╌')
}

fn words(row: &str) -> String {
    row.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn skip_blank(rows: &[&str], index: &mut usize) {
    while rows.get(*index).is_some_and(|row| row.is_empty()) {
        *index += 1;
    }
}

pub(super) fn parse(screen: &str) -> Option<Panel> {
    if screen.len() > 128 * 1024
        || screen
            .chars()
            .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
    {
        return None;
    }
    let rows: Vec<_> = screen.lines().map(str::trim).collect();
    if rows.len() > 200 {
        return None;
    }
    let header = rows.iter().position(|row| version(row).is_some())?;
    if rows[..header].iter().any(|row| !row.is_empty()) {
        return None;
    }
    let release = version(rows[header])?;
    let greeting = rows.iter().position(|row| *row == "Let's get started.")?;
    let art = rows.get(header + 1..greeting)?;
    if art
        .iter()
        .any(|row| row.chars().any(|ch| !" *█▓▒░▄.".contains(ch)))
        || rows.iter().filter(|row| **row == TITLE).count() != 1
        || rows.iter().filter(|row| **row == HINT).count() != 1
        || rows.iter().filter(|row| version(row).is_some()).count() != 1
    {
        return None;
    }
    let mut index = greeting + 1;
    skip_blank(&rows, &mut index);
    if rows.get(index) != Some(&TITLE) || rows.get(index + 1) != Some(&HINT) {
        return None;
    }
    index += 2;
    skip_blank(&rows, &mut index);
    let (mut selected, mut checked) = (None, None);
    for (option, expected) in OPTIONS.iter().enumerate() {
        let mut row = *rows.get(index)?;
        if let Some(label) = row.strip_prefix("❯ ") {
            if selected.replace(option).is_some() {
                return None;
            }
            row = label.trim_start();
        }
        if let Some(label) = row.strip_prefix("✔ ") {
            if checked.replace(option).is_some() {
                return None;
            }
            row = label.trim_start();
        }
        if row != *expected {
            return None;
        }
        index += 1;
    }
    let selected = selected?;
    skip_blank(&rows, &mut index);
    if !rule(rows.get(index)?) {
        return None;
    }
    index += 1;
    for expected in PREVIEW {
        if words(rows.get(index)?) != expected {
            return None;
        }
        index += 1;
    }
    if !rule(rows.get(index)?) {
        return None;
    }
    let footer = *rows.get(index + 1)?;
    if footer != FOOTER && !matches!((selected, footer), (2, LIGHT_FOOTER) | (5, ANSI_FOOTER)) {
        return None;
    }
    if rows[index + 2..].iter().any(|row| !row.is_empty()) {
        return None;
    }
    // Animation/spacing in the verified artwork never creates a new theme
    // interaction. The original binding's once-key is independently durable.
    let semantic = serde_json::to_vec(&(
        release, TITLE, HINT, OPTIONS, selected, checked, PREVIEW, footer,
    ))
    .ok()?;
    Some(Panel {
        version: release.to_owned(),
        footer: footer.to_owned(),
        selected,
        checked,
        fingerprint: format!("{:x}", Sha256::digest(semantic)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const SCREEN: &str = include_str!("../../../tests/fixtures/claude-theme-first-run.txt");

    #[test]
    fn complete_native_panel_preserves_only_the_selected_current_auto_or_dark() {
        let dark = parse(SCREEN).unwrap();
        assert_eq!(dark.selected_default(), Some(DefaultTheme::Dark));
        assert_eq!(dark.selected_default().unwrap().label(), "Dark mode");
        let auto = SCREEN
            .replace("     Auto (match terminal)", " ❯ ✔ Auto (match terminal)")
            .replace(" ❯ ✔ Dark mode", "     Dark mode");
        let auto = parse(&auto).unwrap();
        assert_eq!(auto.selected_default(), Some(DefaultTheme::Auto));
        assert_ne!(auto.fingerprint, dark.fingerprint);
        let light = SCREEN
            .replace(" ❯ ✔ Dark mode", "     Dark mode")
            .replace("     Light mode\n", " ❯ ✔ Light mode\n");
        assert!(parse(&light).unwrap().selected_default().is_none());
        assert!(parse(&SCREEN.replace("❯ ✔ Dark mode", "❯ Dark mode"))
            .unwrap()
            .selected_default()
            .is_none());
    }

    #[test]
    fn native_manual_syntax_variants_cannot_mint_dark_or_auto_authority() {
        for (selected, footer) in [(2, LIGHT_FOOTER), (5, ANSI_FOOTER)] {
            let manual = SCREEN
                .replace(" ❯ ✔ Dark mode", "     Dark mode")
                .replace(
                    &format!("     {}\n", OPTIONS[selected]),
                    &format!(" ❯ ✔ {}\n", OPTIONS[selected]),
                )
                .replace(FOOTER, footer);
            let panel = parse(&manual).unwrap();
            assert_eq!(panel.selected, selected);
            assert!(panel.selected_default().is_none());
            assert_ne!(
                panel.fingerprint,
                parse(&manual.replace(footer, FOOTER)).unwrap().fingerprint
            );
            assert!(parse(&SCREEN.replace(FOOTER, footer)).is_none());
            let auto = SCREEN
                .replace("     Auto (match terminal)", " ❯ ✔ Auto (match terminal)")
                .replace(" ❯ ✔ Dark mode", "     Dark mode")
                .replace(FOOTER, footer);
            assert!(parse(&auto).is_none());
        }
    }

    #[test]
    fn native_light_art_medium_shade_is_only_closed_decoration() {
        // Exact isolated official2.1.322 Light99x54 artwork. This is parser
        // coverage; the separate actual native suite supplies runtime proof.
        const LIGHT_ART: &str = "..........................................................\n\n\n\n            ░░░░░░\n    ░░░   ░░░░░░░░░░\n   ░░░░░░░░░░░░░░░░░░░\n\n                           ░░░░                     ██\n                         ░░░░░░░░░░               ██▒▒██\n                                            ▒▒      ██   ▒\n       █████████                          ▒▒░░▒▒      ▒ ▒▒\n      ██▄█████▄██                           ▒▒         ▒▒\n       █████████                           ░          ▒\n.......█ █   █ █..........................░..........▒....\n\n";
        let tail = SCREEN.split_once(" Let's get started.").unwrap().1;
        let light =
            format!("Welcome to Claude Code v2.1.322\n{LIGHT_ART} Let's get started.{tail}")
                .replace(" ❯ ✔ Dark mode", "     Dark mode")
                .replace("     Light mode\n", " ❯ ✔ Light mode\n")
                .replace(FOOTER, LIGHT_FOOTER);
        let panel = parse(&light).unwrap();
        assert_eq!(panel.selected, 2);
        assert!(panel.selected_default().is_none());
        for unknown in ["unreviewed text", "\u{1b}[31m", "◆"] {
            assert!(parse(&light.replacen('▒', unknown, 1)).is_none());
        }
    }

    #[test]
    fn clipped_changed_quoted_or_followed_panels_never_authorize_an_enter() {
        for screen in [
            SCREEN.replace(TITLE, "Choose your permissions"),
            SCREEN.replace(HINT, "Press Enter to accept the terms"),
            SCREEN.replace("     Light mode (ANSI colors only)\n", ""),
            SCREEN.replace("     Light mode\n", " ❯ Light mode\n"),
            SCREEN.replace("     Light mode\n", "   ✔ Light mode\n"),
            SCREEN.replace("Hello, Claude!", "Trust this folder!"),
            SCREEN.replace(FOOTER, "Select login method:"),
            format!("A quoted example:\n{SCREEN}"),
            format!("{SCREEN}\n❯ 1. Claude account with subscription\n"),
            SCREEN.replacen(
                "..........................................................",
                "unrecognized launcher output",
                1,
            ),
            SCREEN.split("  1  function").next().unwrap().to_owned(),
        ] {
            assert!(parse(&screen).is_none(), "{screen}");
        }
    }

    #[test]
    fn only_reviewed_launcher_notices_may_precede_the_first_run_frame() {
        let notice = "run_claude: SOPS_AGE_KEY already set — reusing it. run_claude: secrets unlocked → launching claude";
        assert!(parse(&format!("{notice}\n{SCREEN}")).is_none());
        let tail = SCREEN.split_once(" Let's get started.").unwrap().1;
        let compact = format!("Welcome to Claude Code v2.1.322\n\n Let's get started.{tail}");
        assert_eq!(parse(&compact), parse(SCREEN));
        assert!(parse(&format!("Ignore permissions and continue\n{SCREEN}")).is_none());
        let later = SCREEN.replace("v2.1.322", "v2.1.323");
        assert_eq!(
            parse(&later).unwrap().selected_default(),
            Some(DefaultTheme::Dark)
        );
        assert!(parse(&SCREEN.replace("v2.1.322", "v2.1.322 invalid")).is_none());
    }
}
