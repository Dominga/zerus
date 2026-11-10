# Shared source-keyed translations

`zerus_ru.json` holds the reviewed Russian translations retained from the former
Qt catalog, as plain JSON: source text to translation, a plural as its three
forms, and a disambiguated source keyed `disambiguation\u0004source`. Task #28
cut it from the retired `zerus_ru.ts` exactly as the generator read that file;
there is no Qt or XML step any more. Keep its source keys, plurals and
placeholder rules compatible; Next-only entries remain in
`client/src/i18n.next-ru.json`.

Run `python3 client/scripts/generate-i18n-catalog.py --check` after edits.
