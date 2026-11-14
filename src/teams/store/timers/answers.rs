//! A member's answers: its own questions of the attempt it holds, and the
//! marks that ring its doorbell. Every read is a prefix search of one
//! attempt or one team's marks, never of the team's history.
use super::*;

pub(crate) mod sql {
    /// Pinned to its index: without statistics the planner would take the
    /// inbox's unique (team,kind,...) index and walk every question of the team.
    pub const QUESTIONS: &str = "SELECT seq,text,state,reply FROM inbox INDEXED BY inbox_questions WHERE team=?1 AND node=?2 AND generation=?3 AND kind GLOB 'question:*' AND json_extract(actor,'$.member')=?4 ORDER BY seq DESC LIMIT ?5";
    pub const MARKS: &str = "SELECT member,node,generation,revision,replies FROM answer_marks WHERE team=?1 AND member>?2 ORDER BY member LIMIT ?3";
    pub const MARK: &str =
        "SELECT revision FROM answer_marks WHERE team=?1 AND member=?2 AND node=?3 AND generation=?4";
    pub const HOLDER: &str = "SELECT e.member,e.state FROM nodes n JOIN effects e ON e.team=n.team AND e.node=n.id AND e.generation=n.generation WHERE n.team=?1 AND n.id=?2 AND n.generation=?3 ORDER BY e.seq DESC LIMIT 1";
}
/// How many of a member's questions `current` shows.
pub const QUESTIONS_SHOWN: u32 = 10;

/// One question of the attempt, as its asker reads it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Question {
    pub item: u64,
    pub question: String,
    /// `open`, `answered`, or `closed` when the attempt's report or an
    /// end closed it unanswered.
    pub state: &'static str,
    pub answer: Option<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Questions {
    pub rows: Vec<Question>,
    pub more: bool,
}
/// The newest reply that marks a member, with the count for its attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnswerMark {
    pub member: String,
    pub node: u32,
    pub generation: u32,
    pub revision: u64,
    pub replies: u32,
}
/// The exact member, attempt and mark a member's doorbell was reserved for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnswerBell {
    pub team: String,
    pub member: String,
    pub session: SessionKey,
    pub node: u32,
    pub generation: u32,
    pub mark: u64,
}

impl Store {
    /// `member`'s own questions of one attempt, newest first, at most ten,
    /// and whether there are more.
    pub fn questions(
        &self,
        team: &str,
        node: u32,
        generation: u32,
        member: &str,
    ) -> Result<Questions> {
        self.read(|db| {
            let take = QUESTIONS_SHOWN + 1;
            let mut rows = rows(
                db,
                sql::QUESTIONS,
                params![team, node, generation, member, take],
                |r| {
                    let state: String = r.get(2)?;
                    let answer: Option<String> = r.get(3)?;
                    let state = match (state.as_str(), &answer) {
                        ("open", _) => "open",
                        (_, Some(_)) => "answered",
                        _ => "closed",
                    };
                    Ok(Question {
                        item: r.get(0)?,
                        question: r.get(1)?,
                        state,
                        answer,
                    })
                },
            )?;
            let more = rows.len() > QUESTIONS_SHOWN as usize;
            rows.truncate(QUESTIONS_SHOWN as usize);
            Ok(Questions { rows, more })
        })
    }
    /// The team's answer marks, one per member, after `after`.
    pub fn answer_marks(&self, team: &str, after: &str, limit: u32) -> Result<Vec<AnswerMark>> {
        let page = super::limit(limit)?;
        self.read(|db| {
            rows(db, sql::MARKS, params![team, after, page], |r| {
                Ok(AnswerMark {
                    member: r.get(0)?,
                    node: r.get(1)?,
                    generation: r.get(2)?,
                    revision: r.get(3)?,
                    replies: r.get(4)?,
                })
            })
        })
    }
}

/// The member doorbell's admission, read inside the immediate transaction
/// held across its spawn: the team still running, the member's binding
/// unchanged, its mark not dropped, and the member still holding that attempt.
pub(crate) fn admit_answer(db: &Connection, bell: &AnswerBell) -> Result<()> {
    let team =
        super::super::rows::team(db, &bell.team)?.ok_or_else(|| refuse("The team is gone."))?;
    if team.status != zerus_team_plan::model::TeamStatus::Running {
        return Err(refuse("The team is not running."));
    }
    let member = super::super::rows::member(db, &team.id, &bell.member)?;
    if !member.is_some_and(|m| {
        m.state == MemberState::Active && m.session.as_ref() == Some(&bell.session)
    }) {
        return Err(refuse("The member's binding changed."));
    }
    let mark: Option<u64> = db
        .query_row(
            sql::MARK,
            params![team.id, bell.member, bell.node, bell.generation],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    if mark.is_none_or(|revision| revision < bell.mark) {
        return Err(refuse("The answer mark was dropped."));
    }
    let holder: Option<(String, String)> = db
        .query_row(
            sql::HOLDER,
            params![team.id, bell.node, bell.generation],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(storage)?;
    let holds = holder.is_some_and(|(m, state)| {
        m == bell.member && !matches!(state.as_str(), "reserved" | "refused" | "superseded")
    });
    if !holds {
        return Err(refuse("The member no longer holds that attempt."));
    }
    Ok(())
}
