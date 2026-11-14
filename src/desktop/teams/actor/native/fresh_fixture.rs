//! Boolean/result-only access to the production proof and final admission.
//! Test data and reservations still come from the fresh-member writers.
use super::*;

impl Native {
    pub(in crate::desktop::teams::actor) fn fixture_first_ready(
        &self,
        member: &Member,
        row: &Value,
    ) -> bool {
        self.first_on(member, row).is_some()
    }

    pub(in crate::desktop::teams::actor) fn fixture_first_admission(
        &self,
        target: (&Team, &Node, &Member, &str, &Value),
        before_admission: impl FnOnce(),
    ) -> Result<()> {
        let (team, node, member, id, row) = target;
        let first = self
            .first_on(member, row)
            .expect("the original terminal must be ready");
        before_admission();
        self.admit(team, node, Some(member), id, None, Some(&first))
            .map(drop)
    }
}
