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
/// Task 166: the lead's page is one readable page of plain prose with worked
/// examples, 4 to 6 KB (also in a CRLF checkout); every other page keeps 2400.
const LEAD_LIMIT: usize = 6000;
const LEAD_MIN: usize = 4000;
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

    /// Task 166: every page is at most 2400 bytes (about 600 tokens), except the
    /// lead's one readable page.
    fn limit(name: &str) -> usize {
        if name == "team-lead" { LEAD_LIMIT } else { 2400 }
    }

    /// Lower-cased with every run of white space one space, so a phrase holds
    /// across a line break and a capital.
    fn flat(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
    }

    fn has(page: &str, phrase: &str) -> bool {
        flat(page).contains(&flat(phrase))
    }

    #[test]
    fn pages_stay_small_plain_and_free_of_local_details() {
        for (name, text) in pages() {
            // About four bytes a token: a page costs an agent at most ~600.
            assert!(text.len() <= limit(name), "{name} has {} bytes", text.len());
            // A CRLF checkout adds one byte for each LF that has none: the
            // page must still fit (N8).
            let bare = text.matches('\n').count() - text.matches("\r\n").count();
            let crlf = text.len() + bare;
            assert!(crlf <= limit(name), "{name} has {crlf} bytes in a CRLF checkout");
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
        assert!(has(lead, "hgs team profile --json"), "{lead}");
        assert!(has(lead, "top three losses"), "{lead}");
        assert!(has(lead, "tokens per accepted task"), "{lead}");
        assert!(has(lead, "unknown, never zero"), "{lead}");
    }

    /// U61, task 158: the lead protocol says an approved phase runs alongside,
    /// what a task still waits for, how Auto accepts a draft, and keeps the
    /// review rule; start ahead and work ahead are gone.
    #[test]
    fn the_lead_protocol_names_the_one_phase_start_model() {
        let lead = TOPICS.iter().find(|(name, _)| *name == "team-lead").unwrap().1;
        assert!(has(lead, "Auto drafts (U61)"), "{lead}");
        assert!(has(lead, "an approved phase runs alongside earlier ones"), "{lead}");
        assert!(has(lead, "files an open task writes"), "{lead}");
        assert!(!has(lead, "start-ahead") && !has(lead, "Work ahead"), "{lead}");
        assert!(has(lead, "Review drafts") && has(lead, "need the owner"), "{lead}");
        // #740: a question to the owner is a proposal, never only prose.
        assert!(has(lead, "a question for the owner is a proposal, never only"), "{lead}");
        let propose = pages().into_iter().find(|(name, _)| *name == "team-propose").unwrap().1;
        assert!(propose.contains(r#"{"kind":"question","question""#) && propose.contains("never only in your reply"), "{propose}");
        assert!(has(lead, "no draft runs before approval"), "{lead}");
    }

    /// #742 / task #220: no agent installs a tool. The lead's page names the
    /// tool_missing item and the install-tool card; the tests' page keeps the
    /// owner's offer for a run outside a team.
    #[test]
    fn agents_propose_a_missing_tool_and_never_install_it() {
        let lead = TOPICS.iter().find(|(topic, _)| *topic == "team-lead").unwrap().1;
        assert!(lead.contains("Never install a tool") && lead.contains("tool_missing"), "{lead}");
        assert!(lead.contains("decide-tool") && lead.contains("install-tool") && lead.contains("only the owner allows an install"), "{lead}");
        let tests = TOPICS.iter().find(|(topic, _)| *topic == "tests").unwrap().1;
        assert!(tests.contains("Never install a tool") && tests.contains("question card"), "{tests}");
        assert!(tests.contains("hgs test tools") && tests.contains("work without it"), "{tests}");
    }

    /// Owner 2026-10-31: the lead protocol says every task gets a time estimate.
    #[test]
    fn the_lead_protocol_says_every_task_gets_an_estimate() {
        let lead = TOPICS.iter().find(|(name, _)| *name == "team-lead").unwrap().1;
        assert!(has(lead, "Every task gets an estimate"), "{lead}");
        // Task #60: follow-ups name their origin.
        assert!(has(lead, "\"origin\":ID") && has(lead, "no dep"), "{lead}");
    }

    /// U63 (owner, 2026-10-31): the lead protocol says an accepted phase that
    /// has not started changes on the owner's word, how a phase pauses, and
    /// that a phase needing the owner's acceptance holds the later ones.
    #[test]
    fn the_lead_protocol_names_phase_control() {
        let lead = TOPICS.iter().find(|(name, _)| *name == "team-lead").unwrap().1;
        assert!(has(lead, "Accepted phase not started"), "{lead}");
        assert!(has(lead, "return-to-draft re-approves"), "{lead}");
        assert!(has(lead, "pause-phase {phase, reason}"), "{lead}");
        assert!(has(lead, "accept-phase (a card)"), "{lead}");
        let tree = TOPICS.iter().find(|(name, _)| *name == "team-tree").unwrap().1;
        assert!(tree.contains("hgs team sub-lead"), "{tree}");
    }

    /// Task #21 (owner, 2026-11-05): the lead proposes every owner-gated change
    /// as a card and never asks the owner to type a phrase.
    #[test]
    fn the_lead_proposes_owner_gates_and_never_asks_for_a_phrase() {
        let lead = TOPICS.iter().find(|(name, _)| *name == "team-lead").unwrap().1;
        assert!(has(lead, "Owner gates are cards") && has(lead, "Never ask the owner to type a phrase"), "{lead}");
        // Task 97: a deferred prompt never stops the lead.
        assert!(has(lead, "denied and queued for the owner; do everything else"), "{lead}");
        // `hgs team decide` takes the owner's own words (task 156); a typed phrase is never asked.
        assert!(!has(lead, "owner's sentence"), "{lead}");
        let propose = TOPICS.iter().find(|(name, _)| *name == "team-propose").unwrap().1;
        assert!(has(propose, "Never ask the owner to type a phrase"), "{propose}");
        for kind in crate::state::teams::ops::OWNER_OPS {
            assert!(propose.contains(kind), "team-propose omits {kind}");
        }
        for kind in ["\"relaunch\"", "\"iron-rules\"", "base_branch", "full_tests", "max_members"] {
            assert!(propose.contains(kind), "team-propose omits {kind}");
        }
    }

    /// Task 166 (owner, 2026-11-07): the lead's guide is one page of plain prose
    /// with a worked example for each thing a lead does, and it does not send the
    /// lead to a dozen other pages.
    #[test]
    fn the_lead_page_is_one_readable_page_with_worked_examples() {
        let lead = TOPICS.iter().find(|(name, _)| *name == "team-lead").unwrap().1;
        // Only a bare LF grows in a CRLF checkout (as in the small-pages test below).
        let crlf = lead.len() + lead.matches('\n').count() - lead.matches("\r\n").count();
        assert!((LEAD_MIN..=LEAD_LIMIT).contains(&lead.len()) && crlf <= LEAD_LIMIT, "{} bytes", lead.len());
        for example in ["Plan a goal.", "Run phases alongside.", "Decide reports.", "Park a blocked task.", "Record a decision.", "Publish through the release gate."] {
            assert!(lead.contains(example), "no example: {example}");
        }
        // Each example shows its command.
        for command in ["hgs team plan --json", "hgs team extend --json", "hgs team accept --json", "hgs team reply --json", "hgs team decide --json", "release-candidate"] {
            assert!(lead.contains(command), "{command}");
        }
        // Prose, not telegraphic fragments: most lines are nearly full width.
        let lines: Vec<&str> = lead.lines().filter(|line| !line.trim().is_empty()).collect();
        let long = lines.iter().filter(|line| line.len() >= 80).count();
        assert!(long * 2 >= lines.len(), "{long} long lines of {}", lines.len());
        // At most a handful of other pages, each an optional reference.
        let mut topics: Vec<&str> = lead
            .split("hgs guide ")
            .skip(1)
            .map(|rest| rest.split(|c: char| !c.is_ascii_alphanumeric() && c != '-').next().unwrap_or_default())
            .collect();
        topics.sort_unstable();
        topics.dedup();
        assert!(topics.len() <= 6, "{topics:?}");
        for topic in &topics {
            assert!(TOPICS.iter().any(|(name, _)| name == topic), "unknown topic {topic}");
        }
        // The lead's text left the member's page, which is the member's now.
        let member = TOPICS.iter().find(|(name, _)| *name == "team").unwrap().1;
        assert!(has(member, "hgs team report") && has(member, "hgs guide team-lead") && !has(member, "hgs team plan"), "{member}");
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
