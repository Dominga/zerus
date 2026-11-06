//! Ported CodeLore conformance cases: the same inputs and the same refusals.
use serde_json::{json, Value};
use zerus_lore::{canonical, parse, serialize, validate};

fn fault(id: &str) -> Value {
    json!({"v":1,"id":id,"kind":"fault","area":"git","title":"t","status":"live","created":"2026-10-24",
        "updated":"2026-10-24","by":"me","symptom":"s","cause":"c","fix":"f","check":"k"})
}

#[test]
fn round_trip_keeps_bytes_and_unknown_fields() {
    let mut first = fault("b-one");
    first["extra"] = json!({"x":[1,"ü"]});
    let records = vec![first, fault("a-two")];
    let bytes = serialize(&records);
    let back = parse(&bytes, "x.jsonl").unwrap();
    assert_eq!(serialize(&back), bytes);
    assert_eq!(back[1]["extra"], json!({"x":[1,"ü"]}));
    assert!(bytes.windows(2).any(|pair| pair == "ü".as_bytes()));
}

#[test]
fn items_sort_by_number() {
    let plan = json!({"v":1,"id":"pl","kind":"plan","area":"plans","title":"p","goal":"g","status":"live",
        "created":"2026-10-24","updated":"2026-10-24","by":"me"});
    let item = |n: u32| json!({"v":1,"id":format!("pl#{n}"),"kind":"item","area":"plans","plan":"pl",
        "title":format!("i{n}"),"status":"open","created":"2026-10-24","updated":"2026-10-24","by":"me"});
    let bytes = serialize(&[item(10), item(9), plan, item(2)]);
    let ids: Vec<String> = parse(&bytes, "p.jsonl").unwrap().iter().map(|r| r["id"].as_str().unwrap().into()).collect();
    assert_eq!(ids, ["pl", "pl#2", "pl#9", "pl#10"]);
}

#[test]
fn store_refusals_match_python() {
    let good = serialize(&[fault("aa"), fault("bb")]);
    let crlf: Vec<u8> = good.iter().flat_map(|byte| if *byte == b'\n' { vec![b'\r', b'\n'] } else { vec![*byte] }).collect();
    assert!(parse(&crlf, "x").is_err());
    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend(&good);
    assert!(parse(&bom, "x").unwrap_err().to_string().contains("BOM"));
    let lines: Vec<&[u8]> = good.split(|b| *b == b'\n').collect();
    let swapped = [lines[1], b"\n", lines[0], b"\n"].concat();
    assert!(parse(&swapped, "x").unwrap_err().to_string().contains("out of order"));
    let blank = [lines[0], b"\n\n", lines[1], b"\n"].concat();
    assert!(parse(&blank, "x").unwrap_err().to_string().contains("blank"));
    assert!(parse(&good[..good.len() - 1], "x").unwrap_err().to_string().contains("final newline"));
    let twice = [lines[0], b"\n", lines[0], b"\n"].concat();
    assert!(parse(&twice, "x").unwrap_err().to_string().contains("twice"));
    let spaced = canonical(&fault("aa")).replace("\":\"", "\": \"").into_bytes();
    assert!(parse(&[spaced, b"\n".to_vec()].concat(), "x").unwrap_err().to_string().contains("canonical"));
    assert!(parse(b"[1]\n", "x").is_err());
}

#[test]
fn schema_refusals_match_python() {
    let areas = vec!["git".to_owned()];
    assert!(validate(&fault("aa"), &areas, None).is_empty());
    let cases = [
        (json!({"v":1,"id":"aa","kind":"fault","area":"git","title":"t","status":"fixed","created":"2026-10-24",
            "updated":"2026-10-24","by":"me","symptom":"s","cause":"c","fix":"f","check":"k"}), "`status` must be one of"),
        (json!({"v":1,"id":"Bad Id","kind":"fault","area":"git","title":"t","status":"live","created":"2026-10-24",
            "updated":"2026-10-24","by":"me","symptom":"s","cause":"c","fix":"f","check":"k"}), "`id` must match"),
        (json!({"v":1,"id":"aa","kind":"fault","area":"nowhere","title":"t","status":"live","created":"2026-10-24",
            "updated":"2026-10-24","by":"me","symptom":"s","cause":"c","fix":"f","check":"k"}), "not one of lore.toml's areas"),
    ];
    for (record, words) in cases {
        let errors = validate(&record, &areas, None);
        assert!(errors.iter().any(|error| error.contains(words)), "{words} in {errors:?}");
    }
    let mut long = fault("aa");
    long["title"] = json!("x".repeat(121));
    assert!(validate(&long, &areas, None).iter().any(|e| e.contains("limit is 120")));
    let mut missing = fault("aa");
    missing.as_object_mut().unwrap().remove("cause");
    assert!(validate(&missing, &areas, None).iter().any(|e| e.contains("`cause` is required")));
    let mut item = json!({"v":1,"id":"pl#1","kind":"item","area":"plans","plan":"pl","title":"i","status":"done",
        "created":"2026-10-24","updated":"2026-10-24","by":"me"});
    assert!(validate(&item, &areas, None).iter().any(|e| e.contains("`commits` is required when `done`")));
    item["status"] = json!("dropped");
    item["reason"] = json!("r");
    assert!(validate(&item, &areas, None).iter().any(|e| e.contains("`decided_by` is required when `dropped`")));
}

