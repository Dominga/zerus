#!/usr/bin/env python3
"""Tests of client/scripts/measure-report.py on synthetic and recorded results.

    python3 -m unittest discover -s client/scripts -p "test_*.py"
"""
import importlib.util
import json
import pathlib
import re
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("measure_report", HERE / "measure-report.py")
report = importlib.util.module_from_spec(spec)
spec.loader.exec_module(report)
RESULTS = HERE.parent / "bench" / "results"


def pacing_stats(deltas, T):
    deltas = sorted(deltas)
    n = len(deltas)
    rank = lambda p: deltas[min(n, max(1, -(-p * n // 100))) - 1]  # noqa: E731
    return {
        "count": n,
        "mean": sum(deltas) / n,
        "median": deltas[n // 2] if n % 2 else (deltas[n // 2 - 1] + deltas[n // 2]) / 2,
        "p95": rank(95),
        "p99": rank(99),
        "over1_5T": sum(d > 1.5 * T for d in deltas) / n,
        "over2_5T": sum(d > 2.5 * T for d in deltas) / n,
        "band60": sum(15.0 <= d <= 18.5 for d in deltas) / n,
        "max": deltas[-1],
    }


def client_report(label, interval, hz=120, representative=True):
    T = 1000 / hz
    stats = pacing_stats([interval] * 1200, T)
    run = {"frames": 1200, "interval_ms": stats, "verdict": {}}
    scenarios = [
        {"id": sid, "label": sid, "kind": "pacing", "metric": "interval_ms", "runs": [run, run, run], "worst_index": 1, "worst": run}
        for sid, _ in report.PACING
    ]
    timing = {"count": 20, "mean": 40.0, "median": 39.5, "p95": 44.8, "p99": 51.0, "max": 51.0}
    scenarios.append({"id": "session-open", "kind": "timing", "metric": "wall_ms", "runs": [{"count": 20, "wall_ms": timing}], "worst_index": 0, "worst": {"count": 20, "wall_ms": timing}})
    latency = {"count": 130, "mean": 5.0, "median": 4.5, "p95": 8.2, "p99": 8.3, "max": 8.4}
    scenarios.append({"id": "input-latency", "kind": "latency", "metric": "latency_ms", "runs": [{"count": 130, "latency_ms": latency}], "worst_index": 0, "worst": {"count": 130, "latency_ms": latency, "to_paint_ms": latency}})
    return {
        "schema": "zerus-perf-v1",
        "client": "tauri",
        "created_at": "2026-10-28T00:00:00Z",
        "platform": {"os": "macos", "label": label, "hz": hz, "hz_source": "native", "T_ms": T, "cross_origin_isolated": True,
                     "timer_resolution_ms": 0.02, "representative": representative},
        "settings": {"warmupMs": 2000, "runMs": 10000, "runs": 3},
        "scenarios": scenarios,
        "resources": {"idle": {"total": {"cpu_percent": 0.8, "rss_bytes": 180 * 1024 * 1024, "footprint_bytes": 120 * 1024 * 1024}}},
    }


class MeasureReportTest(unittest.TestCase):
    def test_verdict_matches_the_spec_rules(self):
        T = 1000 / 120
        self.assertTrue(report.verdict(pacing_stats([T] * 1000, T), T)["all"])
        sixty = report.verdict(pacing_stats([1000 / 60] * 600, T), T)
        self.assertFalse(sixty["mean"])
        self.assertFalse(sixty["band60"])
        tail = report.verdict(pacing_stats([T] * 997 + [3 * T] * 3, T), T)
        self.assertEqual({k: v for k, v in tail.items() if v is False}, {"over2_5T": False, "all": False})
        T60 = 1000 / 60
        self.assertIsNone(report.verdict(pacing_stats([T60] * 600, T60), T60)["band60"])

    def test_page_marks_pass_and_fail_per_platform_without_dots(self):
        passing = client_report("mac-promotion-disabled", 1000 / 120)
        control = client_report("mac-promotion-default", 1000 / 60)
        page = report.build([passing, control])
        self.assertIn('<html lang="ru">', page)
        self.assertIn("mac-promotion-disabled", page)
        self.assertIn("mac-promotion-default", page)
        self.assertIn("проходит", page)
        self.assertIn("не проходит", page)
        # Five pacing scenarios pass for the disabled preference and fail in the control.
        disabled, default = page.split("mac-promotion-default", 1)[0], page.split("mac-promotion-default", 1)[1]
        self.assertEqual(disabled.split("<h2>Qt")[0].count('class="fail"'), 0)
        self.assertGreaterEqual(default.split("<h2>Qt")[0].count('class="fail"'), 5)
        self.assertFalse(any(c in page for c in report.SEPARATORS))
        self.assertIn("8,33" + report.NBSP + "мс", page)

    def test_an_estimated_rate_is_judged_against_the_display(self):
        # The control reads 60 Hz from rAF; judged against its own estimate it would pass.
        control = client_report("mac-promotion-default", 1000 / 60, hz=60)
        control["platform"]["hz_source"] = "estimated"
        self.assertNotIn("не проходит", report.build([control]).split("<h2>Qt")[0])
        judged = report.display_rate(control, report.parse_rates(["macos=120"]))
        self.assertEqual(judged["platform"]["T_ms"], 1000 / 120)
        self.assertEqual(judged["platform"]["estimated_hz"], 60)
        self.assertEqual(control["platform"]["hz"], 60)
        page = report.build([judged])
        self.assertGreaterEqual(page.split("<h2>Qt")[0].count('class="fail"'), 5)
        # A rate from the native side is kept; other systems are untouched.
        native = client_report("mac-native", 1000 / 120)
        self.assertIs(report.display_rate(native, {"macos": 60.0}), native)
        linux = dict(control, platform=control["platform"] | {"os": "linux"})
        self.assertIs(report.display_rate(linux, {"macos": 120.0}), linux)

    def test_summary_comes_first(self):
        page = report.build([client_report("synthetic", 1000 / 120)], summary="<p>Итог: macOS проходит.</p>")
        self.assertLess(page.index("Итог: macOS"), page.index("<h2>Критерий 1"))

    def test_separator_dots_are_refused(self):
        bad = client_report("mac " + chr(0x00B7) + " dots", 1000 / 120)
        with self.assertRaises(ValueError):
            report.build([bad])

    def test_recorded_qt_and_webkitgtk_results_render(self):
        files = sorted(RESULTS.glob("*.json"))
        self.assertTrue(files, "no recorded results in client/bench/results")
        reports = [json.loads(p.read_text(encoding="utf-8")) for p in files]
        kinds = {report.kind_of(r) for r in reports}
        self.assertIn("qt", kinds)
        page = report.build(reports + [client_report("synthetic-120", 1000 / 120)])
        self.assertIn("Qt, стенд", page)
        if "probe" in kinds:
            # The macOS display probe shows the display-link validity check.
            self.assertIn("не меньше 117,6", page)
        self.assertIn("не репрезентативно", page)
        # Every comparison row has a figure for the Qt harness.
        compare = page.split('class="compare"', 1)[1].split("</table>", 1)[0]
        self.assertEqual(len(re.findall("<tr><th scope=\"row\">", compare)), 6)

    def test_cli_writes_the_page(self):
        with tempfile.TemporaryDirectory() as tmp:
            source = pathlib.Path(tmp) / "client.json"
            source.write_text(json.dumps(client_report("synthetic", 1000 / 120)), encoding="utf-8")
            probe = pathlib.Path(tmp) / "probe.txt"
            probe.write_text("macOS: Version 27.0.1\n", encoding="utf-8")
            out = pathlib.Path(tmp) / "page.html"
            report.main([str(source), "--probe", str(probe), "--out", str(out)])
            text = out.read_text(encoding="utf-8")
            self.assertIn("Настройка WebKit и частота экрана на Mac", text)
            self.assertIn("Version 27.0.1", text)


if __name__ == "__main__":
    unittest.main()
