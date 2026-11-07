# RustCodeLore search parity: DCB lore

Method: the owner's benchmark (`tools/lore_bench.py` in `D:/OwlCats/DCB`, 88
rows of `tools/lore_bench_queries.jsonl`, each asked as `exact` and `para`: 176
queries) on a read-only copy of DCB's `lore/` + `lore.toml` (6773 records). A
result answers when one of the row's keys is in the strings of the record,
exactly as the bench judges. Python is CodeLore's own `find` (the conformance
reference, `D:/Tools/CodeLore`); Rust is the `lore` binary (`lore --json -C
<copy> find <query> -n 10`, one process per query) and, for the embedding rows,
the same index and search code driven by a harness on the task-143 branch.

| run | class | Python hit@1 | Python hit@5 | Rust hit@1 | Rust hit@5 |
|---|---|---|---|---|---|
| text only (`lore find --text-only`) | exact | 93.2% (82/88) | 97.7% (86/88) | 93.2% (82/88) | 97.7% (86/88) |
| text only | paraphrase | 31.8% (28/88) | 51.1% (45/88) | 31.8% (28/88) | 51.1% (45/88) |
| with embeddings (Ollama vectors) | exact | 94.3% (83/88) | 98.9% (87/88) | 94.3% (83/88) | 98.9% (87/88) |
| with embeddings (Ollama vectors) | paraphrase | 45.5% (40/88) | 75.0% (66/88) | 45.5% (40/88) | 75.0% (66/88) |

Rust is not worse: it is the same. Query by query, the top ten ids of all 176
queries are identical in identity and order, in all four runs (the text-only
rows through the task-144 binary, commit fc0dae513, the embedding rows on commit 356a541de, whose
index and search code is this crate's `index.rs` and `finder.rs` without a
textual difference); the largest difference between the two scores of one hit is
1e-5 (the final rounding).

## Not measured here

The full-corpus run with the **built-in model** (nomic-embed-text v1.5 on candle)
is not in this table: the embedding rows above used Ollama's vectors on both
sides. The built-in model's vectors are proven against the Python side's on the
task-148 fixture (50 records, 10 queries: minimum cosine 0.999991,
`tests/parity.rs`), and the full DCB run is task 177. Embedding the 6773 records
on the CPU takes tens of minutes, which is why a search never does it for a
user without `lore index`.

## What the earlier port got wrong

Earlier attempts reported exact 89.8% / 94.3% with three records not found at
all. The port differed below the ranking, in what is indexed and what may be
returned: no `main` column, other BM25 weights, a hand-picked key list for the
whole-term table instead of the TERM pattern over every string, no visibility
rule (retired records, source records and the items of a closed plan were
returned), the phrase scan over the wrong text, and ties broken by id instead
of by the order records were scored. `index.rs` and `finder.rs` are now a port
of `index.py` and `search.py` with the same schema (user_version 3) and the
same fusion, tie for tie.

The corpus and the query file are never committed; only this summary is.
