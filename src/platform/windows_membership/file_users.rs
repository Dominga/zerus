//! Positive usage DATA only. Class 47 is reserved for system use; neither an
//! empty array nor successful decoding establishes complete checkout coverage.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PositiveUsers {
    pub(super) request: uuid::Uuid,
    pub(super) object: super::handles::Object,
    pub(super) pids: BTreeSet<u32>,
}

pub(super) fn decode(bytes: &[u8], until: Instant) -> Result<BTreeSet<u32>> {
    live(until)?;
    if bytes.len() < 8 || bytes.len() > MAX_TABLE {
        return Err(Error::Incomplete);
    }
    let count = usize::try_from(word(bytes, 0, 4)?).map_err(|_| Error::Incomplete)?;
    let length = count
        .checked_mul(8)
        .and_then(|n| n.checked_add(8))
        .ok_or(Error::Incomplete)?;
    if count > MAX_PROCESSES || length != bytes.len() {
        return Err(Error::Incomplete);
    }
    // Bytes 4..8 are native padding, NOT a documented reserved-zero field.
    let mut pids = BTreeSet::new();
    for row in bytes[8..].as_chunks::<8>().0 {
        live(until)?;
        let pid = u32::try_from(word(row, 0, 8)?).map_err(|_| Error::Incomplete)?;
        if pid == 0 || !pids.insert(pid) {
            return Err(Error::Incomplete);
        }
    }
    Ok(pids)
}

/// Private test framing only. Libtest can leave its progress prefix on the
/// current stdout line before invoking an exact ignored probe with nocapture.
#[cfg(test)]
pub(super) fn private_report_frame(json: &str) -> String {
    format!("\nfile-users-report-v1={json}\n")
}
#[cfg(test)]
pub(super) fn private_report_line(stdout: &str) -> Result<&str> {
    let mut rows = stdout
        .lines()
        .filter_map(|line| line.strip_prefix("file-users-report-v1="));
    let row = rows.next().ok_or(Error::Incomplete)?;
    if rows.next().is_some() {
        return Err(Error::Incomplete);
    }
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    fn until() -> Instant {
        Instant::now() + Duration::from_secs(2)
    }
    fn bytes(pids: &[u64]) -> Vec<u8> {
        let mut bytes = vec![0; 8];
        bytes[..4].copy_from_slice(&(pids.len() as u32).to_le_bytes());
        for pid in pids {
            bytes.extend_from_slice(&pid.to_le_bytes());
        }
        bytes
    }
    #[test]
    fn only_whole_bounded_native_pid_records_are_decoded() {
        let valid = bytes(&[40, 80]);
        assert_eq!(decode(&valid, until()).unwrap(), [40, 80].into());
        for n in 0..valid.len() {
            assert!(decode(&valid[..n], until()).is_err());
        }
        assert!(decode(&[valid.as_slice(), &[0]].concat(), until()).is_err());
        for pids in [vec![0], vec![u32::MAX as u64 + 1], vec![40, 40]] {
            assert!(decode(&bytes(&pids), until()).is_err());
        }
        assert!(decode(&valid, Instant::now()).is_err());
        let pids: Vec<_> = (1..=MAX_PROCESSES as u64).collect();
        assert_eq!(decode(&bytes(&pids), until()).unwrap().len(), MAX_PROCESSES);
        let over: Vec<_> = (1..=MAX_PROCESSES as u64 + 1).collect();
        assert!(decode(&bytes(&over), until()).is_err());
    }
    #[test]
    fn native_padding_is_not_a_process_identity_or_a_zero_requirement() {
        let mut native = bytes(&[40]);
        native[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(decode(&native, until()).unwrap(), [40].into());
        // This is an empty DATA set, with no Complete or Clear representation.
        assert!(decode(&bytes(&[]), until()).unwrap().is_empty());
    }
    #[test]
    fn private_report_starts_a_new_line_and_keeps_the_exact_single_record_rule() {
        // Captured from an actual --exact --ignored --nocapture --test-threads=1
        // libtest subprocess: its progress prefix has NO terminating newline.
        let progress = "\nrunning 1 test\ntest report_probe ... ";
        let json = r#"{"request":"private-fixture","outcome":"data_only"}"#;
        let old = format!("{progress}file-users-report-v1={json}\nok\n");
        assert!(private_report_line(&old).is_err());
        let frame = private_report_frame(json);
        let stdout = format!("{progress}{frame}ok\n");
        assert_eq!(private_report_line(&stdout).unwrap(), json);
        assert!(private_report_line(&format!("{stdout}{frame}")).is_err());
        assert!(private_report_line("test probe ... ok\n").is_err());
        assert!(private_report_line(&format!("decoy file-users-report-v1={json}\n")).is_err());
    }
}
