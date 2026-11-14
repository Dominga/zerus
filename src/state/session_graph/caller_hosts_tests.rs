use super::*;

#[test]
fn the_selected_home_uses_the_shared_record_reader() {
    let home = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "state::session_graph::sends::caller_hosts::tests::selected_home_fixture",
            "--ignored",
            "--nocapture",
        ])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", home.path())
        .env("HGS_STATE_DIR", home.path().join("state"))
        .env("HGS_CONFIG_DIR", home.path().join("config"))
        .env("DSH_HOME", home.path().join("dsh-home"))
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "private host-record fixture; invoked by its parent test"]
fn selected_home_fixture() {
    let agent_home = std::env::var("DSH_HOME").unwrap();
    let root = crate::state::storage::root();
    let hosts = root.join("dsh/hosts");
    let folder = hosts.join(crate::state::dsh::host_key(&agent_home));
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("host.identity.json"),
        r#"{"state":"running","pid":4242,"birth":"fixture-birth"}"#,
    )
    .unwrap();
    std::fs::write(folder.join("host.pid"), "4242").unwrap();
    // A malformed unrelated home must not enter a selected-home token proof.
    let unrelated = hosts.join("unrelated");
    std::fs::create_dir_all(&unrelated).unwrap();
    std::fs::write(unrelated.join("host.identity.json"), "{").unwrap();
    let before = crate::state::dsh::recorded_host_reads();
    assert_eq!(
        deepseek_home(&agent_home),
        Ok(vec![Named {
            pid: 4242,
            birth: Some("fixture-birth".into())
        }])
    );
    assert_eq!(
        crate::state::dsh::recorded_host_reads() - before,
        2,
        "the per-home proof must use the shared bounded file reader exactly once"
    );
    std::fs::write(folder.join("host.pid"), "invalid").unwrap();
    assert_eq!(
        deepseek_home(&agent_home),
        Err("Cannot verify a recorded agent host.")
    );
}
