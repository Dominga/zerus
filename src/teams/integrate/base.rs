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
    // One snapshot of literal refs: revision parsing can DWIM a missing
    // refs/remotes/origin/main into refs/heads/refs/remotes/origin/main.
    let output = git.run(&[
        "for-each-ref",
        "--format=%(refname)%00%(objectname)%00%(symref)",
        MAIN_REFS[0],
        MAIN_REFS[1],
    ])?;
    if !output.status.success() {
        return Err(git_failure(&output));
    }
    let text = std::str::from_utf8(&output.stdout)
        .map_err(|_| invalid("The repository returned invalid main refs."))?;
    for row in text.lines() {
        let fields: Vec<_> = row.split('\0').collect();
        let [reference, commit, symbolic] = fields.as_slice() else {
            return Err(invalid("The repository returned invalid main refs."));
        };
        // for-each-ref patterns also include descendants of the named ref.
        if !MAIN_REFS.contains(reference) {
            continue;
        }
        if !symbolic.is_empty() {
            return Err(invalid(format!("{reference} must be a direct main ref.")));
        }
        if git.commit(commit)? != *commit {
            return Err(invalid(format!("{reference} does not name a commit.")));
        }
        refs.push(MainRef {
            reference: (*reference).into(),
            commit: (*commit).into(),
        });
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
