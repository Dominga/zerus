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

    def test_json_parse_has_no_gui_thread_exception(self):
        # Owner decision, 2026-10-27: the JsonParse::local() list is empty at the end of M2.
        self.assertFalse(hasattr(check, "LOCAL_JSON_CALLS"))
        self.assertFalse((ROOT / "tray/src/JsonParse.h").exists(), "the Qt parser is retired")



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

    def test_every_local_call_is_rejected(self):
        problems = check.json_parse_findings({
            "tray/src/New.cpp": "JsonParse::localArray(key);\nauto v = JsonParse :: local(bytes);\n",
            check.JSON_PARSER: "QJsonDocument local(const QByteArray &bytes) { return JsonParse::localObject(bytes); }\n",
        })
        self.assertEqual(problems, [
            "tray/src/JsonParse.cpp:1: JsonParse::local() is gone; parse off the GUI thread (tray/src/JsonParse.h)",
            "tray/src/New.cpp:1: JsonParse::local() is gone; parse off the GUI thread (tray/src/JsonParse.h)",
            "tray/src/New.cpp:2: JsonParse::local() is gone; parse off the GUI thread (tray/src/JsonParse.h)",
        ])

    def test_findings_keep_source_order(self):
        problems = check.json_parse_findings({
            "tray/src/Mixed.cpp": "JsonParse::localObject(a);\nQJsonDocument::fromJson(b);\n",
        })
        self.assertEqual(problems, [
            "tray/src/Mixed.cpp:1: JsonParse::local() is gone; parse off the GUI thread (tray/src/JsonParse.h)",
            "tray/src/Mixed.cpp:2: parse JSON with JsonParse (tray/src/JsonParse.h), off the GUI thread",
        ])


if __name__ == "__main__":
    unittest.main()
