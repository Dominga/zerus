//! Read-only admission for changing the next hand-out's base.
use super::*;
use serde::Serialize;

const MAIN_REFS: [&str; 2] = ["refs/heads/main", "refs/remotes/origin/main"];

#[derive(Clone, Debug, Serialize)]
pub(crate) struct MainRef {
    pub reference: String,
    pub commit: String,
}

fn invalid(message: impl Into<String>) -> Problem {
    Problem::new(code::INVALID_STATE, message)
}

fn refs(git: &Git) -> Result<Vec<MainRef>, Problem> {
    let mut refs = Vec::new();
    for reference in MAIN_REFS {
        // A literal main ref must not alias HEAD or another ref.
        let symbolic = git.run(&["symbolic-ref", "-q", reference])?;
        match symbolic.status.code() {
            Some(1) => (),
            Some(0) => return Err(invalid(format!("{reference} must be a direct main ref."))),
            _ => return Err(git_failure(&symbolic)),
        }
        if let Some(commit) = git.reference(reference)? {
            if git.commit(&commit)? != commit {
                return Err(invalid(format!("{reference} does not name a commit.")));
            }
            refs.push(MainRef {
                reference: reference.into(),
                commit,
            });
        }
    }
    if refs.is_empty() {
        return Err(invalid("No main commit is available. Checked refs/heads/main and refs/remotes/origin/main; fetch or update main outside Zerus first."));
    }
    Ok(refs)
}

pub(crate) fn main_refs(repo: &Repo) -> Result<Vec<MainRef>, Problem> {
    refs(&Git::repo(repo)?)
}

fn contains(git: &Git, base: &str, commit: &str) -> Result<bool, Problem> {
    let output = git.run(&["merge-base", "--is-ancestor", base, commit])?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(git_failure(&output)),
    }
}

pub(crate) fn validate_base_advance(
    repo: &Repo,
    old: &str,
    commit: &str,
) -> Result<String, Problem> {
    if !matches!(commit.len(), 40 | 64)
        || !commit
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid(
            "Name the full lowercase commit hash to advance the base.",
        ));
    }
    let git = Git::repo(repo)?;
    // Verify exact immutable commit identity, not a tag/ref/expression.
    if git
        .commit(commit)
        .map_err(|_| invalid("The proposed commit is missing from the team's repository."))?
        != commit
    {
        return Err(invalid("The proposed hash must name a commit directly."));
    }
    let old = git.commit(old)?;
    if commit == old {
        return Err(invalid(
            "The proposed commit is already the team's base; choose a strictly newer commit.",
        ));
    }
    if !contains(&git, &old, commit)? {
        return Err(invalid("The current base must be an ancestor of the proposed commit; backward or unrelated history is refused."));
    }
    for main in refs(&git)? {
        if contains(&git, commit, &main.commit)? {
            return Ok(main.reference);
        }
    }
    Err(invalid("The proposed commit is not contained by main. Checked refs/heads/main and refs/remotes/origin/main; advancing to non-main history is refused."))
}
