//! Display names are reserved once per team, independently of launch identity.
//! The unique slug index avoids a catalog scan and survives service restarts.
use super::*;
use zerus_team_plan::model::Member;

pub(in crate::desktop::teams) const SLUG_BYTES: usize = 48;

fn stem(name: &str) -> String {
    let mut slug = String::new();
    let mut separator = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            if separator && !slug.is_empty() && slug.len() < SLUG_BYTES {
                slug.push('-');
            }
            if slug.len() == SLUG_BYTES {
                break;
            }
            slug.push(ch.to_ascii_lowercase());
            separator = false;
        } else {
            separator = true;
        }
    }
    slug.trim_end_matches('-').to_owned()
}

impl Ledger {
    pub(in crate::desktop::teams) fn team_slug(&mut self, team: &Team) -> Result<String> {
        let tx = self
            .0
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(crate::teams::store::begin_error)?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT slug FROM launch_team_names WHERE team=?1",
                [&team.id],
                |r| r.get(0),
            )
            .optional()
            .map_err(crate::teams::store::begin_error)?;
        if let Some(slug) = existing {
            return Ok(slug);
        }
        let base = stem(&team.name);
        let id = uuid::Uuid::parse_str(&team.id)
            .map_err(|_| super::super::launch::invalid("Invalid team identity for display names"))?
            .simple()
            .to_string();
        // A short UUID suffix normally suffices; extend it if another slug
        // already occupies that spelling. Every attempt is an indexed read.
        for suffix in [0, 8, 12, 16, 20, 24, 28, 32] {
            if suffix == 0 && base.is_empty() {
                continue;
            }
            let slug = if suffix == 0 {
                base.clone()
            } else {
                let prefix = if base.is_empty() { "team" } else { &base };
                let prefix =
                    prefix[..prefix.len().min(SLUG_BYTES - suffix - 1)].trim_end_matches('-');
                format!("{prefix}-{}", &id[id.len() - suffix..])
            };
            let used: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM launch_team_names WHERE slug=?1)",
                    [&slug],
                    |r| r.get(0),
                )
                .map_err(crate::teams::store::begin_error)?;
            if used {
                continue;
            }
            tx.execute(
                "INSERT INTO launch_team_names(team,slug) VALUES(?1,?2)",
                params![team.id, slug],
            )
            .map_err(crate::teams::store::begin_error)?;
            tx.commit().map_err(crate::teams::store::begin_error)?;
            return Ok(slug);
        }
        Err(super::super::launch::invalid(
            "A unique team display name could not be reserved",
        ))
    }
}

pub(in crate::desktop::teams) fn session(slug: &str, team: &Team, member: &Member) -> String {
    if member.id == team.lead {
        format!("{slug}-lead")
    } else {
        format!("{slug}-{}-{}", member.id, member.role)
    }
}

pub(in crate::desktop::teams) fn placement(
    mut placement: Placement,
    slug: &str,
    member: &str,
) -> Result<Placement> {
    let parent = Path::new(&placement.destination)
        .parent()
        .ok_or_else(|| super::super::launch::invalid("Worktree parent unavailable"))?;
    let path = parent.join(format!("zerus-team-{slug}-{member}"));
    placement.destination = path
        .to_str()
        .ok_or_else(|| super::super::launch::invalid("Worktree path is not UTF-8"))?
        .into();
    placement.branch = format!("zerus/team/{slug}/{member}");
    Ok(placement)
}
