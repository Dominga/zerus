//! Built-in embeddings for RustCodeLore: nomic-embed-text-v1.5 on candle.
//!
//! No Ollama, no ONNX runtime, no Python: the model runs in this process on the
//! CPU (candle-transformers' `nomic_bert`), with mean pooling and L2-normalised
//! 768-dimensional vectors. The texts carry the model's task prefixes exactly as
//! the Python CodeLore applies them: `search_document: ` for records and
//! `search_query: ` for a search query.
//!
//! The weights are never in the binary. [`fetch`] downloads them from
//! Hugging Face into the user cache, each file pinned by size and SHA-256, or
//! an offline install hands [`ModelSource::Local`] a directory that already
//! holds them. Without the model the crate answers [`Error::NoEmbeddings`] and
//! callers (search) fall back to FTS5; nothing here ever panics on its absence.
//!
//! The crate never depends on Zerus.
mod fetch;
mod model;

pub use fetch::fetch;

use std::path::{Path, PathBuf};

/// The model this crate embeds with.
pub const MODEL: &str = "nomic-embed-text-v1.5";
/// Every vector this model produces has this many dimensions.
pub const DIMENSIONS: usize = 768;
/// The task prefix of a record, as the Python CodeLore writes it.
pub const DOCUMENT_PREFIX: &str = "search_document: ";
/// The task prefix of a search query, as the Python CodeLore writes it.
pub const QUERY_PREFIX: &str = "search_query: ";
/// The Hugging Face repository the weights come from.
pub const REPOSITORY: &str = "nomic-ai/nomic-embed-text-v1.5";
/// The repository revision every download is resolved at, so the pinned
/// digests below name exact bytes and never a moving `main`.
pub const REVISION: &str = "e9b6763023c676ca8431644204f50c2b100d9aab";

/// One file of the model, pinned by its exact size and SHA-256.
#[derive(Clone, Copy, Debug)]
pub struct PinnedFile {
    pub name: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
}

/// The model's three files. The weights' digest is the one Hugging Face
/// publishes for the LFS object; the two small files' are computed from the
/// bytes at [`REVISION`] and checked again on every fetch and open.
pub const FILES: [PinnedFile; 3] = [
    PinnedFile {
        name: "model.safetensors",
        bytes: 546_938_168,
        sha256: "9e7d262b1fe5ea350782829496efa831901b77486bbde1cea54a4c822d010d5c",
    },
    PinnedFile {
        name: "tokenizer.json",
        bytes: 711_396,
        sha256: "d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66",
    },
    PinnedFile {
        name: "config.json",
        bytes: 2_538,
        sha256: "9ab00bd92cee80a569f708140b7b6c1661a65891ff3765b1519e181ba2f2c92b",
    },
];

/// What went wrong, and whether the caller should fall back to text search.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The model is not installed (or cannot be used): there are no
    /// embeddings, and the caller searches with FTS5 instead.
    #[error("no embeddings: {0}")]
    NoEmbeddings(String),
    /// The model's files are incomplete, changed or unreadable.
    #[error("the model files are incomplete or corrupt: {0}")]
    Model(String),
    /// The files could not be downloaded or verified.
    #[error("the model could not be fetched: {0}")]
    Fetch(String),
    /// A text could not be encoded.
    #[error("a text could not be encoded: {0}")]
    Tokenize(String),
    /// The model itself failed.
    #[error("the model failed: {0}")]
    Run(String),
}

/// Where the model's files come from.
#[derive(Clone, Copy, Debug)]
pub enum ModelSource<'a> {
    /// The user cache: `<cache>/models/nomic-embed-text-v1.5/` (or
    /// `LORE_MODEL_DIR`).
    Cache,
    /// A directory that already holds the three pinned files (an offline
    /// install).
    Local(&'a Path),
}

/// The user cache root CodeLore uses: `LORE_CACHE`, else the platform cache
/// folder under `codelore`.
pub fn cache_root() -> PathBuf {
    if let Some(configured) = std::env::var_os("LORE_CACHE").filter(|value| !value.is_empty()) {
        return PathBuf::from(configured);
    }
    #[cfg(windows)]
    {
        if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|value| !value.is_empty()) {
            return PathBuf::from(local).join("codelore");
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
            return PathBuf::from(home).join("Library/Caches/codelore");
        }
    }
    if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(xdg).join("codelore");
    }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache/codelore")
}

