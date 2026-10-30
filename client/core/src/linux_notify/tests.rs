use super::*;
use serde_json::{json, Value};

const BUS: &str = "00112233445566778899aabbccddeeff";
const OTHER_BUS: &str = "ffeeddccbbaa99887766554433221100";

fn saved() -> Saved {
    decode_saved(
        br#"{"version":1,"bus_id":"00112233445566778899aabbccddeeff","owner":":1.0","tokens":{"7":"exact-edge"},"uncertain":["unknown-edge"]}"#,
    )
    .unwrap()
}

fn encoded(value: Value) -> Vec<u8> {
    serde_json::to_vec(&value).unwrap()
}

#[test]
fn current_wire_round_trips_and_legacy_absence_is_not_current_lineage() {
    let saved = saved();
    let wire = serde_json::to_value(&saved).unwrap();
    assert_eq!(wire["version"], 1);
    assert_eq!(wire["bus_id"], BUS);
    assert_eq!(wire["owner"], ":1.0");
    assert_eq!(wire["tokens"]["7"], "exact-edge");
    assert_eq!(wire["uncertain"], json!(["unknown-edge"]));

    // The projection accepts opaque extensions; the caller retains raw bytes.
    let original =
        br#"{"version":1,"owner":":1.0","tokens":{"7":"edge"},"future":{"private":true}}"#;
    let legacy = decode_saved(original).unwrap();
    assert!(legacy.bus_id.is_empty());
    assert!(legacy.uncertain.is_empty());
    assert_eq!(legacy.tokens.get(&7).unwrap(), "edge");
    assert!(
        serde_json::from_slice::<Value>(original).unwrap()["future"]["private"]
            .as_bool()
            .unwrap()
    );
}

#[test]
fn invalid_or_future_saved_state_refuses_instead_of_resetting() {
    let original = serde_json::to_value(saved()).unwrap();
    for field in ["version", "owner", "tokens"] {
        let mut value = original.clone();
        value.as_object_mut().unwrap().remove(field);
        assert!(decode_saved(&encoded(value)).is_err(), "missing {field}");
    }
    for version in [0, 2] {
        let mut value = original.clone();
        value["version"] = json!(version);
        assert!(decode_saved(&encoded(value)).is_err());
    }
    for tokens in [
        json!({"0":"edge"}),
        json!({"7":""}),
        json!({"no-id":"edge"}),
    ] {
        let mut value = original.clone();
        value["tokens"] = tokens;
        assert!(decode_saved(&encoded(value)).is_err());
    }
    for bytes in [b"{".as_slice(), b"null", b"[]", &[0xff]] {
        assert!(decode_saved(bytes).is_err());
    }
}

#[test]
fn bus_id_compatibility_is_empty_or_exact_hex_guid() {
    let mut value = serde_json::to_value(saved()).unwrap();
    for id in ["", BUS, "00112233445566778899AABBCCDDEEFF"] {
        value["bus_id"] = json!(id);
        assert!(decode_saved(&encoded(value.clone())).is_ok());
    }
    for id in [
        "0",
        "00112233445566778899aabbccddeef",
        "00112233445566778899aabbccddeeff0",
        "00112233445566778899aabbccddeefg",
    ] {
        value["bus_id"] = json!(id);
        assert!(decode_saved(&encoded(value.clone())).is_err());
    }
}

#[test]
fn entry_and_utf8_token_budgets_are_shared_and_exact() {
    let mut saved = saved();
    saved.tokens = (1..LIMIT as u32)
        .map(|id| (id, format!("edge-{id}")))
        .collect();
    assert!(saved.valid()); // 8191 IDs plus one unknown token.
    saved.uncertain.insert("one-too-many".into());
    assert!(!saved.valid());
    saved.tokens.clear();
    saved.uncertain = BTreeSet::from(["я".repeat(8192)]);
    assert!(saved.valid());
    saved.uncertain = BTreeSet::from(["я".repeat(8193)]);
    assert!(!saved.valid());
    saved.uncertain = BTreeSet::from([String::new()]);
    assert!(!saved.valid());
    saved.uncertain.clear();
    saved.tokens.insert(1, "я".repeat(8192));
    assert!(saved.valid());
    saved.tokens.insert(1, "я".repeat(8193));
    assert!(!saved.valid());
}

