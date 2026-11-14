use super::*;

#[test]
fn unavailable_publication_never_becomes_a_definite_refusal_or_delivery() {
    for observed in [
        Err("Durable native refusal publication is unverified on this computer".into()),
        Err("failure after publication".into()),
        Ok(Observed::Inactive),
        Ok(Observed::Absent),
        Ok(Observed::Existing),
    ] {
        let calls = std::cell::Cell::new(0);
        let result = mode_refusal_result(observed, |state, error| {
            calls.set(calls.get() + 1);
            Ok(json!({"state":state,"error":error}))
        })
        .unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(result, json!({"state":"uncertain","error":UNCERTAIN}));
        assert!(result.get("refusal").is_none());
    }
}

#[test]
fn an_existing_proven_tombstone_keeps_the_original_reply_sentence() {
    let reply = mode_refusal_result(
        Ok(Observed::Refused {
            same_plan: false,
            proof: None,
        }),
        |state, error| Ok(json!({"state":state,"error":error})),
    )
    .unwrap();
    assert_eq!(reply, json!({"state":"refused","error":ANOTHER}));
}