/// Where the model lives: `LORE_MODEL_DIR` when set (an offline install or a
/// test fixture), else the user cache's model folder.
pub fn default_model_dir() -> PathBuf {
    if let Some(configured) = std::env::var_os("LORE_MODEL_DIR").filter(|value| !value.is_empty()) {
        return PathBuf::from(configured);
    }
    cache_root().join("models").join(MODEL)
}

/// Whether the model can be used from `dir`, with the reason it cannot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub dir: PathBuf,
    pub available: bool,
    /// Why the model cannot be used (empty when it is available).
    pub reason: String,
}

impl Status {
    pub fn missing(dir: &Path, reason: impl Into<String>) -> Self {
        Self { dir: dir.to_path_buf(), available: false, reason: reason.into() }
    }
}

/// The model's state at `dir` without loading it: every pinned file present
/// and of the pinned size. The hashes are checked on fetch and by [`verify`].
pub fn status_at(dir: &Path) -> Status {
    for file in FILES {
        let path = dir.join(file.name);
        match std::fs::metadata(&path) {
            Ok(metadata) if metadata.len() == file.bytes => {}
            Ok(metadata) => {
                return Status::missing(
                    dir,
                    format!(
                        "{} is {} bytes, the pinned file is {} bytes; run `lore model fetch`",
                        path.display(),
                        metadata.len(),
                        file.bytes
                    ),
                )
            }
            Err(_) => {
                return Status::missing(
                    dir,
                    format!("{} is missing; run `lore model fetch`", path.display()),
                )
            }
        }
    }
    Status { dir: dir.to_path_buf(), available: true, reason: String::new() }
}

/// The default model's state (see [`status_at`]).
pub fn status() -> Status {
    status_at(&default_model_dir())
}

/// Checks every pinned file's size and SHA-256.
pub fn verify(dir: &Path) -> Result<(), Error> {
    fetch::verify_all(dir)
}

/// Downloads the model into the cache, or verifies an offline install, and
/// returns the directory holding it. `progress` is told each file's name, the
/// bytes so far and the total bytes.
pub fn fetch_model(
    source: ModelSource<'_>,
    progress: &mut dyn FnMut(&str, u64, u64),
) -> Result<PathBuf, Error> {
    fetch::fetch(source, progress)
}

/// The embedder: the loaded model and tokenizer, ready to embed.
pub struct Embedder {
    model: model::Model,
    batch_size: usize,
    threads: usize,
}

impl Embedder {
    /// Loads the model from the cache (or `LORE_MODEL_DIR`). The model is not
    /// downloaded here: a missing model is [`Error::NoEmbeddings`], so search
    /// falls back to FTS5 and `lore model fetch` is the owner's explicit step.
    pub fn open_cached() -> Result<Self, Error> {
        Self::open(ModelSource::Cache)
    }

    /// Loads the model from `source`. A local directory is verified (size and
    /// SHA-256) before it is trusted.
    pub fn open(source: ModelSource<'_>) -> Result<Self, Error> {
        let dir = match source {
            ModelSource::Cache => default_model_dir(),
            ModelSource::Local(path) => path.to_path_buf(),
        };
        let status = status_at(&dir);
        if !status.available {
            return Err(Error::NoEmbeddings(status.reason));
        }
        if matches!(source, ModelSource::Local(_)) {
            verify(&dir)?;
        }
        let model = model::Model::load(&dir)?;
        Ok(Self { model, batch_size: 32, threads: default_threads() })
    }

    /// The number of vectors per forward pass (1..=256).
    pub fn batch_size(&self) -> usize {
        self.batch_size
    }

