//! Project and team memory: the CodeLore implementation in Rust
//! (RustCodeLore).
//!
//! This crate is the working tree the specification `docs/lore.md` describes:
//! it starts as a port of the owner's Python CodeLore and becomes the
//! implementation itself (CodeLore 1.0) in github.com/WhiteWh/CodeLore. It
//! never depends on Zerus.
//!
//! The first module is [`embed`], the built-in nomic-embed-text-v1.5
//! embeddings on candle. The store, search and the CLI join it from their own
//! tasks.

pub mod embed;

/// The number of dimensions every embedding of the built-in model has.
pub const EMBEDDING_DIMENSIONS: usize = embed::DIMENSIONS;

/// The model this crate embeds with, named in the index and in errors.
pub const EMBEDDING_MODEL: &str = embed::MODEL;

pub use embed::{DOCUMENT_PREFIX, QUERY_PREFIX};
