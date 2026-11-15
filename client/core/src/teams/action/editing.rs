//! Checks for the person-authored live-team edits. The CLI/store still decide.
use super::types::TeamAction;
use zerus_team_plan::catalog::AccountChoice;
fn field(value: &str, what: &str, limit: usize) -> Result<(), String> {
    zerus_team_plan::name_problem(value, what, limit).map_or(Ok(()), |p| Err(p.message))
}
pub(super) fn check(action: &TeamAction) -> Result<(), String> {
    match action {
        TeamAction::RenameTeam { name } => {
            if name.trim().is_empty() {
                return Err("A team name is required.".into());
            }
            field(name, "The team name", 256)
        }
        TeamAction::SetGoal { goal } => {
            if goal.len() > 16_384 {
                Err("The team goal exceeds its size limit.".into())
            } else {
                Ok(())
            }
        }
        TeamAction::AddMember {
            role: _,
            agent,
            model,
            effort,
            account,
        }
        | TeamAction::SwapMember {
            member: _,
            agent,
            model,
            effort,
            account,
        } => {
            let id = match action {
                TeamAction::AddMember { role, .. } => role,
                TeamAction::SwapMember { member, .. } => member,
                _ => unreachable!(),
            };
            if let Some(p) = zerus_team_plan::id_problem(id, "The member or role ID") {
                return Err(p.message);
            }
            if !["claude", "codex", "kimi", "dsh"].contains(&agent.as_str()) {
                return Err("Choose a supported team agent.".into());
            }
            if agent == "dsh" && !model.is_empty() {
                return Err("DeepSeek model is managed in its native account.".into());
            }
            if !matches!(agent.as_str(), "claude" | "codex") && !effort.is_empty() {
                return Err("This agent has no supported launch effort argument.".into());
            }
            field(model, "The member model", 256)?;
            field(effort, "The member effort", 64)?;
            if let AccountChoice::Named { account } = account {
                if account.trim().is_empty() {
                    return Err("A named account needs its ID.".into());
                }
                field(account, "The member account", 256)?;
            }
            Ok(())
        }
        _ => unreachable!("only editing actions reach this helper"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seat_input_keeps_explicit_choices_and_swap_requires_confirmation() {
        let op = TeamAction::SwapMember {
            member: "m1".into(),
            agent: "codex".into(),
            model: "example-model".into(),
            effort: "high".into(),
            account: AccountChoice::Named {
                account: "example-account".into(),
            },
        };
        assert!(op.check().is_ok());
        assert_eq!(
            op.input()["account"],
            serde_json::json!({"mode":"named","account":"example-account"})
        );
        let confirmation = op.confirmation("Readable team").unwrap();
        assert!(confirmation
            .text
            .contains("old session stays outside the team"));
        assert!(confirmation.text.contains("no open or unsettled work"));
        assert_eq!(op.op(), "swap-member");
    }
    #[test]
    fn editing_fields_refuse_invalid_ids_and_controls_but_goal_can_be_cleared() {
        for member in ["", "../other"] {
            assert!(TeamAction::SwapMember {
                member: member.into(),
                agent: "claude".into(),
                model: String::new(),
                effort: String::new(),
                account: AccountChoice::Freest
            }
            .check()
            .is_err());
        }
        assert!(TeamAction::RenameTeam {
            name: "a\nb".into()
        }
        .check()
        .is_err());
        assert!(TeamAction::SetGoal {
            goal: String::new()
        }
        .check()
        .is_ok());
    }
}
