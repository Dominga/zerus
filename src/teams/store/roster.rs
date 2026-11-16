//! A complete capped roster for confirmation, including the separate lead.
//! Public pages keep their 200-row bound; this read needs at most two pages.
use super::*;

impl Store {
    pub(crate) fn complete_member_rows(&self, team: &str) -> Result<Vec<MemberRow>> {
        self.active()?;
        let Some(db) = &self.db else {
            return Ok(Vec::new());
        };
        let snapshot = db.unchecked_transaction().map_err(storage)?;
        let page = self.member_rows(team, MAX_PAGE, None)?;
        let mut members = page.rows;
        if let Some(cursor) = page.next {
            let tail = self.member_rows(team, MAX_PAGE, Some(&cursor))?;
            if tail.next.is_some() {
                return Err(roster_over_cap());
            }
            members.extend(tail.rows);
        }
        if members.len() > member_sessions::MAX_ROSTER {
            return Err(roster_over_cap());
        }
        snapshot.commit().map_err(storage)?;
        Ok(members)
    }
}

fn roster_over_cap() -> Problem {
    Problem::new(code::OVER_CAP, "The member roster exceeds its bound.")
}
