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

/// The check a takeover refusal's code failed, as the app names it.
pub(crate) fn check(code: &str) -> &'static str {
    match code {
        "frontend_binding_unavailable" | "frontend_binding_other_cli" | "frontend_not_bound_application"
        | "frontend_not_installed_application" => "binding",
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
}
