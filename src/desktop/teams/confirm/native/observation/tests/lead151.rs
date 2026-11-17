use super::*;
#[test]
fn a_first_message_lead_uses_the_native_first_send_observation() {
    isolated("desktop::teams::confirm::native::observation::tests::lead151::first_lead_fixture");
}
#[test]
#[ignore = "run by its isolated wrapper"]
fn first_lead_fixture() {
    let mut h = crate::desktop::teams::tests::bound151_fixture();
    let config = Config {
        home: String::new(),
        dir: h.temp.path().join("config"),
        state: h.temp.path().into(),
        selves: vec![],
        peers: vec![],
        base_peers: vec![],
        peers_from_environment: false,
        machines: Default::default(),
        tab: false,
        tab_colors: String::new(),
    };
    let (b, name) = super::super::super::binding::launch_binding(
        &config,
        crate::teams::ops::tests::TEAM,
        "lead",
    )
    .unwrap();
    assert!(b.conversation.is_empty());
    let mut raw = crate::state::listing_fixture::launched_record(
        &b.agent, &name, &b.launch, &b.session, &b.run, &b.folder, &b.account,
    );
    // Ownership is input from the team's already-written launch binding.
    raw["worktree_owner"] = json!({"id":b.nonce});
    crate::state::listing_fixture::save(&mut raw).unwrap();
    let mut view = crate::state::inspection_for_test(&mut raw).unwrap();
    let live = crate::state::listing_fixture::listed_live_row(raw.clone()).unwrap();
    // Supply the terminal listing port through its producer, while keeping
    // the inspection producer's question and native-identity fields.
    for key in ["runtime_state", "process_state"] {
        view[key] = live[key].clone();
    }
    assert_eq!(view["runtime_state"], "live", "{view}");
    assert_eq!(view["process_state"], "running", "{view}");
    let stop = Arc::new(AtomicBool::new(false));
    let native = Native::new(&config, &b.machine, &stop, &mut h.store).unwrap();
    let (snapshot, _) = native.read_view(&b, &view, raw, None).unwrap();
    assert!(snapshot.ready, "{snapshot:?}");
    assert!(snapshot.conversation.is_empty());
    assert!(
        crate::desktop::teams::child::FIXTURE_ARGS.with(|s| s.borrow().is_empty()),
        "Claude has no --observe CLI; its native first-send observation is shared directly"
    );
}
