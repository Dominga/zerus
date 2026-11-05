# Shared source-keyed translations

These reviewed Russian translations and their bounded XML preparation helper
were retained from the former Qt catalog for the Next generator. They are data
and validation code, not a Qt build or runtime dependency. Keep their source keys,
context, plurals and placeholder rules compatible; Next-only entries remain in
`client/src/i18n.next-ru.json`.

Run `python3 client/scripts/generate-i18n-catalog.py --check` after edits. Historical
XML location attributes do not require the retired C++ files to exist.
