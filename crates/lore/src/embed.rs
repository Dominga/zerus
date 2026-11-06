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
            return Err(LoreError::new("the fixed embedding corpus is too small"));
        }
        Ok(self.0[..texts.len()].to_vec())
    }
}

pub type Shared = Arc<dyn Embeddings>;
