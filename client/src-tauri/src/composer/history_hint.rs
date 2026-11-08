//! Advisory only: text equality never settles a deferred input or its draft.
use super::*;

pub(super) fn shown(hub: &Hub, composer: &Composer) -> bool {
    if !composer.uncertain
        || composer.draft.text.trim().is_empty()
        || composer.draft.text.len() > MAX_TEXT_BYTES
        || hub.open_key.as_deref() != Some(composer.key.as_str())
    {
        return false;
    }
    let Some(held) = composer.checkpoint.as_ref() else {
        return false;
    };
    let Some(details) = hub.followed.details.as_deref() else {
        return false;
    };
    let child = hub.open.as_ref().and_then(|s| s.subagent.as_ref());
    let conversation = if child.is_some() {
        "parent_conversation_id"
    } else {
        "conversation_id"
    };
    if held.run_id.is_empty()
        || held.conversation_id.is_empty()
        || details["run_id"].as_str() != Some(&held.run_id)
        || details[conversation].as_str() != Some(&held.conversation_id)
    {
        return false;
    }
    // The followed window already holds at most EVENTS rows. No history read,
    // retained text cache, checkpoint update or delivery inference is added.
    hub.followed.events.iter().any(|event| {
        let agent = event["agent_id"].as_str().unwrap_or("");
        let same_agent = child.map_or(agent.is_empty(), |child| {
            agent == child.agent_id
                || (agent.is_empty() && details["agent_id"].as_str() == Some(&child.agent_id))
        });
        same_agent
            && matches!(
                event["type"].as_str(),
                Some("UserPromptSubmit" | "UserPromptQueued" | "TurnStarted")
            )
            && event["detail"].as_str() == Some(composer.draft.text.as_str())
    })
}
