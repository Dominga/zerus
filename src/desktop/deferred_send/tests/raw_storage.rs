//! Corrupt only synthetic SQLite rows, never an installed state database.
use super::*;

fn raw_row(db: &rusqlite::Connection, id: &str) -> Vec<(String, Option<Vec<u8>>)> {
    let columns = [
        "request_id",
        "session",
        "fingerprint",
        "source",
        "checkpoint",
        "plan",
        "receipt",
        "writer_epoch",
        "phase",
        "drained",
        "ticket",
        "custody",
        "created",
    ];
    let projection = columns
        .iter()
        .map(|column| format!("typeof({column}),CAST({column} AS BLOB)"))
        .collect::<Vec<_>>()
        .join(",");
    db.query_row(
        &format!("SELECT {projection} FROM desktop_deferred_sends WHERE request_id=?1"),
        [id],
        |row| {
            (0..columns.len())
                .map(|i| Ok((row.get(2 * i)?, row.get(2 * i + 1)?)))
                .collect()
        },
    )
    .unwrap()
}

fn corruptible(db: &rusqlite::Connection) {
    db.execute_batch("DROP TRIGGER desktop_deferred_send_original; DROP TRIGGER desktop_deferred_send_custody; PRAGMA ignore_check_constraints=ON;").unwrap();
}

fn rejects_corruption(
    columns: &[&str],
    with_custody: bool,
    corrupt: impl Fn(&str, &str) -> rusqlite::types::Value,
) {
    let mut accepted = Vec::new();
    for column in columns {
        let (_temp, location, mut store, epoch) = fixture();
        let verified = original(&location.configuration, &epoch);
        let _held = if with_custody {
            Some(running(&mut store, &location, &verified, &epoch).1)
        } else {
            admit(&mut store, &verified, &epoch);
            None
        };
        let id = &verified.plan.request_id;
        let raw: String = store
            .db
            .query_row(
                &format!("SELECT {column} FROM desktop_deferred_sends WHERE request_id=?1"),
                [id],
                |r| r.get(0),
            )
            .unwrap();
        corruptible(&store.db);
        store
            .db
            .execute(
                &format!("UPDATE desktop_deferred_sends SET {column}=?1 WHERE request_id=?2"),
                rusqlite::params![corrupt(column, &raw), id],
            )
            .unwrap();
        let before = raw_row(&store.db, id);
        let counts = budget(&store.db);
        let refused = ledger::load(&store.db, id).is_err();
        let startup_refused = ledger::startup(&mut store.db).is_err();
        if !refused || !startup_refused {
            accepted.push((*column, refused, startup_refused));
        } else {
            assert_eq!(
                raw_row(&store.db, id),
                before,
                "{column}: raw evidence changed"
            );
            assert_eq!(budget(&store.db), counts, "{column}: accounting changed");
        }
    }
    assert!(
        accepted.is_empty(),
        "corrupt columns decoded as valid proof (load/startup refusal): {accepted:?}"
    );
}

fn rejects_nul_suffixes(columns: &[&str], with_custody: bool) {
    rejects_corruption(columns, with_custody, |_, raw| {
        format!("{raw}\0corrupt suffix").into()
    });
}

#[test]
fn raw_proof_json_nul_suffixes_are_unreadable() {
    rejects_nul_suffixes(&["source", "checkpoint", "plan", "receipt"], false);
}

#[test]
fn raw_reservation_json_nul_suffixes_are_unreadable() {
    rejects_nul_suffixes(&["ticket", "custody"], true);
}

#[test]
fn raw_identity_string_nul_suffixes_are_unreadable() {
    rejects_nul_suffixes(&["fingerprint", "writer_epoch", "session"], false);
}

#[test]
fn raw_blobs_cannot_be_cast_into_valid_text_proofs() {
    rejects_corruption(
        &[
            "source",
            "checkpoint",
            "plan",
            "receipt",
            "fingerprint",
            "writer_epoch",
            "session",
            "phase",
            "ticket",
            "custody",
        ],
        true,
        |_, raw| raw.as_bytes().to_vec().into(),
    );
}

#[test]
fn raw_json_byte_bounds_include_trailing_whitespace() {
    rejects_corruption(
        &[
            "source",
            "checkpoint",
            "plan",
            "receipt",
            "ticket",
            "custody",
        ],
        true,
        |column, raw| {
            let limit = match column {
                "source" => 65536,
                "checkpoint" | "plan" => 16384,
                "receipt" => 24576,
                "ticket" => 4096,
                "custody" => 8192,
                _ => unreachable!(),
            };
            format!("{raw}{}", " ".repeat(limit + 1 - raw.len())).into()
        },
    );
}

