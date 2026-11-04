use super::*;
use crate::image::linux::SEALS;
use std::{
    io::Write,
    os::fd::{AsRawFd, FromRawFd},
    time::Duration,
};

fn bytes() -> Vec<u8> {
    std::fs::read("/bin/true").unwrap()
}
fn memory(bytes: &[u8], seals: i32) -> File {
    let fd = unsafe {
        libc::memfd_create(
            c"zerus-identical-label".as_ptr(),
            libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING | 0x10,
        )
    };
    assert!(fd >= 0);
    // SAFETY: the successful syscall returned a new owned descriptor.
    let mut file = unsafe { File::from_raw_fd(fd) };
    file.write_all(bytes).unwrap();
    if seals != 0 {
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_ADD_SEALS, seals) }, 0);
    }
    file
}
fn reference() -> Arc<ServiceImage> {
    Arc::new(ServiceImage::from_original(memory(&bytes(), SEALS)).unwrap())
}
fn handles(file: &File) -> usize {
    use std::os::unix::fs::MetadataExt;
    let original = file.metadata().unwrap();
    let mut count = 0;
    for (n, entry) in std::fs::read_dir("/proc/self/fd").unwrap().enumerate() {
        assert!(n < 8192, "private descriptor inventory must be bounded");
        if let Ok(actual) = entry.unwrap().path().metadata() {
            if (actual.dev(), actual.ino()) == (original.dev(), original.ino()) {
                count += 1;
            }
        }
    }
    count
}
fn until() -> Instant {
    Instant::now() + Duration::from_secs(2)
}

#[test]
fn equal_sealed_native_image_is_retained_and_each_new_kernel_object_must_match() {
    let reference = reference();
    let original = memory(&bytes(), SEALS);
    let same = original.try_clone().unwrap();
    let image = reference.inspect_peer(original, until()).unwrap();
    assert!(image.current(&same));
    let replacement = memory(&bytes(), SEALS);
    // SAME code and memfd name are insufficient after authentication.
    assert!(!image.current(&replacement));
    assert_eq!(Arc::strong_count(&reference), 2);
    drop(image);
    assert_eq!(Arc::strong_count(&reference), 1);
}

#[test]
fn every_missing_seal_and_future_write_only_refuses_without_retaining_the_reference() {
    let reference = reference();
    for missing in [
        libc::F_SEAL_WRITE,
        libc::F_SEAL_GROW,
        libc::F_SEAL_SHRINK,
        libc::F_SEAL_SEAL,
    ] {
        let peer = memory(&bytes(), (SEALS & !missing) | libc::F_SEAL_FUTURE_WRITE);
        let retained = peer.try_clone().unwrap();
        assert!(
            reference.inspect_peer(peer, until()).is_err(),
            "missing {missing}"
        );
        assert_eq!(handles(&retained), 1, "refused original fd must close");
        assert_eq!(Arc::strong_count(&reference), 1);
    }
    assert!(
        reference
            .inspect_peer(memory(&bytes(), libc::F_SEAL_FUTURE_WRITE), until())
            .is_err()
    );
}

#[test]
fn differing_truncated_oversize_non_native_and_expired_peers_refuse() {
    let reference = reference();
    let mut different = bytes();
    let last = different.len() - 1;
    different[last] ^= 1;
    let truncated = &bytes()[..32];
    for data in [&different[..], truncated, b"#!/bin/sh\nexit 0\n"] {
        assert!(
            reference
                .inspect_peer(memory(data, SEALS), until())
                .is_err()
        );
    }
    let oversized = memory(b"\x7fELF", 0);
    oversized.set_len(MAX_IMAGE_BYTES + 1).unwrap();
    assert_eq!(
        unsafe { libc::fcntl(oversized.as_raw_fd(), libc::F_ADD_SEALS, SEALS) },
        0
    );
    assert!(reference.inspect_peer(oversized, until()).is_err());
    assert!(
        reference
            .inspect_peer(memory(&bytes(), SEALS), Instant::now())
            .is_err()
    );
    assert_eq!(Arc::strong_count(&reference), 1);
}

#[test]
fn an_unsealed_or_changed_reference_can_never_validate_an_equal_peer() {
    // Corrupt internal reference control: production publishes only Frozen::new.
    let file = memory(&bytes(), 0);
    let original = stamp(&file).unwrap();
    let writer = file.try_clone().unwrap();
    let reference = Arc::new(ServiceImage {
        _source: file.try_clone().unwrap(),
        _source_version: stamp(&file).unwrap(),
        frozen: Frozen { file, original },
    });
    assert!(
        reference
            .inspect_peer(memory(&bytes(), SEALS), until())
            .is_err()
    );
    writer.write_all_at(b"changed", 0).unwrap();
    assert!(
        reference
            .inspect_peer(memory(&bytes(), SEALS), until())
            .is_err()
    );
}

#[test]
fn startup_reference_is_not_refreshed_from_a_replaced_or_rewritten_installation() {
    let root = std::env::temp_dir().join(format!("zerus-relay-reference-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("hgs");
    std::fs::copy("/bin/true", &path).unwrap();
    let reference = Arc::new(ServiceImage::from_original(File::open(&path).unwrap()).unwrap());
    let old = memory(&bytes(), SEALS);
    std::fs::rename(&path, root.join("original")).unwrap();
    std::fs::copy("/bin/false", &path).unwrap();
    assert!(reference.inspect_peer(old, until()).is_ok());
    assert!(
        reference
            .inspect_peer(memory(&std::fs::read(path).unwrap(), SEALS), until())
            .is_err()
    );
    std::fs::copy("/bin/false", root.join("original")).unwrap();
    assert!(
        reference
            .inspect_peer(memory(&bytes(), SEALS), until())
            .is_ok()
    );
    std::fs::remove_dir_all(root).unwrap();
}
