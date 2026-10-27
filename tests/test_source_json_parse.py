"""Exercise the desktop JSON parsing rule of scripts/ci/check-source.py."""
import importlib.util
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("check_source", ROOT / "scripts/ci/check-source.py")
check = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(check)


class CurrentTreeTest(unittest.TestCase):
    def test_desktop_parses_json_only_through_json_parse(self):
        files = {path.relative_to(ROOT).as_posix(): path.read_text(encoding="utf-8")
                 for path in (ROOT / "tray/src").rglob("*") if path.suffix in {".cpp", ".h", ".mm"}}
        self.assertEqual(check.json_parse_findings(files), [])
        self.assertLessEqual(set(check.LOCAL_JSON_CALLS), set(files), "remove allowances of deleted files")


class RuleTest(unittest.TestCase):
    def test_direct_decoders_are_rejected_outside_the_parser(self):
        problems = check.json_parse_findings({
            "tray/src/View.cpp": "void f(const QByteArray &b) {\n    auto d = QJsonDocument::fromJson(b);\n}\n",
            "tray/src/Cbor.cpp": "auto v = QCborValue :: fromCbor(bytes);\n",
            check.JSON_PARSER: "QJsonDocument document = QJsonDocument::fromJson(bytes, &error);\n",
        })
        self.assertEqual(problems, [
            "tray/src/Cbor.cpp:1: parse JSON with JsonParse (tray/src/JsonParse.h), off the GUI thread",
            "tray/src/View.cpp:2: parse JSON with JsonParse (tray/src/JsonParse.h), off the GUI thread",
        ])

    def test_comments_and_strings_do_not_count(self):
        problems = check.json_parse_findings({
            "tray/src/Notes.h": "// QJsonDocument::fromJson is rejected here\n"
                                "const char *text = \"JsonParse::local(bytes)\";\n",
        })
        self.assertEqual(problems, [])

    def test_local_calls_must_match_the_allowance_exactly(self):
        listed = next(iter(check.LOCAL_JSON_CALLS))
        allowed = check.LOCAL_JSON_CALLS[listed]
        extra = "".join("JsonParse::localObject(a);\n" for _ in range(allowed + 1))
        fewer = "".join("JsonParse::local(a);\n" for _ in range(allowed - 1))
        self.assertEqual(check.json_parse_findings({"tray/src/New.cpp": "JsonParse::localArray(key);\n"}),
                         ["tray/src/New.cpp: 1 JsonParse::local() calls, 0 allowed; parse off the GUI thread instead"])
        self.assertEqual(check.json_parse_findings({listed: extra}),
                         [f"{listed}: {allowed + 1} JsonParse::local() calls, {allowed} allowed; parse off the GUI thread instead"])
        self.assertEqual(check.json_parse_findings({listed: fewer}),
                         [f"{listed}: lower LOCAL_JSON_CALLS to {allowed - 1} in scripts/ci/check-source.py"])


if __name__ == "__main__":
    unittest.main()
