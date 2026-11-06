//! The search seam: a local index today, Zerus's session history later through
//! the same trait. No Zerus type appears in this crate.
use crate::LoreError;

#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub id: String,
    pub title: String,
    pub score: f32,
}

/// A corpus the search fuses with the lore store. Zerus implements this for
/// session history; the crate only sees the trait.
pub trait Source: Send + Sync {
    fn search(&self, query: &str, limit: usize) -> Result<Vec<Hit>, LoreError>;
}
