# RustCodeLore search parity: DCB lore (text only)

Method: the owner's benchmark (`tools/lore_bench.py` in `D:/OwlCats/DCB`,
44 rows asked as `exact` and `para`, 88 queries each), on the read-only copy of
DCB's `lore/` + `lore.toml` (about 6100 records). Embeddings are off on both
sides: Python `lore --text-only`, Rust `Index::search_all` (FTS5 words +
identifier terms, RRF k=60, phrase bonus). Keys are matched against the full
record text, exactly as the bench does.

| class | Python hit@1 | Python hit@5 | Rust hit@1 | Rust hit@5 |
|---|---|---|---|---|
| exact | 93.2% | 97.7% | 89.8% (79/88) | 94.3% (83/88) |
| paraphrase | 31.8% | 51.1% | 43.2% (38/88) | 65.9% (58/88) |

The Rust side is better on paraphrases and worse on pasted errors. It is
therefore **not yet at parity**: exact hit@1 is 3.4 points lower and hit@5 3.4
points lower. Do not treat the search port as complete until exact reaches the
Python baseline.

The harness is `crates/lore/examples/parity.rs`:

```
cargo run --release --example parity -- <copy-with-lore> <lore_bench_queries.jsonl>
```

The corpus and query file are never committed; only this summary and the
harness are.

