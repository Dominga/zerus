//! The candle side: the nomic-bert model, the tokenizer and the batches.
//!
//! Mean pooling over the attention mask and L2 normalisation are candle's own
//! `nomic_bert` helpers, so the vectors are the ones the Python CodeLore's
//! Ollama model produces (the port's conformance corpus in `tests/reference`
//! holds the Python vectors and the integration test compares the cosine).
use super::Error;
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::nomic_bert::{self, Config, NomicBertModel};
use std::path::Path;
use tokenizers::Tokenizer;

/// A loaded model and its tokenizer on the CPU.
pub struct Model {
    model: NomicBertModel,
    tokenizer: Tokenizer,
    device: Device,
    max_tokens: usize,
}

impl Model {
    /// Loads the three pinned files from `dir`. The caller checked their size
    /// (and, for an offline install, their SHA-256); a corrupt file fails here
    /// rather than producing garbage vectors.
    pub fn load(dir: &Path) -> Result<Self, Error> {
        let device = Device::Cpu;
        let config = std::fs::read(dir.join("config.json"))
            .map_err(|error| Error::Model(format!("cannot read config.json: {error}")))?;
        let config: Config = serde_json::from_slice(&config)
            .map_err(|error| Error::Model(format!("cannot parse config.json: {error}")))?;
        let weights = dir.join("model.safetensors");
        // SAFETY: the file was installed by the fetch after its pinned size and
        // SHA-256 matched (or verified by `verify` for an offline install), and
        // nothing writes it while it is mapped; the mapping is read-only.
        let builder = unsafe {
            VarBuilder::from_mmaped_safetensors(std::slice::from_ref(&weights), DType::F32, &device)
        }
        .map_err(|error| Error::Model(format!("cannot map {}: {error}", weights.display())))?;
        let model = NomicBertModel::load(builder, &config)
            .map_err(|error| Error::Model(format!("cannot load the model: {error}")))?;
        let tokenizer = Tokenizer::from_file(dir.join("tokenizer.json"))
            .map_err(|error| Error::Model(format!("cannot read tokenizer.json: {error}")))?;
        let max_tokens = config.n_positions.clamp(8, 8192);
        Ok(Self { model, tokenizer, device, max_tokens })
    }

    /// Embeds every text once, at most `batch_size` texts per forward pass and
    /// at most `threads` batches at a time. The result is in input order.
    pub fn embed(&self, texts: &[&str], batch_size: usize, threads: usize) -> Result<Vec<Vec<f32>>, Error> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let batch_size = batch_size.max(1);
        let threads = threads.clamp(1, 64);
        let batches: Vec<&[&str]> = texts.chunks(batch_size).collect();
        let mut vectors = Vec::with_capacity(texts.len());
        for group in batches.chunks(threads) {
            let mut rows: Vec<Vec<Vec<f32>>> = Vec::with_capacity(group.len());
            if group.len() == 1 {
                rows.push(self.embed_batch(group[0])?);
            } else {
                std::thread::scope(|scope| -> Result<(), Error> {
                    let handles: Vec<_> = group
                        .iter()
                        .map(|batch| scope.spawn(move || self.embed_batch(batch)))
                        .collect();
                    for handle in handles {
                        let row = handle
                            .join()
                            .map_err(|_| Error::Run("an embedding thread panicked".into()))??;
                        rows.push(row);
                    }
                    Ok(())
                })?;
            }
            for row in rows {
                vectors.extend(row);
            }
        }
        Ok(vectors)
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, Error> {
        let mut encoded: Vec<Vec<u32>> = Vec::with_capacity(texts.len());
        for text in texts {
            let encoding = self
                .tokenizer
                .encode(*text, true)
                .map_err(|error| Error::Tokenize(error.to_string()))?;
            let mut ids = encoding.get_ids().to_vec();
            ids.truncate(self.max_tokens);
            encoded.push(ids);
        }
        let width = encoded.iter().map(Vec::len).max().unwrap_or(0).max(1);
        let mut ids = vec![0u32; texts.len() * width];
        let mut mask = vec![0u32; texts.len() * width];
        for (row, tokens) in encoded.iter().enumerate() {
            for (column, id) in tokens.iter().enumerate() {
                ids[row * width + column] = *id;
                mask[row * width + column] = 1;
            }
        }
        let ids = Tensor::from_vec(ids, (texts.len(), width), &self.device).map_err(run)?;
        let mask = Tensor::from_vec(mask, (texts.len(), width), &self.device).map_err(run)?;
        let hidden = self
            .device
            .with_context(|| self.model.forward(&ids, None, Some(&mask)))
            .map_err(run)?;
        let pooled = nomic_bert::mean_pooling(&hidden, &mask).map_err(run)?;
        let normalised = nomic_bert::l2_normalize(&pooled).map_err(run)?;
        normalised.to_vec2::<f32>().map_err(run)
    }
}

fn run(error: candle_core::Error) -> Error {
    Error::Run(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directory_without_the_config_is_no_model() {
        let dir = std::env::temp_dir().join(format!("lore-model-load-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let error = Model::load(&dir).err().expect("a directory without the config is no model");
        assert!(matches!(error, Error::Model(_)), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
