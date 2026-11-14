//! A prospective member ref must be absent and have neither an ancestor nor
//! a descendant ref. Git's namespace query includes loose and packed refs.
use super::*;

pub const MEMBER_REF_CONFLICT: &str = "member_branch_ref_conflict";

pub fn member_branch_available(repo: &Repo, branch: &str) -> Result<(), Problem> {
    if branch.len() > 512 || branch.split('/').count() > 64 {
        return Err(unavailable(
            "Member branch exceeds the bounded namespace check",
        ));
    }
    let git = Git::repo(repo)?;
    let reference = git.branch_ref(branch)?;
    let conflict = |existing: &str| {
        Problem::new(MEMBER_REF_CONFLICT, format!(
            "Member ref '{reference}' conflicts with existing Git ref '{existing}'. Choose a different integration branch or review the retained refs and files."
        )).with_refs([reference.clone(), existing.to_owned()])
    };
    // Check only the candidate's parent components, never the whole ref list.
    let mut parent = reference.as_str();
    while let Some((above, _)) = parent.rsplit_once('/') {
        if above == "refs" {
            break;
        }
        let output = git.run(&["show-ref", "--verify", "--quiet", above])?;
        match output.status.code() {
            Some(0) => return Err(conflict(above)),
            Some(1) => (),
            _ => return Err(git_failure(&output)),
        }
        parent = above;
    }
    let raw = git.read(&[
        "for-each-ref",
        "--count=1",
        "--format=%(refname)",
        &reference,
    ])?;
    let found = std::str::from_utf8(&raw)
        .map_err(|_| unavailable("Git ref namespace is not UTF-8"))?
        .trim_end_matches('\n');
    if !found.is_empty() {
        if found.len() > 4096
            || found.contains(['\n', '\r'])
            || !(found == reference || found.starts_with(&format!("{reference}/")))
        {
            return Err(unavailable("Git returned an invalid member ref namespace"));
        }
        return Err(conflict(found));
    }
    Ok(())
}
