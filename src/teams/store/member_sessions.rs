use super::*;

// Settings admit at most 200 members, plus the separate lead roster row.
pub(super) const MAX_ROSTER: usize = 201;
pub(super) const MEMBER_SESSIONS_SQL: &str =
    "SELECT id,machine,session_id,state,data FROM members WHERE team=?1 ORDER BY id LIMIT ?2";
pub(super) const MEMBER_SESSION_SQL: &str =
    "SELECT machine,session_id FROM members WHERE team=?1 AND id=?2";
pub(super) const MEMBER_HANDLE_SQL: &str =
    "SELECT machine,session_id,launch,label_hint FROM member_handles WHERE team=?1 AND member=?2";

impl Store {
    /// Binding metadata by primary key; a malformed member record cannot hide it.
    pub fn member_session(&self, team: &str, member: &str) -> Result<Option<SessionKey>> {
        self.active()?;
        let Some(db) = &self.db else {
            return Ok(None);
        };
        let row: Option<(Option<String>, Option<String>)> = db
            .query_row(MEMBER_SESSION_SQL, rusqlite::params![team, member], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()
            .map_err(storage)?;
        Ok(row
            .and_then(|(machine, session)| machine.zip(session))
            .map(|(machine, session_id)| SessionKey {
                machine,
                session_id,
            }))
    }

    /// A member's durable handle by primary key (zerus-63rru.53); none for a
    /// member bound by run, or bound before handles and not yet backfilled.
    pub fn member_handle(&self, team: &str, member: &str) -> Result<Option<MemberHandle>> {
        self.active()?;
        let Some(db) = &self.db else {
            return Ok(None);
        };
        db.query_row(MEMBER_HANDLE_SQL, rusqlite::params![team, member], |r| {
            Ok(MemberHandle {
                machine: r.get(0)?,
                session_id: r.get(1)?,
                launch: r.get(2)?,
                label_hint: r.get(3)?,
            })
        })
        .optional()
        .map_err(storage)
    }

    /// One primary-key range read, including unbound rows in the bound so a
    /// malformed roster cannot turn this into a scan of arbitrary size.
    pub fn member_sessions(&self, team: &str) -> Result<Vec<MemberSession>> {
        self.active()?;
        let Some(db) = &self.db else {
            return Ok(Vec::new());
        };
        let mut statement = db.prepare(MEMBER_SESSIONS_SQL).map_err(storage)?;
        let roster = statement
            .query_map(rusqlite::params![team, MAX_ROSTER + 1], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })
            .map_err(storage)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage)?;
        if roster.len() > MAX_ROSTER {
            return Err(Problem::new(
                code::OVER_CAP,
                "The member roster exceeds its bound.",
            ));
        }
        roster
            .into_iter()
            .map(|(member, machine, session, state, data)| {
                let state: MemberState = serde_json::from_str(&state).map_err(|_| {
                    Problem::new(
                        code::INVALID_STATE,
                        "The member roster contains an invalid state.",
                    )
                    .with_refs(["member_sessions"])
                })?;
                if state == MemberState::Removed {
                    return Ok(None);
                }
                let role = serde_json::from_str::<serde_json::Value>(&data)
                    .ok()
                    .and_then(|value| value["role"].as_str().map(str::to_owned))
                    .unwrap_or_default();
                Ok(machine
                    .zip(session)
                    .map(|(machine, session_id)| MemberSession {
                        member,
                        role,
                        machine,
                        session_id,
                    }))
            })
            .filter_map(|result| result.transpose())
            .collect()
    }
}
