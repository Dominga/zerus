//! Embeddings: a trait seam. Task 143 replaces this with candle/nomic-bert
//! (`nomic-embed-text-v1.5`, mean pooling, L2-normalised 768-d, query/document
//! prefixes); the weights are fetched separately and never bundled. No Ollama
//! and no ONNX runtime are used.
use crate::LoreError;
use std::sync::Arc;

pub trait Embeddings: Send + Sync {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, LoreError>;
}

/// A recorded corpus for tests: each text's vector, never a model call.
#[derive(Clone, Default)]
pub struct Fixed(pub Vec<Vec<f32>>);

impl Embeddings for Fixed {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, LoreError> {
        if texts.len() > self.0.len() {
            return Err(LoreError("the fixed embedding corpus is too small".into()));
        }
        Ok(self.0[..texts.len()].to_vec())
    }
}

pub type Shared = Arc<dyn Embeddings>;

/// The configured provider. Task 146 supplies the builtin candle model; until
/// then builtin and none both mean text-only search.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider { Builtin, None }

impl Provider {
    pub fn from_config(provider: &str) -> Self {
        if provider == "builtin" { Self::Builtin } else { Self::None }
    }
    /// The embedder a caller plugs into search, or None for text-only.
    pub fn embedder(self) -> Option<Shared> { None }
}
