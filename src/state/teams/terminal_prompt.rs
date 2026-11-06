//! Task 113: a request only the member's Terminal takes (`terminal_question`,
//! `can_answer` false) never stays a quiet wait. The owner sees it at once
//! (`inbox::attention`, `permission_terminal`); once it has waited for the
//! owner queue's `defer_minutes` (task 97) Zerus raises it: one owner item with
//! the session and the screen's last lines, one phone alert on the lead's row
//! with **Open Terminal**, and the lead's starved-member text names it.
use super::model::{Member, MemberState, PendingQuestion, Team, TerminalEscalation};
use serde_json::{json, Value};

/// The phone alert shows at most this many of the screen's last lines.
const PHONE_LINES: usize = 6;
/// And at most this many characters of each.
const PHONE_LINE_CHARS: usize = 120;

/// The member's Terminal-only request, with its escalation when it belongs to
/// this very request.
pub(crate) fn of(member: &Member) -> Option<(&PendingQuestion, Option<&TerminalEscalation>)> {
    let asked = member.terminal_question.as_ref()?;
    let raised = member.terminal_escalation.as_ref().filter(|raised| raised.question_id == asked.question_id);
    Some((asked, raised))
}

/// Whether the request has waited `defer_minutes` and is not raised yet.
/// A team with the queue off (0 minutes) raises it after the default 10.
pub(crate) fn due(team: &Team, member: &Member, now: f64) -> bool {
    if member.state == MemberState::Removed || member.session.is_none() {
        return false;
    }
    let minutes = if team.settings.defer_minutes > 0 { team.settings.defer_minutes } else { 10 };
    of(member).is_some_and(|(asked, raised)| raised.is_none() && now - asked.at >= minutes as f64 * 60.0)
}

/// The request's title as the cards show it: the tool or the question.
pub(crate) fn title(asked: &PendingQuestion) -> String {
    let question = &asked.card["questions"][0];
    let header = question["header"].as_str().unwrap_or_default().trim();
    let text = question["question"].as_str().unwrap_or_default().trim();
    match (header.is_empty(), text.is_empty()) {
        (false, false) if header != text => format!("{header}: {text}"),
        (false, _) => header.to_owned(),
        (true, false) => text.to_owned(),
        (true, true) => "a prompt".to_owned(),
    }
}

pub(crate) fn minutes(asked: &PendingQuestion, now: f64) -> u64 {
    ((now - asked.at).max(0.0) / 60.0).round() as u64
}

/// The fields the owner's item adds once the request is raised.
pub(crate) fn owner_fields(team: &Team, member: &Member, now: f64) -> Option<Value> {
    let (asked, raised) = of(member)?;
    let raised = raised?;
    let session = member.session.as_ref();
    Some(json!({"escalated":true,"raised_at":raised.at,"minutes":minutes(asked, now),"screen":raised.screen,
        "session":session.map(|s| s.name.clone()).unwrap_or_default(),"host":session.map(|s| s.host.clone()).unwrap_or_default(),
        "tool":title(asked),"reason":asked.card["answer_unavailable_reason"],"team_name":team.name}))
}

/// The phone alert on the lead's row, one per raised request: its `kind` says
/// it is an alert to open the member's Terminal, never a question to answer.
pub(crate) fn mobile_item(team: &Team, member: &Member) -> Option<Value> {
    use sha2::{Digest, Sha256};
    let (asked, raised) = of(member)?;
    let raised = raised?;
    let id = format!("team-terminal-prompt:{}:{}:{}", team.id, member.id, asked.question_id);
    let hash = format!("{:x}", Sha256::digest(format!("{id}\n{}", raised.at).as_bytes()));
    let screen: Vec<String> = raised.screen[raised.screen.len().saturating_sub(PHONE_LINES)..]
        .iter()
        .map(|line| line.chars().take(PHONE_LINE_CHARS).collect())
        .collect();
    let session = member.session.as_ref();
    Some(json!({"question_id":id,"question_hash":hash,"kind":"team_terminal_prompt","team":team.id,"team_name":team.name,
        "member":team.role_of(&member.id),"member_id":member.id,"session":session.map(|s| s.name.clone()).unwrap_or_default(),
        "host":session.map(|s| s.host.clone()).unwrap_or_default(),"tool":title(asked),"screen":screen,
        "action":"open_terminal","action_label":"Open Terminal","since":raised.at}))
}

/// The lines the lead's starved-member text adds: every member that waits on
/// a request only its Terminal takes, so the lead names the real blocker.
pub(crate) fn lead_lines(team: &Team, now: f64) -> String {
    let mut text = String::new();
    for member in team.members.iter().filter(|member| member.state != MemberState::Removed) {
        let Some((asked, raised)) = of(member) else { continue };
        let name = team.name_of(&member.id);
        let session = member.session.as_ref().map(|s| s.name.as_str()).unwrap_or_default();
        text += &format!(
            "  {name} waits {} min in {session} on a prompt only its Terminal takes: {}. {}\n",
            minutes(asked, now),
            title(asked),
            if raised.is_some() {
                "Zerus raised it to the owner with the screen; only the owner can answer it there (Open Terminal)."
            } else {
                "Only the owner can answer it in that Terminal; Zerus raises it to the owner after the defer time."
            }
        );
    }
    if text.is_empty() {
        text
    } else {
        format!("Waiting in a Terminal:\n{text}")
    }
}
