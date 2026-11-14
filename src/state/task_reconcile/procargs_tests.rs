use super::*;

fn answer(tail: &[u8]) -> Vec<u8> {
    let mut bytes = 2_i32.to_ne_bytes().to_vec();
    bytes.extend_from_slice(b"/fixture/bin/executor\0\0\0executor\0--wait\0");
    bytes.extend_from_slice(tail);
    bytes
}

#[test]
fn a_withheld_executor_environment_is_unavailable() {
    for tail in [&[][..], &[0][..], &[0; 8][..], &[0; 64][..]] {
        assert!(
            procargs_environment(&answer(tail)).is_err(),
            "withheld environment of {} NUL bytes must be unavailable",
            tail.len()
        );
    }
}

#[test]
fn readable_empty_environment_and_native_run_stay_distinct() {
    let kernel = b"\0ptr_munge=\0executable_path=/fixture/bin/executor\0\0";
    assert_eq!(procargs_environment(&answer(kernel)).unwrap(), kernel);
    let marked = b"HGS_RUN_ID=run-fixture\0PATH=/usr/bin\0\0";
    assert_eq!(procargs_environment(&answer(marked)).unwrap(), marked);
}

#[test]
fn incomplete_procargs_do_not_supply_executor_evidence() {
    assert!(procargs_environment(&[]).is_err());
    assert!(procargs_environment(&[2, 0]).is_err());
    let mut bytes = answer(b"HGS_RUN_ID=run-fixture\0");
    bytes[..4].copy_from_slice(&3_i32.to_ne_bytes());
    bytes.truncate(bytes.len() - 1);
    assert!(procargs_environment(&bytes).is_err());
}
