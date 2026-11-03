//! The same Processes payload is consumed by the CLI, desktop and Android.
use serde_json::Value;
use zerus_client_core::process_view::{self, Group};

#[test]
fn command_previews_keep_process_identity_controls_and_notes() {
    let contract: Value = serde_json::from_str(include_str!(
        "../../../mobile/android/app/src/test/resources/process-command-contract.json"
    ))
    .unwrap();
    for case in contract["cases"].as_array().unwrap() {
        let processes = &case["processes"];
        let job = &processes["items"][0];
        assert_eq!(job["id"], "job-a");
        assert_eq!(job["run_id"], "run-a");
        assert_eq!(job["conversation_id"], "conversation-a");
        assert!(process_view::active(job));
        assert!(!process_view::stoppable(job));
        assert_eq!(process_view::group(job, true), Group::Live);
        assert!(process_view::details(job, "").contains("Running"));
        let (notes, _) = process_view::notes(processes);
        assert_eq!(!notes.is_empty(), job["command_truncated"] == true);
        if job["command_truncated"] == true {
            assert!(notes.contains("displayed preview"));
            assert!(job["command"].as_str().unwrap().ends_with('…'));
        } else {
            assert_eq!(job["command"], case["command_input"]);
        }
        let mut offline = job.clone();
        process_view::normalize(&mut offline, false);
        assert_eq!(offline["status"], "unknown");
        assert_eq!(offline["last_status"], "running");
        assert_eq!(offline["command"], job["command"]);
    }
}
