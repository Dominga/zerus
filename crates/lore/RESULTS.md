# RustCodeLore search parity: DCB lore

Method: the owner's benchmark (`tools/lore_bench.py` in `D:/OwlCats/DCB`,
88 rows of `tools/lore_bench_queries.jsonl`, each asked as `exact` and `para`:
176 queries) on a read-only copy of DCB's `lore/` + `lore.toml` (6773 records).
A result answers when one of the row's keys is in the strings of the record,
exactly as the bench judges. Python is CodeLore's own `find` (the conformance
reference); Rust is `zerus_lore::search::find`. Both were run with the text side
only and with the meaning list (Ollama `nomic-embed-text`, each side embedding
the corpus itself).

| run | class | Python hit@1 | Python hit@5 | Rust hit@1 | Rust hit@5 |
|---|---|---|---|---|---|
| text only | exact | 93.2% (82/88) | 97.7% (86/88) | 93.2% (82/88) | 97.7% (86/88) |
| text only | paraphrase | 31.8% (28/88) | 51.1% (45/88) | 31.8% (28/88) | 51.1% (45/88) |
| with embeddings | exact | 94.3% (83/88) | 98.9% (87/88) | 94.3% (83/88) | 98.9% (87/88) |
| with embeddings | paraphrase | 45.5% (40/88) | 75.0% (66/88) | 45.5% (40/88) | 75.0% (66/88) |

Rust is not worse: it is the same. Query by query, the top ten ids of all 176
queries are identical in identity and order in both runs; the largest
difference between the two scores of one hit is 1e-5 (the final rounding).

## What the earlier port got wrong

Earlier attempts reported exact 89.8% / 94.3% with three records not found at
all. The port differed below the ranking, in what is indexed and what may be
returned: no `main` column (the refs and signals were folded into other
columns), different BM25 weights, a hand-picked key list for the whole-term
table instead of the TERM pattern over every string, no visibility rule
(retired records, source records and the items of a closed plan were returned),
the phrase scan over the wrong text, and ties broken by id instead of by the
order records were scored. `index.rs` and `search.rs` are now a port of
`index.py` and `search.py` with the same schema (user_version 3) and the same
fusion, tie for tie.

## Harness

`crates/lore/examples/parity.rs` runs the bench on a checkout and writes the
top ten of every query as JSON lines, for a query-by-query comparison with
Python's own ranking:

```
cargo run --release --example parity -- <copy-with-lore> <lore_bench_queries.jsonl> [out.jsonl [semantic]]
```

The corpus and the query file are never committed; only this summary and the
harness are.