#[test]
fn raw_byte_budget_applies_before_projection_even_to_unknown_fields() {
    let mut bytes = br#"{"version":1,"owner":"","tokens":{},"opaque":""#.to_vec();
    let remaining = FILE_LIMIT as usize - bytes.len() - 2;
    bytes.resize(bytes.len() + remaining, b'x');
    bytes.extend_from_slice(br#""}"#);
    assert_eq!(bytes.len() as u64, FILE_LIMIT);
    assert!(decode_saved(&bytes).is_ok());
    bytes.push(b' ');
    assert_eq!(
        decode_saved(&bytes).err().unwrap(),
        "Notification identities exceed the storage limit"
    );
}

#[test]
fn same_namespace_retains_ids_and_is_idempotent() {
    let mut saved = saved();
    let current = Namespace::saved(&saved);
    assert!(!saved.adopt_namespace(&current));
    assert_eq!(saved.tokens.get(&7).unwrap(), "exact-edge");
    assert_eq!(saved.uncertain, BTreeSet::from(["unknown-edge".into()]));
}

#[test]
fn legacy_bus_change_owner_change_and_missing_owner_keep_unknown_suppression() {
    let current = Namespace {
        bus_id: BUS.into(),
        owner: ":1.0".into(),
    };
    for prior in [
        Namespace {
            bus_id: String::new(),
            owner: ":1.0".into(),
        },
        Namespace {
            bus_id: OTHER_BUS.into(),
            owner: ":1.0".into(),
        },
        Namespace {
            bus_id: BUS.into(),
            owner: ":1.1".into(),
        },
    ] {
        let mut saved = saved();
        saved.bus_id = prior.bus_id;
        saved.owner = prior.owner;
        assert!(saved.adopt_namespace(&current));
        assert!(saved.tokens.is_empty());
        assert_eq!(
            saved.uncertain,
            BTreeSet::from(["unknown-edge".into(), "exact-edge".into()])
        );
        assert_eq!(Namespace::saved(&saved), current);
        assert!(!saved.adopt_namespace(&current));
    }
    let mut saved = saved();
    saved.owner.clear();
    let absent = Namespace::saved(&saved);
    assert!(saved.adopt_namespace(&absent));
    assert!(saved.tokens.is_empty());
    assert_eq!(saved.uncertain.len(), 2);
    assert!(!saved.adopt_namespace(&absent));
}

#[test]
fn namespace_replacement_never_turns_unknown_into_retry() {
    let mut saved = saved();
    for current in [
        Namespace {
            bus_id: BUS.into(),
            owner: String::new(),
        },
        Namespace {
            bus_id: BUS.into(),
            owner: ":1.1".into(),
        },
        Namespace {
            bus_id: OTHER_BUS.into(),
            owner: ":1.0".into(),
        },
    ] {
        saved.adopt_namespace(&current);
        assert!(saved.tokens.is_empty());
        assert!(saved.uncertain.contains("unknown-edge"));
        assert!(saved.uncertain.contains("exact-edge"));
    }
}

#[test]
fn body_markup_is_literal_and_keeps_russian_newlines_and_apostrophes() {
    assert_eq!(
        escape_body("Агент <b>готов</b> & \"ответ\"\n'код' > 0 &amp;"),
        "Агент &lt;b&gt;готов&lt;/b&gt; &amp; &quot;ответ&quot;\n'код' &gt; 0 &amp;amp;"
    );
    assert_eq!(escape_body(""), "");
}
