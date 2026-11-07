//! Getting the pinned model files: download them from Hugging Face into the
//! user cache, or verify a directory an offline install already holds.
//!
//! Every file is pinned by exact size and SHA-256 in [`super::FILES`]; a
//! download goes to a temporary file beside its destination, is hashed while
//! it streams, and only a match is renamed into place. A network failure, a
//! short read, a wrong size or a wrong digest leaves whatever was there.
use super::{Error, ModelSource, PinnedFile, FILES, REPOSITORY, REVISION};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const CHUNK: usize = 64 * 1024;
/// A download may not run past the pinned size by more than this before it is
/// refused; the final size must match exactly.
const SLACK: u64 = 1024;

/// Fetches every pinned file into `dir` (diagnosed in [`super::cache_root`]),
/// or verifies `dir` for an offline install. Returns the model directory.
pub fn fetch(source: ModelSource<'_>, progress: &mut dyn FnMut(&str, u64, u64)) -> Result<PathBuf, Error> {
    let dir = match source {
        ModelSource::Cache => super::default_model_dir(),
        ModelSource::Local(path) => path.to_path_buf(),
    };
    match source {
        ModelSource::Local(_) => {
            verify_all(&dir)?;
        }
        ModelSource::Cache => {
            std::fs::create_dir_all(&dir)
                .map_err(|error| Error::Fetch(format!("cannot create {}: {error}", dir.display())))?;
            for file in FILES {
                ensure(&dir, &file, progress)?;
            }
        }
    }
    Ok(dir)
}

/// Every pinned file's exact size and SHA-256.
pub fn verify_all(dir: &Path) -> Result<(), Error> {
    for file in FILES {
        verify_file(dir, &file)?;
    }
    Ok(())
}

/// A file's SHA-256 read in chunks; the file's size is checked first.
fn hash(path: &Path) -> Result<String, Error> {
    let mut reader = std::fs::File::open(path)
        .map_err(|error| Error::Model(format!("cannot read {}: {error}", path.display())))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; CHUNK];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| Error::Model(format!("cannot read {}: {error}", path.display())))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn verify_file(dir: &Path, file: &PinnedFile) -> Result<(), Error> {
    let path = dir.join(file.name);
    let metadata = std::fs::metadata(&path)
        .map_err(|error| Error::Model(format!("{}: {error}", path.display())))?;
    if !metadata.is_file() || metadata.len() != file.bytes {
        return Err(Error::Model(format!(
            "{} is {} bytes, the pinned file is {} bytes",
            path.display(),
            metadata.len(),
            file.bytes
        )));
    }
    let digest = hash(&path)?;
    if digest != file.sha256 {
        return Err(Error::Model(format!(
            "{} has SHA-256 {digest}, the pinned file is {}",
            path.display(),
            file.sha256
        )));
    }
    Ok(())
}

/// One file: kept when it already matches, downloaded and verified otherwise.
fn ensure(dir: &Path, file: &PinnedFile, progress: &mut dyn FnMut(&str, u64, u64)) -> Result<(), Error> {
    let path = dir.join(file.name);
    if verify_file(dir, file).is_ok() {
        progress(file.name, file.bytes, file.bytes);
        return Ok(());
    }
    download(file, &path, progress)
}

