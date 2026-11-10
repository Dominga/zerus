#!/usr/bin/env python3
"""Generate Next's source-keyed Russian catalog from the reviewed shared
catalog and the reviewed Next-only supplement.

Task #28 (team ede724c4): the shared catalog is plain JSON
(client/i18n/shared/zerus_ru.json), source -> translation, a plural as its
three forms and a disambiguated source keyed "disambiguation\u0004source"; it
was cut from the retired Qt catalog (zerus_ru.ts) exactly as this generator
read it."""
import argparse
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "client/i18n/shared/zerus_ru.json"
SUPPLEMENT = ROOT / "client/src/i18n.next-ru.json"
DESTINATION = ROOT / "client/src/i18n.ru.json"
PLACEHOLDERS = re.compile(r"%L?\d+|%n")


def supplement(catalog: dict, entries: dict) -> dict:
    """Adds Next-only translations under the shared catalog's own rules: a source the
    Qt catalog already translates stays there, placeholders stay exact, plurals
    need %n and three Russian forms, and no value carries a separator dot."""
    merged = dict(catalog)
    for source, value in entries.items():
        if not isinstance(source, str) or not source or "\u0004" in source:
            raise ValueError(f"Invalid supplement source: {source!r}")
        if source in catalog:
            raise ValueError(f"Translate {source!r} in zerus_ru.json, not in the supplement")
        values = value if isinstance(value, list) else [value]
        if isinstance(value, list) and (len(value) != 3 or "%n" not in source):
            raise ValueError(f"Russian plurals need %n and three forms: {source!r}")
        for text in values:
            if not isinstance(text, str) or not text.strip():
                raise ValueError(f"Empty supplement translation: {source!r}")
            if sorted(PLACEHOLDERS.findall(source)) != sorted(PLACEHOLDERS.findall(text)):
                raise ValueError(f"Changed placeholders: {source!r}")
            if "·" in text:
                raise ValueError(f"Separator dot in supplement translation: {source!r}")
        merged[source] = value
    return merged


def generate() -> str:
    catalog = json.loads(SOURCE.read_text(encoding="utf-8"))
    if not isinstance(catalog, dict):
        raise ValueError(f"{SOURCE.relative_to(ROOT)} is not a JSON object")
    catalog = supplement(catalog, json.loads(SUPPLEMENT.read_text(encoding="utf-8")))
    return json.dumps(catalog, ensure_ascii=False, indent=2, sort_keys=True) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail unless the catalog is current and the supplement is sorted")
    args = parser.parse_args()
    generated = generate()
    supplement_text = json.dumps(json.loads(SUPPLEMENT.read_text(encoding="utf-8")), ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    if args.check:
        if (not DESTINATION.exists() or DESTINATION.read_text(encoding="utf-8") != generated
                or SUPPLEMENT.read_text(encoding="utf-8") != supplement_text):
            raise SystemExit(f"Run {Path(__file__).relative_to(ROOT)} to sort the supplement and refresh the catalog")
        print(f"Next Russian catalog is current: {DESTINATION.relative_to(ROOT)}")
    else:
        SUPPLEMENT.write_text(supplement_text, encoding="utf-8")
        DESTINATION.write_text(generated, encoding="utf-8")
        print(f"Wrote {DESTINATION.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
