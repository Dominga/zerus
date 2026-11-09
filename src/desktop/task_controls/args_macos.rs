//! Bounded KERN_PROCARGS2 decoding. Retain argv only; environment bytes never
//! authenticate the caller and must not enter the peer object or diagnostics.
pub(super) const BYTES: usize = 256 << 10;
pub(super) fn decode(bytes: &[u8]) -> Option<Vec<Vec<u8>>> {
    if bytes.len() < 5 || bytes.len() > BYTES {
        return None;
    }
    let argc = i32::from_ne_bytes(bytes[..4].try_into().ok()?);
    if !(1..=64).contains(&argc) {
        return None;
    }
    let mut at = 4 + bytes[4..].iter().position(|b| *b == 0)?;
    while bytes.get(at) == Some(&0) {
        at += 1;
    }
    let mut args = Vec::new();
    let mut total = 0;
    for _ in 0..argc {
        let end = at + bytes.get(at..)?.iter().position(|b| *b == 0)?;
        total += end - at;
        if total > 16 << 10 {
            return None;
        }
        args.push(bytes[at..end].to_vec());
        at = end + 1;
    }
    (!args.first().is_none_or(Vec::is_empty)).then_some(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(args: &[&[u8]]) -> Vec<u8> {
        let mut b = (args.len() as i32).to_ne_bytes().to_vec();
        b.extend_from_slice(b"/srv/example/hgs\0\0\0");
        for arg in args {
            b.extend_from_slice(arg);
            b.push(0);
        }
        b.extend_from_slice(b"HGS_SESSION=an/environment/label\0IGNORED=private fixture\0");
        b
    }
    #[test]
    fn actual_argv_count_excludes_the_environment_and_preserves_empty_arguments() {
        let args: &[&[u8]] = &[b"hgs", b"task", b"control", b"status", b"--json", b""];
        let bytes = fixture(args);
        assert_eq!(
            decode(&bytes).unwrap(),
            args.iter().map(|a| a.to_vec()).collect::<Vec<_>>()
        );
    }
    #[test]
    fn a_declared_argument_list_cannot_run_past_the_native_buffer() {
        let mut b = fixture(&[b"hgs", b"task"]);
        b[..4].copy_from_slice(&64i32.to_ne_bytes());
        assert!(decode(&b).is_none());
        b = fixture(&[b"hgs"]);
        b.truncate(4 + "/srv/example/hgs".len() + 3);
        assert!(decode(&b).is_none());
    }
    #[test]
    fn truncated_unknown_and_oversize_layouts_refuse() {
        for b in [vec![], vec![0; 4], vec![0; 5], vec![1; BYTES + 1]] {
            assert!(decode(&b).is_none());
        }
        let mut b = fixture(&[b"hgs"]);
        b[..4].copy_from_slice(&65i32.to_ne_bytes());
        assert!(decode(&b).is_none());
        let long = vec![b'x'; (16 << 10) + 1];
        assert!(decode(&fixture(&[b"hgs", &long])).is_none());
        let mut unterminated = 1i32.to_ne_bytes().to_vec();
        unterminated.extend_from_slice(b"not terminated");
        assert!(decode(&unterminated).is_none());
    }
}