/// Streams `file` from Hugging Face at [`REVISION`] and renames it into place
/// only when its size and digest match the pin.
fn download(file: &PinnedFile, path: &Path, progress: &mut dyn FnMut(&str, u64, u64)) -> Result<(), Error> {
    let url = format!("https://huggingface.co/{REPOSITORY}/resolve/{REVISION}/{}", file.name);
    let mut response = ureq::get(&url)
        .call()
        .map_err(|error| Error::Fetch(format!("{url}: {error}")))?;
    if !response.status().is_success() {
        return Err(Error::Fetch(format!("{url}: HTTP {}", response.status())));
    }
    let mut temporary = path.to_path_buf();
    temporary.set_file_name(format!(".{}.{}.part", file.name, std::process::id()));
    let mut output = std::fs::File::create(&temporary)
        .map_err(|error| Error::Fetch(format!("cannot write {}: {error}", temporary.display())))?;
    let mut hasher = Sha256::new();
    let mut written: u64 = 0;
    let mut reader = response.body_mut().as_reader();
    let mut buffer = vec![0u8; CHUNK];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| Error::Fetch(format!("{url}: {error}")))?;
        if read == 0 {
            break;
        }
        written += read as u64;
        if written > file.bytes + SLACK {
            let _ = std::fs::remove_file(&temporary);
            return Err(Error::Fetch(format!("{url}: the file is larger than its pinned {} bytes", file.bytes)));
        }
        hasher.update(&buffer[..read]);
        output
            .write_all(&buffer[..read])
            .map_err(|error| Error::Fetch(format!("cannot write {}: {error}", temporary.display())))?;
        progress(file.name, written, file.bytes);
    }
    output
        .flush()
        .map_err(|error| Error::Fetch(format!("cannot write {}: {error}", temporary.display())))?;
    drop(output);
    if written != file.bytes {
        let _ = std::fs::remove_file(&temporary);
        return Err(Error::Fetch(format!("{url}: got {written} bytes, the pinned file is {} bytes", file.bytes)));
    }
    let digest = format!("{:x}", hasher.finalize());
    if digest != file.sha256 {
        let _ = std::fs::remove_file(&temporary);
        return Err(Error::Fetch(format!(
            "{url}: SHA-256 {digest} does not match the pinned {}; the file was not installed",
            file.sha256
        )));
    }
    replace(&temporary, path)
        .map_err(|error| Error::Fetch(format!("cannot install {}: {error}", path.display())))
}

/// Renames a verified download over its destination. Windows refuses a rename
/// onto an existing file in some configurations, so the old file is removed
/// when the plain rename does not take.
fn replace(temporary: &Path, path: &Path) -> std::io::Result<()> {
    match std::fs::rename(temporary, path) {
        Ok(()) => Ok(()),
        Err(first) => {
            let _ = std::fs::remove_file(path);
            std::fs::rename(temporary, path).map_err(|_| first)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lore-fetch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_file_is_verified_by_its_exact_size_and_sha256() {
        let dir = scratch("verify");
        let file = PinnedFile { name: "small.bin", bytes: 5, sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824" };
        std::fs::write(dir.join(file.name), b"hello").unwrap();
        assert!(verify_file(&dir, &file).is_ok());
        // One changed byte is refused.
        std::fs::write(dir.join(file.name), b"hellO").unwrap();
        assert!(verify_file(&dir, &file).is_err());
        // A short file is refused by size before hashing.
        std::fs::write(dir.join(file.name), b"hel").unwrap();
        assert!(verify_file(&dir, &file).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_offline_directory_is_verified_before_it_is_used() {
        let dir = scratch("offline");
        let error = fetch(ModelSource::Local(&dir), &mut |_, _, _| {}).unwrap_err();
        assert!(matches!(error, Error::Model(_)), "{error}");
        // A directory with the weights but no tokenizer is still refused.
        std::fs::write(dir.join(FILES[0].name), vec![0u8; 16]).unwrap();
        let error = fetch(ModelSource::Local(&dir), &mut |_, _, _| {}).unwrap_err();
        assert!(matches!(error, Error::Model(_)), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The real fetch: downloads the three pinned files from Hugging Face,
    /// verifies them, then opens the model and embeds a query. Ignored because
    /// it needs the network and ~550 MB; run it with a fresh
    /// `LORE_MODEL_DIR`:
    /// `LORE_MODEL_DIR=<dir> cargo test --locked --lib -- --ignored`.
    #[test]
    #[ignore]
    fn the_model_downloads_and_verifies() {
        let dir = super::super::default_model_dir();
        let mut files = 0;
        let mut fetch_progress = |name: &str, done: u64, total: u64| {
            eprintln!("{name}: {done}/{total}");
            if done == total {
                files += 1;
            }
        };
        let fetched = fetch(ModelSource::Cache, &mut fetch_progress).expect("the pinned model downloads");
        assert_eq!(fetched, dir);
        assert_eq!(files, FILES.len());
        verify_all(&dir).expect("the downloaded files match the pins");
        let embedder = crate::embed::Embedder::open(crate::embed::ModelSource::Cache).expect("the model opens");
        let vector = embedder.embed_query("atlas checksum mismatch").expect("a query embeds");
        assert_eq!(vector.len(), crate::embed::DIMENSIONS);
        let norm: f32 = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4, "{norm}");
    }
}
