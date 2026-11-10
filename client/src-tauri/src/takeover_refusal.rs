//! Task 180 (owner, 2026-11-07, local build 152): a refused native takeover
//! says why in the app. The CLI refuses the frontend with one fixed line per
//! check; only those exact lines select a code (child text never reaches the
//! UI or a log), and each code names the check it failed and so the next step
//! the app shows: the portable binding, the binding's folder privacy, the
//! process ancestry, the ownership journal, or the process identity.

/// The code of an exact refusal line of the CLI's frontend admission that the
/// older lists do not already name.
pub(crate) fn code(line: &str) -> Option<&'static str> {
    Some(match line {
        "hgs: frontend binding unavailable" => "frontend_binding_unavailable",
        "hgs: frontend folders are not private" => "frontend_folders_not_private",
        "hgs: frontend binding names another hgs" => "frontend_binding_other_cli",
        "hgs: frontend is not the bound application" => "frontend_not_bound_application",
        // A CLI before task 180 says only this for every portable refusal.
        "hgs: canonical frontend is not the installed application" => "frontend_not_installed_application",
        _ => return None,
    })
}

/// Task #53 (team ede724c4): the fixed codes `hgs session-host install --json`
/// gives as `frontend_refusal_code` when it records no binding for this app.
/// Only these exact values are kept; any other text selects none.
const RECORD_CODES: [&str; 9] = [
    "agent_descended_frontend_refused",
    "frontend_parent_not_beside",
    "frontend_parent_gone",
    "frontend_record_timed_out",
    "frontend_cli_pin_changed",
    "frontend_cli_image_unreadable",
    "frontend_app_unreadable",
    "frontend_ancestry_unverified",
    "frontend_record_refused",
];

/// The fixed code of an install's record refusal, or None for any other text.
pub(crate) fn record_code(value: &str) -> Option<&'static str> {
    RECORD_CODES.iter().copied().find(|code| *code == value)
}

/// Why this start's install recorded no binding (a `RECORD_CODES` value), or "".
static RECORD_REFUSAL: std::sync::Mutex<&'static str> = std::sync::Mutex::new("");

/// Task #53: the install before the takeover recorded no binding, and why.
pub(crate) fn note_record_refusal(code: &'static str) {
    *RECORD_REFUSAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = code;
}

/// Task #53 (owner, 2026-11-10, build 175): a takeover that found no binding
/// says why the install before it recorded none (the process that started
/// Zerus, a deadline, a changed file), not only that frontend.json is missing.
pub(crate) fn refined(reason: &'static str) -> &'static str {
    let recorded = *RECORD_REFUSAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let binding = matches!(reason, "frontend_binding_unavailable" | "frontend_binding_other_cli" | "frontend_not_bound_application");
    if binding && !recorded.is_empty() { recorded } else { reason }
}

/// The check a takeover refusal's code failed, as the app names it.
pub(crate) fn check(code: &str) -> &'static str {
    match code {
        "frontend_binding_unavailable" | "frontend_binding_other_cli" | "frontend_not_bound_application"
        | "frontend_not_installed_application" => "binding",
        "frontend_parent_not_beside" | "frontend_parent_gone" | "frontend_ancestry_unverified" => "ancestry",
        "frontend_record_timed_out" | "frontend_cli_pin_changed" | "frontend_cli_image_unreadable" | "frontend_app_unreadable"
        | "frontend_record_refused" => "binding",
        "frontend_folders_not_private" => "folder_privacy",
        "agent_descended_frontend_refused" => "ancestry",
        "frontend_process_image_unavailable"
        | "frontend_process_identity_unavailable"
        | "canonical_frontend_evidence_unavailable"
        | "frontend_authentication_refused"
        | "frontend_identity_expired" => "identity",
        code if code.starts_with("ownership_") => "ownership_journal",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_refusal_line_names_its_check_and_other_text_names_none() {
        for (line, code, named) in [
            ("hgs: frontend binding unavailable", "frontend_binding_unavailable", "binding"),
            ("hgs: frontend folders are not private", "frontend_folders_not_private", "folder_privacy"),
            ("hgs: frontend binding names another hgs", "frontend_binding_other_cli", "binding"),
            ("hgs: frontend is not the bound application", "frontend_not_bound_application", "binding"),
            ("hgs: canonical frontend is not the installed application", "frontend_not_installed_application", "binding"),
        ] {
            assert_eq!(super::code(line), Some(code), "{line}");
            assert_eq!(check(code), named, "{code}");
        }
        assert_eq!(super::code("hgs: frontend binding unavailable: C:\\Users\\x"), None, "child text never selects a code");
        assert_eq!(check("agent_descended_frontend_refused"), "ancestry");
        assert_eq!(check("ownership_not_settled"), "ownership_journal");
        assert_eq!(check("canonical_frontend_evidence_unavailable"), "identity");
        assert_eq!(check("timed_out"), "other");
    }

    /// Task #53: an install's record refusal is kept only as one of its fixed
    /// codes, and it explains a later binding refusal, never another one.
    #[test]
    fn a_record_refusal_explains_a_missing_binding() {
        for code in RECORD_CODES {
            assert_eq!(record_code(code), Some(code));
            assert_ne!(check(code), "other", "{code}");
        }
        assert_eq!(record_code("the parent's image C:/x is not the app"), None, "child text selects no code");
        assert_eq!(refined("frontend_binding_unavailable"), "frontend_binding_unavailable");
        note_record_refusal("frontend_parent_not_beside");
        assert_eq!(refined("frontend_binding_unavailable"), "frontend_parent_not_beside");
        assert_eq!(check(refined("frontend_binding_unavailable")), "ancestry");
        assert_eq!(refined("child_failed"), "child_failed", "another refusal keeps its own code");
        note_record_refusal("");
        assert_eq!(refined("frontend_binding_unavailable"), "frontend_binding_unavailable");
    }
}
