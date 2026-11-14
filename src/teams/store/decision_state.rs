use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use serde::{Deserialize, Serialize};

/// The one decision lifecycle, shared by recording, stop, card approval and
/// settlement. Stop provenance and integration outcomes are separate facts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionState {
    Pending,
    Ready,
    AwaitingOwner,
    EffectReserved,
    Settled,
}
impl DecisionState {
    /// Whether a node's unsettled decision of `kind` in this state can still
    /// be replaced by a lead's or the person's Return, Reassign or Cancel: a
    /// Return or Reassign that has not started. Reserving its reopen is what
    /// moves it on from Pending; an acceptance and a cancel are never
    /// replaced. The operations (`Context::unstarted`) and the queue row's
    /// `pending_replaceable` both read this one rule.
    pub fn replaceable(self, kind: &str) -> bool {
        matches!(kind, "return" | "reassign") && self == Self::Pending
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::AwaitingOwner => "awaiting_owner",
            Self::EffectReserved => "effect_reserved",
            Self::Settled => "settled",
        }
    }
}
impl ToSql for DecisionState {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(self.as_str().into())
    }
}
impl FromSql for DecisionState {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let text = value.as_str()?;
        [
            Self::Pending,
            Self::Ready,
            Self::AwaitingOwner,
            Self::EffectReserved,
            Self::Settled,
        ]
        .into_iter()
        .find(|state| state.as_str() == text)
        .ok_or_else(|| {
            FromSqlError::Other(Box::new(std::io::Error::other("Unknown decision state.")))
        })
    }
}
