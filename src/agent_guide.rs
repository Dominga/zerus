//! `hgs guide`: how an agent reads, messages, launches and stops other Zerus
//! sessions. Every byte printed here lands in an agent's context, so the index
//! stays short and each topic is read only when the agent needs it. The text
//! ships inside the binary and therefore matches the commands this hgs has.
use crate::cli::{Error, Result};

const INDEX: &str = include_str!("agent_guide/index.md");
const TOPICS: &[(&str, &str)] = &[
    ("send", include_str!("agent_guide/send.md")),
    ("wait", include_str!("agent_guide/wait.md")),
    // WhiteWh fork: agent teams.
    ("team", include_str!("agent_guide/team.md")),
    ("team-tests", include_str!("agent_guide/team-tests.md")),
    ("team-lead", include_str!("agent_guide/team-lead.md")),
    ("team-custom", include_str!("agent_guide/team-custom.md")),
    ("team-draft", include_str!("agent_guide/team-draft.md")),
    ("team-tree", include_str!("agent_guide/team-tree.md")),
    ("team-run", include_str!("agent_guide/team-run.md")),
    ("team-safe", include_str!("agent_guide/team-safe.md")),
    ("team-propose", include_str!("agent_guide/team-propose.md")),
    ("run", include_str!("agent_guide/run.md")),
    // WhiteWh fork (U62): the project's affected tests.
    ("tests", include_str!("agent_guide/tests.md")),
    ("designer", include_str!("agent_guide/designer.md")),
    ("launch", include_str!("agent_guide/launch.md")),
    ("stop", include_str!("agent_guide/stop.md")),
    ("tasks", include_str!("agent_guide/tasks.md")),
];
const USAGE: &str = "usage: hgs guide [send|wait|team|team-tests|team-lead|team-custom|team-draft|team-tree|team-run|team-safe|team-propose|run|tests|designer|launch|stop|tasks]";

pub(crate) fn dispatch(args: &[String]) -> Result<i32> {
    let text = match args {
        [] => INDEX,
        [topic] => TOPICS
            .iter()
            .find(|(name, _)| name == topic)
            .map(|(_, text)| *text)
            .ok_or_else(|| Error::new(1, format!("unknown guide topic: {topic}; {USAGE}")))?,
        _ => return Err(Error::new(1, USAGE)),
    };
    print!("{text}");
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pages() -> impl Iterator<Item = (&'static str, &'static str)> {
        std::iter::once(("index", INDEX)).chain(TOPICS.iter().copied())
    }

    #[test]
    fn pages_stay_small_plain_and_free_of_local_details() {
        for (name, text) in pages() {
            // About four bytes a token: a page costs an agent at most ~600.
            assert!(text.len() <= 2400, "{name} has {} bytes", text.len());
            // A CRLF checkout adds one byte for each LF that has none: the
            // page must still fit (N8).
            let bare = text.matches('\n').count() - text.matches("\r\n").count();
            let crlf = text.len() + bare;
            assert!(crlf <= 2400, "{name} has {crlf} bytes in a CRLF checkout");
            assert!(text.is_ascii(), "{name} is not ASCII");
            assert!(text.ends_with('\n') && !text.contains('\t'), "{name}");
            assert!(text.lines().all(|line| line.len() <= 100), "{name} has a long line");
            // Spelled in parts: the source privacy guard rejects the literal.
            for root in ["w", "home", "Users"] {
                assert!(!text.contains(&format!("/{root}/")), "{name} names a local path");
            }
            assert!(!text.contains("~/") && !text.contains(":\\"), "{name} names a local path");
        }
    }

    #[test]
    fn index_lists_every_topic_and_usage_names_them() {
        for (name, _) in TOPICS {
            assert!(INDEX.contains(&format!("hgs guide {name} ")), "index omits {name}");
            assert!(USAGE.contains(name), "usage omits {name}");
        }
    }

    #[test]
    fn named_commands_exist_in_this_hgs() {
        let usage = crate::cli::USAGE;
        for (name, text) in pages() {
            for line in text.lines() {
                let mut rest = line;
                while let Some(at) = rest.find("hgs ") {
                    rest = &rest[at + 4..];
                    let command = rest
                        .split_whitespace()
                        .find(|word| !word.starts_with('@') && *word != "[@h]")
                        .unwrap_or_default()
                        .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-');
                    // `hgs [@h] AGENT DIRECTORY` launches an agent; -h is help.
                    if command.is_empty() || command == "-h" || command.chars().all(|c| c.is_ascii_uppercase()) {
                        continue;
                    }
                    assert!(
                        usage.contains(&format!("hgs {command} "))
                            || usage.contains(&format!("hgs [@host] {command} "))
                            || usage.contains(&format!("hgs [@host] {command}|")),
                        "{name} names unknown command {command:?}: {line}"
                    );
                }
            }
        }
    }

    /// U56: the draft page shows seen with its request_id, and the custom page
    /// names keep_assignees.
    #[test]
    fn the_draft_seen_and_keep_assignees_are_documented() {
        let draft = TOPICS.iter().find(|(name, _)| *name == "team-draft").unwrap().1;
        let seen = draft.lines().find(|line| line.contains("\"action\":\"seen\"")).expect("a seen example");
        assert!(seen.contains("\"request_id\":\"UUID\""), "{seen}");
        let custom = TOPICS.iter().find(|(name, _)| *name == "team-custom").unwrap().1;
        assert!(custom.contains("keep_assignees (off)"));
    }

    /// U38: the lead protocol names the profile, the top three losses and the
    /// per-role numbers, and says unknown is never zero.
    #[test]
    fn the_lead_protocol_names_the_profile_and_its_top_three_losses() {
        let lead = TOPICS.iter().find(|(name, _)| *name == "team-lead").unwrap().1;
        assert!(lead.contains("hgs team profile --json"), "{lead}");
        assert!(lead.contains("top three losses"), "{lead}");
        assert!(lead.contains("tokens per accepted task"), "{lead}");
        assert!(lead.contains("unknown, never zero"), "{lead}");
    }

    /// U61: the lead protocol says an auto-accepted draft stays editable until
    /// its first hand-out, when to start it ahead, and keeps the review rule.
    #[test]
    fn the_lead_protocol_names_auto_drafts_and_start_ahead() {
        let lead = TOPICS.iter().find(|(name, _)| *name == "team-lead").unwrap().1;
        assert!(lead.contains("Auto drafts (U61)"), "{lead}");
        assert!(lead.contains("stays a draft you edit"), "{lead}");
        assert!(lead.contains("hgs team start-ahead --json"), "{lead}");
        assert!(lead.contains("members are idle") && lead.contains("independent"), "{lead}");
        assert!(lead.contains("Review drafts") && lead.contains("need the owner"), "{lead}");
        // #740: a question to the owner is a proposal, never only prose.
        assert!(lead.contains("a question for the owner is a proposal, never only"), "{lead}");
        let propose = pages().into_iter().find(|(name, _)| *name == "team-propose").unwrap().1;
        assert!(propose.contains(r#"{"kind":"question","question""#) && propose.contains("never only in your reply"), "{propose}");
        assert!(lead.contains("Work ahead is for review drafts"), "{lead}");
    }

    #[test]
    fn topics_and_arguments_are_exact() {
        assert!(dispatch(&[]).is_ok());
        for (name, _) in TOPICS {
            assert!(dispatch(&[(*name).into()]).is_ok());
        }
        assert!(dispatch(&["Send".into()]).is_err());
        assert!(dispatch(&["send".into(), "wait".into()]).is_err());
    }
}
