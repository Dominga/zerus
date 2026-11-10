#!/usr/bin/env python3
"""Merge the Next Russian catalogs without stacking text conflicts.

`.gitattributes` names the driver `i18n` for two files:

- `client/src/i18n.next-ru.json` is a reviewed key -> text object (a plural is
  a list of three forms). Two branches that add different keys merge as a key
  union; a key both sides changed differently stays a conflict with both
  values shown.
- `client/src/i18n.ru.json` is generated from the supplement and the shared Qt
  catalog (sorted keys). Each side was generated from its own supplement, so
  the same three-way key merge of its two sides is the catalog the merged
  supplement generates; it is written sorted, as the generator writes it.
  Task #22 (team ede724c4): it used to be regenerated from the supplement in
  the working tree, but Git writes the merged files only after every driver
  ran, so that read ours' supplement and dropped every key theirs added
  (merge 276e77248 lost task #7's strings that way).

Git runs the driver as
    python3 scripts/i18n-merge.py %O %A %B %L %P
and the result is left in %A; a conflict exits 1 and leaves conflict markers.
Zerus passes the same command with `-c merge.i18n.driver=...` on its own merge
paths and never writes it into a repository config (C2 refuses a repository's
own merge drivers). Contributors merging by hand enable it once:

    git config merge.i18n.driver "python3 scripts/i18n-merge.py %O %A %B %L %P"

(Windows: `python` instead of `python3`; Git for Windows runs the driver with
its shell.)
"""
import argparse
import importlib.util
import json
from pathlib import Path
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SUPPLEMENT = "client/src/i18n.next-ru.json"
GENERATED = "client/src/i18n.ru.json"
GENERATOR = "client/scripts/generate-i18n-catalog.py"
DELETED = object()


