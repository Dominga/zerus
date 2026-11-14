use super::*;
const NODE: &str = "11111111-1111-4111-8111-111111111111";
#[test]
fn offline_writer_refusal_names_diagnostic_label_and_exact_node_and_local_details() {
    for reason in ["unroutable", "unverified", "deadline"] {
        let closed = Closed {
            node: Some(NODE.into()),
            reason,
        };
        let (code, text) = diagnostic_refusal(&closed, "", "orchard", Some("example-laptop"));
        assert_eq!(code, "cutting_over");
        assert!(text.contains(NODE), "{text}");
        assert!(text.contains("example-laptop"), "{text}");
        assert!(text.contains("diagnostic"), "{text}");
        assert!(text.contains("Absent computers do not block starts on reachable computers. This computer must finish its automatic task cutover before starting."), "{text}");
        assert!(
            text.contains("hgs task cutover --project orchard --json"),
            "{text}"
        );
        assert!(text.contains("Nothing was sent."), "{text}");
        assert_eq!(closed.node.as_deref(), Some(NODE));
        assert_eq!(closed.reason, reason);
    }
}
#[test]
fn absent_labels_and_global_deadlines_keep_the_refusal_and_details_without_inventing_a_node() {
    let (code, text) = diagnostic_refusal(
        &Closed {
            node: None,
            reason: "deadline",
        },
        "",
        "orchard",
        None,
    );
    assert_eq!(code, "cutting_over");
    assert!(
        text.contains("hgs task cutover --project orchard --json"),
        "{text}"
    );
    assert!(!text.contains(NODE));
}
#[test]
fn other_refusals_keep_the_existing_exact_sentence() {
    for reason in [
        "changed",
        "not_sendable",
        "held",
        "busy",
        "member_downgraded",
    ] {
        let closed = Closed {
            node: Some(NODE.into()),
            reason,
        };
        assert_eq!(
            diagnostic_refusal(
                &closed,
                "Original business refusal",
                "orchard",
                Some("misleading-label")
            ),
            basic_refusal(&closed, "Original business refusal")
        );
    }
}

#[test]
fn the_production_formatter_reads_only_the_exact_catalog_caption_without_changing_state() {
    let temp = tempfile::tempdir().unwrap();
    let config = Config {
        home: temp.path().display().to_string(),
        dir: temp.path().join("config"),
        state: temp.path().join("state"),
        selves: vec!["example-local".into()],
        peers: vec![],
        base_peers: vec![],
        peers_from_environment: false,
        machines: BTreeMap::new(),
        tab: false,
        tab_colors: String::new(),
    };
    let path = config.dir.join("swarm/catalog.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let op = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    let bytes = serde_json::to_vec(&json!({"catalog":{"schema":1,
        "swarm_id":"bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", "operations":{
            (op):{"id":op,"actor":NODE,"clock":1,"key":{"kind":"machine","id":NODE,"field":"name"},
                "parents":[],"value":"example-laptop"}}}}))
    .unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let closed = Closed {
        node: Some(NODE.into()),
        reason: "unroutable",
    };
    let (code, text) = refusal(&config, "orchard", &closed, "");
    assert_eq!(code, "cutting_over");
    assert!(text.contains("example-laptop"), "{text}");
    assert!(text.contains(NODE));
    assert!(text.contains("diagnostic"));
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    assert!(!config.state.exists());
}