    /// At most this many batches run at once (1..=64).
    pub fn threads(&self) -> usize {
        self.threads
    }

    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = bounded_batch(batch_size);
        self
    }

    pub fn with_threads(mut self, threads: usize) -> Self {
        self.threads = bounded_threads(threads);
        self
    }

    /// [`DIMENSIONS`].
    pub fn dimensions(&self) -> usize {
        DIMENSIONS
    }

    /// The embeddings of records, each text prefixed with `search_document: `
    /// (unless it already starts with a task prefix).
    pub fn embed_documents(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, Error> {
        let prefixed: Vec<String> = texts
            .iter()
            .map(|text| {
                if text.starts_with(DOCUMENT_PREFIX) || text.starts_with(QUERY_PREFIX) {
                    (*text).to_owned()
                } else {
                    format!("{DOCUMENT_PREFIX}{text}")
                }
            })
            .collect();
        let references: Vec<&str> = prefixed.iter().map(String::as_str).collect();
        self.model.embed(&references, self.batch_size, self.threads)
    }

    /// The embedding of a search query, prefixed with `search_query: ` (unless
    /// it already starts with a task prefix).
    pub fn embed_query(&self, text: &str) -> Result<Vec<f32>, Error> {
        let prefixed = if text.starts_with(DOCUMENT_PREFIX) || text.starts_with(QUERY_PREFIX) {
            text.to_owned()
        } else {
            format!("{QUERY_PREFIX}{text}")
        };
        let mut vectors = self.model.embed(&[prefixed.as_str()], 1, 1)?;
        vectors.pop().ok_or_else(|| Error::Run("the model returned no vector".into()))
    }

    /// Embeds texts with the prefix they already carry (an escape hatch for
    /// the conformance corpus and for callers that build their own text).
    pub fn embed_prefixed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, Error> {
        self.model.embed(texts, self.batch_size, self.threads)
    }
}

/// At most this many batches run at once by default: the physical cores, at
/// most four, so a loaded desktop keeps a core for the person.
fn default_threads() -> usize {
    std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
        .min(4)
}

/// One forward pass holds 1..=256 texts.
fn bounded_batch(batch_size: usize) -> usize {
    batch_size.clamp(1, 256)
}

/// One embedder runs 1..=64 batches at once.
fn bounded_threads(threads: usize) -> usize {
    threads.clamp(1, 64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pinned_model_is_the_python_one() {
        assert_eq!(DIMENSIONS, 768);
        assert_eq!(DOCUMENT_PREFIX, "search_document: ");
        assert_eq!(QUERY_PREFIX, "search_query: ");
        assert_eq!(FILES.len(), 3);
        // The weights are the model's only large file; the two others are the
        // tokenizer and the config, small enough to hash on every open.
        assert!(FILES[0].bytes > 500 * 1024 * 1024);
        for file in FILES {
            assert_eq!(file.sha256.len(), 64);
            assert!(file.sha256.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
        }
    }

    #[test]
    fn a_missing_or_short_file_is_no_embeddings_and_says_so() {
        let dir = std::env::temp_dir().join(format!("lore-model-status-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let status = status_at(&dir);
        assert!(!status.available);
        assert!(status.reason.contains("missing"), "{}", status.reason);
        // A truncated file is refused before the model is loaded.
        std::fs::write(dir.join(FILES[0].name), b"short").unwrap();
        let status = status_at(&dir);
        assert!(!status.available);
        assert!(status.reason.contains("bytes"), "{}", status.reason);
        // Opening it answers no embeddings, the fallback search relies on.
        let error = Embedder::open(ModelSource::Local(&dir)).err().expect("no model is installed");
        assert!(matches!(error, Error::NoEmbeddings(_)), "{error}");
        assert!(error.to_string().starts_with("no embeddings:"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_default_dir_follows_the_environment() {
        // The pure parts: LORE_MODEL_DIR names the model folder itself, and the
        // cache root is the platform's codelore folder unless LORE_CACHE says
        // otherwise. Both read the process environment, so this test does not
        // set it; it only checks the paths are absolute and end where they do.
        let dir = default_model_dir();
        assert!(dir.is_absolute(), "{}", dir.display());
        assert!(dir.ends_with(MODEL), "{}", dir.display());
        let root = cache_root();
        assert!(root.is_absolute(), "{}", root.display());
    }

    #[test]
    fn the_thread_and_batch_bounds_are_bounded() {
        // At least one worker, at most four by default: a loaded desktop keeps
        // a core for the person.
        let threads = default_threads();
        assert!((1..=4).contains(&threads), "{threads}");
        assert_eq!(bounded_batch(0), 1);
        assert_eq!(bounded_batch(32), 32);
        assert_eq!(bounded_batch(10_000), 256);
        assert_eq!(bounded_threads(0), 1);
        assert_eq!(bounded_threads(4), 4);
        assert_eq!(bounded_threads(10_000), 64);
    }
}