def read_json(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def write_text(path, text):
    # Newlines stay LF on Windows too: the checked-in catalogs use LF, and the
    # generator compares the text.
    with open(path, "w", encoding="utf-8", newline="\n") as handle:
        handle.write(text)


def serialized(catalog):
    return json.dumps(catalog, ensure_ascii=False, indent=2) + "\n"


def entry_text(key, value):
    """One entry exactly as `json.dumps(indent=2)` renders it inside an object."""
    dumped = json.dumps({key: value}, ensure_ascii=False, indent=2)
    return dumped.split("\n", 1)[1].rsplit("\n", 1)[0]


def merge_catalogs(base, ours, theirs):
    """Three-way key merge of the supplement.

    Returns `(items, conflicts)`: an item is `(key, value)` for a clean entry or
    `(key, ours_value, theirs_value)` for a conflict (a side may be DELETED).
    Ours' key order is kept; keys only theirs has follow in theirs' order.
    """
    items = []
    conflicts = 0
    order = list(ours) + [key for key in theirs if key not in ours]
    for key in order:
        mine = ours.get(key, DELETED)
        other = theirs.get(key, DELETED)
        common = base.get(key, DELETED)
        if mine is DELETED:
            if other is DELETED:
                continue
            if common is DELETED:
                items.append((key, other))
            elif other == common:
                continue  # deleted here, unchanged there
            else:
                items.append((key, DELETED, other))
                conflicts += 1
        elif other is DELETED:
            if common is DELETED:
                items.append((key, mine))
            elif mine == common:
                continue  # deleted there, unchanged here
            else:
                items.append((key, mine, DELETED))
                conflicts += 1
        elif mine == other:
            items.append((key, mine))
        elif common is DELETED:
            items.append((key, mine, other))
            conflicts += 1
        elif mine == common:
            items.append((key, other))
        elif other == common:
            items.append((key, mine))
        else:
            items.append((key, mine, other))
            conflicts += 1
    return items, conflicts


def render(items):
    """The catalog text; conflicting keys become Git conflict blocks."""
    if not items:
        return "{}\n"
    blocks = []
    for index, item in enumerate(items):
        comma = "," if index + 1 < len(items) else ""
        if len(item) == 2:
            blocks.append(entry_text(item[0], item[1]) + comma)
            continue
        key, mine, other = item
        ours = entry_text(key, mine) + comma if mine is not DELETED else ""
        theirs = entry_text(key, other) + comma if other is not DELETED else ""
        blocks.append(f"<<<<<<< ours\n{ours}\n=======\n{theirs}\n>>>>>>> theirs")
    return "{\n" + "\n".join(blocks) + "\n}\n"


def loaded(path):
    try:
        text = Path(path).read_text(encoding="utf-8")
    except OSError as error:
        raise ValueError(f"{path} was not read: {error}") from None
    # Git passes an empty file (or /dev/null) as %O when both sides added the
    # file: the empty catalog is the merge base.
    if not text.strip():
        return {}
    try:
        value = json.loads(text)
    except ValueError as error:
        raise ValueError(f"{path} is not a JSON object: {error}") from None
    if not isinstance(value, dict):
        raise ValueError(f"{path} is not a JSON object")
    return value


def generator():
    spec = importlib.util.spec_from_file_location("zerus_i18n_generator", ROOT / GENERATOR)
    module = importlib.util.module_from_spec(spec)
    if spec.loader is None:
        raise ValueError(f"{GENERATOR} is missing")
    spec.loader.exec_module(module)
    return module


def regenerate(supplement, shared):
    """`client/src/i18n.ru.json` for `supplement`, with the merged shared catalog."""
    module = generator()
    with tempfile.TemporaryDirectory(prefix="zerus-i18n-") as scratch:
        supplement_path = Path(scratch) / "i18n.next-ru.json"
        write_text(supplement_path, serialized(supplement))
        module.SUPPLEMENT = supplement_path
        if shared is not None:
            shared_path = Path(scratch) / "zerus_ru.ts"
            write_text(shared_path, shared)
            module.SOURCE = shared_path
        return module.generate()


def merge_supplement(base_path, ours_path, theirs_path):
    try:
        base, ours, theirs = loaded(base_path), loaded(ours_path), loaded(theirs_path)
    except ValueError as error:
        print(f"i18n-merge: {error}", file=sys.stderr)
        return 1
    items, conflicts = merge_catalogs(base, ours, theirs)
    # Task #22 (team ede724c4): the checked-in supplement is sorted (the
    # generator's --check requires it); a sorted side stays sorted, instead of
    # theirs' new keys trailing after ours' and failing that check.
    if list(ours) == sorted(ours):
        items.sort(key=lambda item: item[0])
    write_text(ours_path, render(items))
    if conflicts:
        print(f"i18n-merge: {conflicts} key(s) differ on both sides; both values are in the file",
              file=sys.stderr)
        return 1
    return 0


def merge_generated(base_path, ours_path, theirs_path):
    """The generated catalog: a three-way key merge of its two sides, sorted."""
    try:
        base, ours, theirs = loaded(base_path), loaded(ours_path), loaded(theirs_path)
    except ValueError as error:
        print(f"i18n-merge: {error}", file=sys.stderr)
        return 1
    items, conflicts = merge_catalogs(base, ours, theirs)
    if conflicts:
        write_text(ours_path, render(items))
        print(f"i18n-merge: {conflicts} key(s) of {GENERATED} differ on both sides; resolve {SUPPLEMENT}, "
              f"then run {GENERATOR}", file=sys.stderr)
        return 1
    merged = {item[0]: item[1] for item in items}
    write_text(ours_path, json.dumps(merged, ensure_ascii=False, indent=2, sort_keys=True) + "\n")
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("base", help="%O, the merge base")
    parser.add_argument("ours", help="%A, the current version (the result is written here)")
    parser.add_argument("theirs", help="%B, the other side")
    parser.add_argument("marker", help="%L, the conflict marker size")
    parser.add_argument("path", help="%P, the merged path relative to the repository root")
    arguments = parser.parse_args()
    relative = arguments.path.replace("\\", "/")
    if relative == SUPPLEMENT:
        return merge_supplement(arguments.base, arguments.ours, arguments.theirs)
    if relative == GENERATED:
        return merge_generated(arguments.base, arguments.ours, arguments.theirs)
    print(f"i18n-merge: {arguments.path} has no i18n merge rule", file=sys.stderr)
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
