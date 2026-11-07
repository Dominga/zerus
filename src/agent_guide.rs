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
    ("team-input", include_str!("agent_guide/team-input.md")),
    ("team-tree", include_str!("agent_guide/team-tree.md")),
    ("team-run", include_str!("agent_guide/team-run.md")),
    ("team-safe", include_str!("agent_guide/team-safe.md")),
    ("team-propose", include_str!("agent_guide/team-propose.md")),
    // Task 128: a team's RustCodeLore memory.
    ("team-lore", include_str!("agent_guide/team-lore.md")),
    // Task 160: park a task on an outside event, never poll.
    ("team-park", include_str!("agent_guide/team-park.md")),
    ("team-broker", include_str!("agent_guide/team-broker.md")),
    ("team-cpp", include_str!("agent_guide/team-cpp.md")),
    ("run", include_str!("agent_guide/run.md")),
    // WhiteWh fork (U62): the project's affected tests.
    ("tests", include_str!("agent_guide/tests.md")),
    ("designer", include_str!("agent_guide/designer.md")),
    ("launch", include_str!("agent_guide/launch.md")),
    ("stop", include_str!("agent_guide/stop.md")),
    ("tasks", include_str!("agent_guide/tasks.md")),
];
const USAGE: &str = "usage: hgs guide [send|wait|team|team-tests|team-lead|team-custom|team-draft|team-input|team-tree|team-run|team-safe|team-propose|team-lore|team-park|team-broker|team-cpp|run|tests|designer|launch|stop|tasks]";

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

    /// Owner (2026-11-01): a member's question goes through `hgs team ask`,
    /// never a report.
    #[test]
    fn the_member_page_sends_a_question_through_team_ask() {
        let team = TOPICS.iter().find(|(name, _)| *name == "team").unwrap().1;
        assert!(team.contains("A question goes through hgs team ask, never a report"), "{team}");
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

    /// U61, task 158: the lead protocol says an approved phase runs alongside,
    /// what a task still waits for, how Auto accepts a draft, and keeps the
    /// review rule; start ahead and work ahead are gone.
    #[test]
    fn the_lead_protocol_names_the_one_phase_start_model() {
        let lead = TOPICS.iter().find(|(name, _)| *name == "team-lead").unwrap().1;
        assert!(lead.contains("Auto drafts (U61)"), "{lead}");
        assert!(lead.contains("an approved phase runs alongside earlier ones"), "{lead}");
        assert!(lead.contains("files an open task writes"), "{lead}");
        assert!(!lead.contains("start-ahead") && !lead.contains("Work ahead"), "{lead}");
        assert!(lead.contains("Review drafts") && lead.contains("need the owner"), "{lead}");
        // #740: a question to the owner is a proposal, never only prose.
        assert!(lead.contains("a question for the owner is a proposal, never only"), "{lead}");
        let propose = pages().into_iter().find(|(name, _)| *name == "team-propose").unwrap().1;
        assert!(propose.contains(r#"{"kind":"question","question""#) && propose.contains("never only in your reply"), "{propose}");
        assert!(lead.contains("no draft runs before approval"), "{lead}");
    }

    /// #742: no agent installs a tool; the lead's and the tests' pages say a
    /// missing one is offered to the owner and proposed on the question card.
    #[test]
    fn agents_propose_a_missing_tool_and_never_install_it() {
        for name in ["team-lead", "tests"] {
            let page = TOPICS.iter().find(|(topic, _)| *topic == name).unwrap().1;
            assert!(page.contains("Never install a tool"), "{name}");
            assert!(page.contains("question card"), "{name}");
        }
        let tests = TOPICS.iter().find(|(topic, _)| *topic == "tests").unwrap().1;
        assert!(tests.contains("hgs test tools") && tests.contains("work without it"), "{tests}");
    }

    /// Owner 2026-10-31: the lead protocol says every task gets a time estimate.
    #[test]
    fn the_lead_protocol_says_every_task_gets_an_estimate() {
        let lead = TOPICS.iter().find(|(name, _)| *name == "team-lead").unwrap().1;
        assert!(lead.contains("Every task gets an estimate"), "{lead}");
        // Task #60: follow-ups name their origin.
        assert!(lead.contains("\"origin\":ID") && lead.contains("no dep"), "{lead}");
    }

    /// U63 (owner, 2026-10-31): the lead protocol says an accepted phase that
    /// has not started changes on the owner's word, how a phase pauses, and
    /// that a phase needing the owner's acceptance holds the later ones.
    #[test]
    fn the_lead_protocol_names_phase_control() {
        let lead = TOPICS.iter().find(|(name, _)| *name == "team-lead").unwrap().1;
        assert!(lead.contains("Accepted phase not started"), "{lead}");
        assert!(lead.contains("return-to-draft re-approves"), "{lead}");
        assert!(lead.contains("pause-phase {phase, reason}"), "{lead}");
        assert!(lead.contains("accept-phase (a card)"), "{lead}");
        let tree = TOPICS.iter().find(|(name, _)| *name == "team-tree").unwrap().1;
        assert!(tree.contains("hgs team sub-lead"), "{tree}");
    }

    /// Task #21 (owner, 2026-11-05): the lead proposes every owner-gated change
    /// as a card and never asks the owner to type a phrase.
    #[test]
    fn the_lead_proposes_owner_gates_and_never_asks_for_a_phrase() {
        let lead = TOPICS.iter().find(|(name, _)| *name == "team-lead").unwrap().1;
        assert!(lead.contains("Owner gates are cards") && lead.contains("Never ask the owner to type a phrase"), "{lead}");
        // Task 97: a deferred prompt never stops the lead.
        assert!(lead.contains("denied and queued for the owner; do everything else"), "{lead}");
        assert!(!lead.contains("owner_word") && !lead.contains("owner's sentence"), "{lead}");
        let propose = TOPICS.iter().find(|(name, _)| *name == "team-propose").unwrap().1;
        assert!(propose.contains("Never ask the owner to type a phrase"), "{propose}");
        for kind in crate::state::teams::ops::OWNER_OPS {
            assert!(propose.contains(kind), "team-propose omits {kind}");
        }
        for kind in ["\"relaunch\"", "\"iron-rules\"", "base_branch", "full_tests", "max_members"] {
            assert!(propose.contains(kind), "team-propose omits {kind}");
        }
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
