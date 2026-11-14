//! A member whose terminal holds input (zerus-63rru.79). The task command
//! refuses a hand-out before any key when the member's terminal is not at an
//! empty agent prompt (a question, a dialog or an unsent draft), and says so
//! by a typed cause. That is not a failed hand-out but a fact about the
//! member: its terminal is not free. It is kept under `terminal:<member>` in
//! this actor's ledger, one row per member, with the stamp of the member's
//! fleet row when the refusal was observed.
//!
//! While it stands the member is not free for the selection, so the next pass
//! offers the task to another free member, and a ready task that only this
//! member can take gets zerus-63rru.55's card after its period. It ends when
//! the member's row changes, or once the retry table's delay for its refusals
//! has passed (1, 2, 4, 8, then 15 minutes): a member whose row never shows the
//! question is then tried again, not on every pass, and a further refusal
//! waits longer. A delivery submitted to the member ends it.
use super::*;
use serde::{Deserialize, Serialize};

const ROWS: &str = "terminal:";

/// The fact as stored: the stamp of the member's row when the refusal was
/// observed (none when it had no row then), the refusals so far, and when it
/// lapses.
#[derive(Debug, Serialize, Deserialize)]
struct Waiting {
    stamp: Option<String>,
    refusals: u32,
    until: i64,
    /// Which dialog the refusal's gate knew, when one did (zerus-63rru.83).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dialog: Option<String>,
}

/// A member's terminal fact as a pass finds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum TerminalState {
    /// None, or its row changed.
    None,
    /// The member is not free; the dialog its gate knew, when one did.
    Standing {
        dialog: Option<String>,
    },
    /// Its time passed: the member is tried again, and a ready task's card
    /// for it stays as it is until that try settles.
    Lapsed,
}

impl Runtime {
    /// Records a refusal of the task command's terminal gate for `member`.
    pub(super) fn terminal_refused(
        &mut self,
        team: &str,
        member: &str,
        stamp: Option<String>,
        dialog: Option<String>,
        now: i64,
    ) -> Result<()> {
        let key = format!("{ROWS}{member}");
        let refusals = self
            .load::<Waiting>(team, &key)?
            .map_or(0, |w| w.refusals)
            .saturating_add(1);
        let until = now.saturating_add(retry::delay(refusals));
        self.put(
            team,
            &key,
            &Waiting {
                stamp,
                refusals,
                until,
                dialog,
            },
        )?;
        Ok(())
    }

    /// `member`'s terminal fact now. A changed row ends it at once and
    /// forgets its refusals. A lapsed one lets the member be tried again but
    /// keeps its count, so that a further refusal waits longer. A row that
    /// cannot be read leaves the fact as it is: unknown is not free.
    pub(super) fn terminal_state(
        &mut self,
        ports: &mut impl Ports,
        team: &Team,
        member: &Member,
        now: i64,
    ) -> Result<TerminalState> {
        let key = format!("{ROWS}{}", member.id);
        let Some(row) = self.ledger.get(&team.id, &key)? else {
            return Ok(TerminalState::None);
        };
        let w: Waiting =
            serde_json::from_value(row.data.clone()).map_err(crate::teams::store::storage)?;
        if w.stamp.is_some() {
            if let Ok(current) = ports.row_stamp(team, member) {
                if current != w.stamp {
                    self.ledger.remove(&row)?;
                    return Ok(TerminalState::None);
                }
            }
        }
        Ok(if now < w.until {
            TerminalState::Standing {
                dialog: w.dialog.clone(),
            }
        } else {
            TerminalState::Lapsed
        })
    }
    /// Whether `member`'s terminal fact stands now ([`Runtime::terminal_state`]).
    pub(super) fn terminal_standing(
        &mut self,
        ports: &mut impl Ports,
        team: &Team,
        member: &Member,
        now: i64,
    ) -> Result<bool> {
        Ok(matches!(
            self.terminal_state(ports, team, member, now)?,
            TerminalState::Standing { .. }
        ))
    }

    /// A delivery to `member` was submitted: its terminal took input.
    pub(super) fn terminal_free(&mut self, team: &str, member: &str) -> Result<()> {
        if let Some(row) = self.ledger.get(team, &format!("{ROWS}{member}"))? {
            self.ledger.remove(&row)?;
        }
        Ok(())
    }
}
