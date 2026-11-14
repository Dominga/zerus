use super::*;

fn answer(tail: &[u8]) -> Vec<u8> {
    let mut bytes = 2_i32.to_ne_bytes().to_vec();
    bytes.extend_from_slice(b"/fixture/bin/frontend\0\0\0frontend\0--wait\0");
    bytes.extend_from_slice(tail);
    bytes
}

#[test]
fn a_withheld_environment_is_unavailable_without_agent_evidence() {
    for tail in [&[][..], &[0][..], &[0; 8][..], &[0; 64][..]] {
        let bytes = answer(tail);
        assert_eq!(
            procargs_environment(&bytes).unwrap(),
            None,
            "withheld environment of {} NUL bytes is not readable evidence",
            tail.len()
        );
        // The production exclusion check must leave GUI registration and
        // foreground Terminal callers to their independent native proofs.
        assert!(!procargs_agent_environment(&bytes).unwrap());
    }
}

#[test]
fn readable_empty_environment_and_agent_markers_stay_distinct() {
    // Even an empty environment leaves the kernel's own strings in the tail.
    let kernel = b"\0ptr_munge=\0executable_path=/fixture/bin/frontend\0\0";
    assert_eq!(
        procargs_environment(&answer(kernel)).unwrap(),
        Some(kernel.as_slice())
    );
    assert!(!procargs_agent_environment(&answer(kernel)).unwrap());
    assert!(!procargs_agent_environment(&answer(b"PATH=/usr/bin\0\0")).unwrap());
    assert!(procargs_agent_environment(&answer(b"HGS_RUN_ID=run-fixture\0\0")).unwrap());
    assert!(procargs_agent_environment(&answer(b"HGS_AGENT=dsh\0\0")).unwrap());
}

#[test]
fn incomplete_procargs_do_not_supply_frontend_evidence() {
    assert!(procargs_agent_environment(&[]).is_err());
    assert!(procargs_agent_environment(&[2, 0]).is_err());
    let mut bytes = answer(b"PATH=/usr/bin\0");
    bytes[..4].copy_from_slice(&3_i32.to_ne_bytes());
    bytes.truncate(bytes.len() - 1);
    assert!(procargs_agent_environment(&bytes).is_err());
}