#[test]
fn escaped_json_and_unicode_keep_exact_byte_charges_and_unknown_no_replay() {
    let (_temp, location, mut store, epoch) = fixture();
    let mut intent = original_text(&location.configuration, &epoch, "Привет\n\t你好").intent;
    intent.original = format!(
        " \n{}\t ",
        intent
            .original
            .replace("Привет", r"\u041f\u0440\u0438\u0432\u0435\u0442")
    );
    let verified = intent.verify().unwrap();
    let row = admit(&mut store, &verified, &epoch);
    assert_eq!(row.verified.intent.original, verified.intent.original);
    assert!(row.ticket.is_none() && row.custody.is_none());
    let charge = i64::try_from(
        verified.intent.original.len()
            + verified.intent.checkpoint.len()
            + serde_json::to_string(&verified.plan).unwrap().len()
            + 24576
            + 8192
            + 4096
            + 1024,
    )
    .unwrap();
    assert_eq!(budget(&store.db), (1, charge, 1));
    ledger::startup(&mut store.db).unwrap();
    let retired = ledger::load(&store.db, &verified.plan.request_id)
        .unwrap()
        .unwrap();
    assert_eq!(retired.receipt.phase, Phase::Unknown);
    assert!(retired.receipt.drained);
    assert_eq!(retired.verified.intent.original, verified.intent.original);
    assert_eq!(
        admit(&mut store, &verified, &epoch).receipt,
        retired.receipt
    );
    assert!(ledger::pending(&store.db).unwrap().is_none());
    assert_eq!(budget(&store.db), (0, 0, 0));
}

#[test]
fn legacy_reconciliation_refuses_corrupt_history_without_releasing_its_charge() {
    let (_temp, location, mut store, epoch) = fixture();
    let mut originals = Vec::new();
    let mut bytes = 0;
    for _ in 0..2 {
        let verified = original(&location.configuration, &epoch);
        let row = admit(&mut store, &verified, &epoch);
        bytes += budget(&store.db).1;
        let tx = store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        ledger::retire(&tx, &row).unwrap();
        tx.commit().unwrap();
        originals.push(verified);
    }
    assert_eq!(budget(&store.db), (0, 0, 0));
    store
        .db
        .execute(
            "UPDATE desktop_deferred_send_budget SET rows=2,bytes=?1 WHERE id=1",
            [bytes],
        )
        .unwrap();
    corruptible(&store.db);
    let bad = &originals[1];
    store
        .db
        .execute(
            "UPDATE desktop_deferred_sends SET source=?1 WHERE request_id=?2",
            rusqlite::params![
                format!("{}\0suffix", bad.intent.original),
                bad.plan.request_id
            ],
        )
        .unwrap();
    let before: Vec<_> = originals
        .iter()
        .map(|v| raw_row(&store.db, &v.plan.request_id))
        .collect();
    assert!(ledger::startup(&mut store.db).is_err());
    assert_eq!(budget(&store.db), (2, bytes, 0));
    assert_eq!(
        originals
            .iter()
            .map(|v| raw_row(&store.db, &v.plan.request_id))
            .collect::<Vec<_>>(),
        before
    );
    let good = ledger::load(&store.db, &originals[0].plan.request_id)
        .unwrap()
        .unwrap();
    assert_eq!(good.receipt.phase, Phase::Unknown);
    assert!(good.receipt.drained);
    assert!(ledger::pending(&store.db).unwrap().is_none());
}

#[test]
fn raw_request_id_suffix_cannot_alias_another_valid_original() {
    let (_temp, location, mut store, epoch) = fixture();
    let first = original(&location.configuration, &epoch);
    let second = original(&location.configuration, &epoch);
    admit(&mut store, &first, &epoch);
    admit(&mut store, &second, &epoch);
    corruptible(&store.db);
    let alias = format!("{}\0suffix", first.plan.request_id);
    store
        .db
        .execute(
            "UPDATE desktop_deferred_sends SET request_id=?1 WHERE request_id=?2",
            rusqlite::params![alias, second.plan.request_id],
        )
        .unwrap();
    let before = [
        raw_row(&store.db, &first.plan.request_id),
        raw_row(&store.db, &alias),
    ];
    let counts = budget(&store.db);
    assert!(ledger::undrained(&store.db).is_err());
    assert!(ledger::startup(&mut store.db).is_err());
    assert_eq!(
        [
            raw_row(&store.db, &first.plan.request_id),
            raw_row(&store.db, &alias)
        ],
        before
    );
    assert_eq!(budget(&store.db), counts);
}

#[test]
fn raw_history_phase_suffix_is_unreadable_metadata() {
    let (_temp, location, mut store, epoch) = fixture();
    let verified = original(&location.configuration, &epoch);
    admit(&mut store, &verified, &epoch);
    ledger::startup(&mut store.db).unwrap();
    corruptible(&store.db);
    store
        .db
        .execute(
            "UPDATE desktop_deferred_sends SET phase=?1 WHERE request_id=?2",
            rusqlite::params!["unknown\0suffix", verified.plan.request_id],
        )
        .unwrap();
    let before = raw_row(&store.db, &verified.plan.request_id);
    assert!(ledger::summary(&store.db).is_err());
    assert_eq!(raw_row(&store.db, &verified.plan.request_id), before);
    assert_eq!(budget(&store.db), (0, 0, 0));
}
