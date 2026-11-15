use zerus_team_plan::draft::DraftInput;

pub(super) fn check(phase: u32, draft: &DraftInput) -> Result<(), String> {
    if phase == 0 {
        return Err("The intake queue is not a planned draft.".into());
    }
    if draft.title.trim().is_empty() || draft.nodes.is_empty() || draft.nodes.len() > 200 {
        return Err("A phase title and 1 to 200 draft tasks are required.".into());
    }
    if serde_json::to_vec(draft).map_err(|e| e.to_string())?.len() > 250_000 {
        return Err("The draft exceeds its request bound.".into());
    }
    if let Some(p) = zerus_team_plan::name_problem(&draft.title, "The phase title", 200) {
        return Err(p.message);
    }
    for node in &draft.nodes {
        if node.id == 0
            || node.title.trim().is_empty()
            || node.criteria.is_empty()
            || node.criteria.len() > 32
        {
            return Err(
                "Every draft task needs its identity, title and acceptance criteria.".into(),
            );
        }
        if let Some(p) = zerus_team_plan::name_problem(&node.title, "The task title", 200) {
            return Err(p.message);
        }
    }
    Ok(())
}
