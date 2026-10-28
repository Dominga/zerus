#!/usr/bin/env python3
"""Builds the Russian results page of the client measurement (zerus-5wmh.4).

Reads result files of schema zerus-perf-v1:
  - client reports (client "tauri"): the Diagnostics panel or client/bench/run-webkitgtk.py;
  - Qt harness reports (client "qt" with scenarios): client/bench/run-qt-bench.sh;
  - running-GUI samples (client "qt" with resources.running_gui): client/bench/sample-running-gui.py;
and optionally the text output of client/perf-probe-mac (--probe).
Writes one self-contained HTML page: the macOS criterion 1 table per platform
(pass/fail), p95 frame time while scrolling, input-to-frame latency, session open
time, idle CPU and RSS, Qt against the client.

The page never separates values with dots (owner rule, 2026-10-27) and holds no
session names: the inputs carry timings and process names only.

    python3 client/scripts/measure-report.py RESULT.json... [--probe probe.txt] [--out page.html]
"""
import argparse
import html
import json
import math
import pathlib
import sys

# Built from code points so this file never contains the characters it forbids.
SEPARATORS = "".join(chr(c) for c in (0x00B7, 0x2022, 0x2219, 0x22C5, 0x2027, 0x2981, 0x30FB))
NBSP = chr(0xA0)
BAND = (15.0, 18.5)

PACING = [
    ("raf-light", "rAF на лёгкой странице"),
    ("transform-animation", "rAF во время CSS-анимации transform"),
    ("left-animation", "rAF во время анимации left (без ускорения)"),
    ("list-scroll", "Прокрутка списка сессий"),
    ("activity-scroll", "Прокрутка длинной Activity"),
]
CRITERIA = [
    ("mean", "Среднее в пределах T ± 1%"),
    ("median", "Медиана в пределах T ± 0,5 мс"),
    ("within1_5T", "Не меньше 99% интервалов до 1,5 T"),
    ("over2_5T", "Не больше 0,2% интервалов выше 2,5 T"),
    ("band60", "Меньше 1% интервалов в полосе 15,0–18,5 мс"),
]


def finite(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)


def num(value, digits=2, unit=""):
    if not finite(value):
        return "н/д"
    text = f"{value:.{digits}f}".replace(".", ",")
    return f"{text}{NBSP}{unit}" if unit else text


def pct(share, digits=2):
    return num(share * 100, digits, "%") if finite(share) else "н/д"


def mib(value):
    return num(value / (1024 * 1024), 0, "МиБ") if finite(value) else "н/д"


def verdict(stats, T):
    """Criterion 1 of the spec, recomputed from the statistics (the same rules as pacing.ts)."""
    if not stats or not finite(stats.get("mean")) or not stats.get("count"):
        return {key: False for key, _ in CRITERIA} | {"all": False}
    eps = 1e-9
    result = {
        "mean": abs(stats["mean"] - T) <= 0.01 * T + eps,
        "median": abs(stats["median"] - T) <= 0.5 + eps,
        "within1_5T": stats["over1_5T"] <= 0.01 + eps,
        "over2_5T": stats["over2_5T"] <= 0.002 + eps,
        "band60": None if BAND[0] <= T <= BAND[1] else stats["band60"] < 0.01,
    }
    result["all"] = all(v is not False for v in result.values())
    return result


def mark(ok):
    if ok is None:
        return '<span class="na">н/п</span>'
    return '<span class="pass">проходит</span>' if ok else '<span class="fail">не проходит</span>'


def esc(text):
    return html.escape(str(text), quote=True)


def scenario(report, sid):
    for item in report.get("scenarios", []):
        if item.get("id") == sid:
            return item
    return None


def kind_of(report):
    if report.get("client") == "tauri":
        return "client"
    if report.get("client") == "qt" and report.get("scenarios"):
        return "qt"
    if report.get("client") == "qt" and (report.get("resources") or {}).get("running_gui"):
        return "qt-live"
    if report.get("client") == "probe" and report.get("webkit"):
        return "probe"
    return None


def probe_section(probes):
    """The WebKit preference and the display rate from client/perf-probe-mac --json."""
    rows = []
    for probe in probes:
        webkit, display = probe.get("webkit", {}), probe.get("display", {})
        disabled, control = webkit.get("disabled", {}), webkit.get("control", {})

        def state(value):
            return {True: "включена", False: "выключена"}.get(value, "н/д")

        links = [num(hz, 1, "Гц") for hz in display.get("display_link_hz") or []]
        lowest = min((hz for hz in display.get("display_link_hz") or [] if finite(hz)), default=None)
        valid = mark(lowest >= 117.6) if finite(lowest) else '<span class="na">не измерено</span>'
        rows.append(
            '<tr><th scope="row">' + esc(probe.get("platform", {}).get("os_version", "macOS")) + "</th>"
            f"<td>{'найдена' if webkit.get('found') else 'не найдена'} среди {esc(webkit.get('features', 'н/д'))}, "
            f"по умолчанию {state(webkit.get('default'))}</td>"
            f"<td>до {state(disabled.get('before'))}, после {state(disabled.get('after'))}</td>"
            f"<td>до {state(control.get('before'))}, после {state(control.get('after'))}</td>"
            f"<td>{num(display.get('screen_max_fps'), 0, 'Гц')}</td>"
            f"<td>{', '.join(links) or 'н/д'}<div class=\"fig\">не меньше 117,6 Гц: {valid}</div></td></tr>"
        )
    return (
        '<div class="scroll"><table><thead><tr><th scope="col">Система</th><th scope="col">Настройка WebKit</th>'
        '<th scope="col">Отключение</th><th scope="col">Контроль (по умолчанию)</th>'
        '<th scope="col">NSScreen, максимум</th><th scope="col">Display link, три замера по 2 с</th></tr></thead><tbody>'
        + "".join(rows)
        + "</tbody></table></div>"
    )


def title_of(report):
    kind = kind_of(report)
    platform = report.get("platform", {})
    label = platform.get("label") or platform.get("os") or "без метки"
    name = {"client": "Новый клиент", "qt": "Qt, стенд", "qt-live": "Qt, запущенный GUI"}[kind]
    engine = platform.get("engine") or (f"Qt {platform['qt']}" if platform.get("qt") else "")
    parts = [name, label] + ([engine] if engine else [])
    return ", ".join(parts)


def representative(report):
    return report.get("platform", {}).get("representative", True) is not False


def criteria_section(clients):
    if not clients:
        return "<p>Нет результатов нового клиента.</p>"
    out = []
    for report in clients:
        platform = report.get("platform", {})
        T = platform.get("T_ms") or (1000.0 / platform["hz"] if platform.get("hz") else None)
        head = (
            f"<h3>{esc(title_of(report))}</h3><p class=\"meta\">Частота экрана {num(platform.get('hz'), 0, 'Гц')}"
            f" (источник: {esc(platform.get('hz_source', 'н/д'))}), T = {num(T, 2, 'мс')}, "
            f"изоляция страницы: {'да' if platform.get('cross_origin_isolated') else 'нет'}, "
            f"шаг таймера {num(platform.get('timer_resolution_ms'), 3, 'мс')}, "
            f"прогонов {esc(report.get('settings', {}).get('runs', 'н/д'))}, засчитан худший."
        )
        if not representative(report):
            head += ' <span class="warn">Не репрезентативно (Xvfb или синтетический стенд).</span>'
        head += "</p>"
        rows = []
        for sid, label in PACING:
            item = scenario(report, sid)
            if not item:
                continue
            if item.get("skipped"):
                rows.append(f"<tr><th scope=\"row\">{esc(label)}</th><td colspan=\"{len(CRITERIA) + 2}\">пропущено: {esc(item['skipped'])}</td></tr>")
                continue
            stats = (item.get("worst") or {}).get("interval_ms") or {}
            v = verdict(stats, T) if T else {}
            cells = "".join(
                f"<td>{mark(v.get(key))}<div class=\"fig\">{figure(key, stats, T)}</div></td>" for key, _ in CRITERIA
            )
            rows.append(
                f"<tr><th scope=\"row\">{esc(label)}</th><td>{num(stats.get('p95'), 2, 'мс')}</td>{cells}<td>{mark(v.get('all'))}</td></tr>"
            )
        header = "".join(f"<th scope=\"col\">{esc(text)}</th>" for _, text in CRITERIA)
        out.append(
            head
            + '<div class="scroll"><table><thead><tr><th scope="col">Сценарий</th><th scope="col">p95 интервала</th>'
            + header
            + '<th scope="col">Итог</th></tr></thead><tbody>'
            + "".join(rows)
            + "</tbody></table></div>"
        )
    return "".join(out)


def figure(key, stats, T):
    if key == "mean":
        return num(stats.get("mean"), 2, "мс")
    if key == "median":
        return num(stats.get("median"), 2, "мс")
    if key == "within1_5T":
        return pct(1 - stats["over1_5T"]) if finite(stats.get("over1_5T")) else "н/д"
    if key == "over2_5T":
        return pct(stats.get("over2_5T"))
    return pct(stats.get("band60"))


def client_figures(report):
    figures = {}
    for sid, key in (("list-scroll", "list"), ("activity-scroll", "activity")):
        item = scenario(report, sid)
        stats = ((item or {}).get("worst") or {}).get("interval_ms") or {}
        figures[key] = (stats.get("p95"), "интервал rAF")
    item = scenario(report, "input-latency")
    worst = (item or {}).get("worst") or {}
    figures["input"] = (worst.get("latency_ms") or {}, "до следующего кадра")
    item = scenario(report, "session-open")
    figures["open"] = (((item or {}).get("worst") or {}).get("wall_ms") or {}, "до первой отрисовки")
    idle = (report.get("resources") or {}).get("idle") or {}
    total = idle.get("total") or idle.get("webkit") or {}
    figures["cpu"] = (total.get("cpu_percent"), "дерево процессов" if "total" in idle else "процессы WebKit")
    figures["rss"] = (total.get("rss_bytes"), total.get("pss_bytes"))
    return figures


def qt_figures(report):
    figures = {}
    for sid, key in (("list-scroll", "list"), ("activity-scroll", "activity")):
        stats = ((scenario(report, sid) or {}).get("worst") or {}).get("wall_ms") or {}
        figures[key] = (stats.get("p95"), "работа кадра")
    figures["input"] = ((((scenario(report, "input-latency") or {}).get("worst")) or {}).get("latency_ms") or {}, "до конца отрисовки")
    figures["open"] = ((((scenario(report, "session-open") or {}).get("worst")) or {}).get("wall_ms") or {}, "до первой отрисовки")
    idle = ((scenario(report, "idle") or {}).get("worst")) or {}
    figures["cpu"] = (idle.get("cpu_percent"), "процесс стенда")
    figures["rss"] = (idle.get("rss_bytes"), None)
    return figures


def live_figures(report):
    gui = report["resources"]["running_gui"]
    return {
        "cpu": (gui["app"].get("cpu_percent"), f"за {num(gui.get('window_ms', 0) / 1000, 0, 'с')}"),
        "rss": (gui["app"].get("rss_bytes"), gui["app"].get("pss_bytes")),
    }


def comparison_section(reports):
    columns = []
    for report in reports:
        kind = kind_of(report)
        figures = {"client": client_figures, "qt": qt_figures, "qt-live": live_figures}[kind](report)
        columns.append((title_of(report), representative(report), figures))
    if not columns:
        return "<p>Нет данных для сравнения.</p>"

    def frame(key):
        def cell(figures):
            value = figures.get(key)
            if not value:
                return "—"
            return f"{num(value[0], 2, 'мс')}<div class=\"fig\">{esc(value[1])}</div>"
        return cell

    def timing(key):
        def cell(figures):
            value = figures.get(key)
            if not value or not value[0]:
                return "—"
            s = value[0]
            return f"медиана {num(s.get('median'), 2, 'мс')}<br>p95 {num(s.get('p95'), 2, 'мс')}<div class=\"fig\">{esc(value[1])}, замеров {esc(s.get('count', 0))}</div>"
        return cell

    def cpu(figures):
        value = figures.get("cpu")
        if not value or not finite(value[0]):
            return "—"
        return f"{num(value[0], 2, '%')}<div class=\"fig\">{esc(value[1])}</div>"

    def rss(figures):
        value = figures.get("rss")
        if not value or not finite(value[0]):
            return "—"
        extra = f"<div class=\"fig\">PSS {mib(value[1])}</div>" if finite(value[1]) else ""
        return f"{mib(value[0])}{extra}"

    rows = [
        ("p95 времени кадра, прокрутка списка", frame("list")),
        ("p95 времени кадра, прокрутка Activity", frame("activity")),
        ("Задержка от ввода до кадра", timing("input")),
        ("Открытие сессии", timing("open")),
        ("CPU в простое", cpu),
        ("Память (RSS)", rss),
    ]
    warn = '<div class="warn">не репрезентативно</div>'
    head = "".join(f'<th scope="col">{esc(title)}{"" if rep else warn}</th>' for title, rep, _ in columns)
    body = "".join(
        f"<tr><th scope=\"row\">{esc(label)}</th>" + "".join(f"<td>{fn(figures)}</td>" for _, _, figures in columns) + "</tr>"
        for label, fn in rows
    )
    return f'<div class="scroll"><table class="compare"><thead><tr><th scope="col">Показатель</th>{head}</tr></thead><tbody>{body}</tbody></table></div>'


LABELS = dict(PACING) | {
    "session-open": "Открытие сессии до первой отрисовки Activity",
    "input-latency": "Задержка от ввода до кадра",
    "idle": "Простой",
}


def setting(value):
    if isinstance(value, bool):
        return "да" if value else "нет"
    if isinstance(value, list):
        return " и ".join(str(v) for v in value)
    return str(value)


def details_section(reports):
    out = []
    for report in reports:
        kind = kind_of(report)
        rows = []
        for item in report.get("scenarios", []):
            runs = item.get("runs") or []
            cells = []
            for i, run in enumerate(runs):
                stats = run.get("interval_ms") or run.get("wall_ms") or run.get("latency_ms") or {}
                flag = " (худший)" if i == item.get("worst_index") else ""
                cells.append(
                    f"прогон {i + 1}{flag}: среднее {num(stats.get('mean'))}, медиана {num(stats.get('median'))}, "
                    f"p95 {num(stats.get('p95'))}, p99 {num(stats.get('p99'))} мс, замеров {esc(stats.get('count', 0))}"
                )
            if item.get("id") == "idle":
                worst = item.get("worst") or {}
                cells.append(f"CPU {num(worst.get('cpu_percent'), 2, '%')}, RSS {mib(worst.get('rss_bytes'))}")
            if item.get("skipped"):
                cells.append(f"пропущено: {esc(item['skipped'])}")
            name = LABELS.get(item.get("id")) or item.get("label") or item.get("id")
            rows.append(f"<li><strong>{esc(name)}</strong><br>{'<br>'.join(cells)}</li>")
        if kind == "qt-live":
            gui = report["resources"]["running_gui"]
            for p in gui.get("processes", []):
                rows.append(
                    f"<li>{esc(p['role'])} {esc(p['name'])}: CPU {num(p.get('cpu_percent'), 2, '%')}, RSS {mib(p.get('rss_bytes'))}, PSS {mib(p.get('pss_bytes'))}</li>"
                )
        idle = (report.get("resources") or {}).get("idle") or {}
        for p in idle.get("processes", []):
            rows.append(
                f"<li>{esc(p.get('role'))} {esc(p.get('name'))}: CPU {num(p.get('cpu_percent'), 2, '%')}, RSS {mib(p.get('rss_bytes'))}, PSS {mib(p.get('pss_bytes'))}</li>"
            )
        settings = report.get("settings") or {}
        meta = ", ".join(f"{esc(k)} {esc(setting(v))}" for k, v in settings.items())
        out.append(
            f"<details><summary>{esc(title_of(report))}</summary><p class=\"meta\">{esc(report.get('created_at', ''))}"
            f"{', ' + meta if meta else ''}</p><ul>{''.join(rows)}</ul></details>"
        )
    return "".join(out)


STYLE = """
:root { --bg: #f7f7f5; --fg: #1d1f22; --dim: #5d636b; --line: #d9dbde; --card: #ffffff;
  --pass: #1d7a3a; --fail: #b3261e; --warn: #8a5a00; color-scheme: light; }
@media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) { --bg: #121416; --fg: #e6e8eb; --dim: #9aa0a8;
  --line: #2b2f35; --card: #1a1d20; --pass: #6bd08a; --fail: #ff8a80; --warn: #f0c060; color-scheme: dark; } }
:root[data-theme="dark"] { --bg: #121416; --fg: #e6e8eb; --dim: #9aa0a8; --line: #2b2f35; --card: #1a1d20;
  --pass: #6bd08a; --fail: #ff8a80; --warn: #f0c060; color-scheme: dark; }
body { margin: 0; background: var(--bg); color: var(--fg); font: 15px/1.5 system-ui, -apple-system, "Segoe UI", sans-serif; }
main { max-width: 1180px; margin: 0 auto; padding: 24px 16px 48px; }
h1 { font-size: 26px; margin: 0 0 6px; } h2 { font-size: 20px; margin: 32px 0 8px; } h3 { font-size: 16px; margin: 20px 0 4px; }
.meta, .fig { color: var(--dim); font-size: 13px; } .fig { margin-top: 2px; }
.scroll { overflow-x: auto; border: 1px solid var(--line); border-radius: 8px; background: var(--card); }
table { border-collapse: collapse; width: 100%; font-variant-numeric: tabular-nums; }
th, td { padding: 8px 10px; border-bottom: 1px solid var(--line); text-align: left; vertical-align: top; }
thead th { font-size: 13px; color: var(--dim); font-weight: 600; }
tbody tr:last-child th, tbody tr:last-child td { border-bottom: 0; }
.pass { color: var(--pass); font-weight: 600; } .fail { color: var(--fail); font-weight: 600; }
.na { color: var(--dim); } .warn { color: var(--warn); font-size: 13px; }
details { margin: 8px 0; padding: 8px 12px; border: 1px solid var(--line); border-radius: 8px; background: var(--card); }
summary { cursor: pointer; font-weight: 600; } li { margin: 6px 0; }
pre { overflow-x: auto; padding: 12px; background: var(--card); border: 1px solid var(--line); border-radius: 8px; font-size: 13px; }
"""


def expand(report):
    """A Qt harness report that also carries a running-GUI sample becomes two reports."""
    live = (report.get("resources") or {}).get("running_gui")
    if report.get("client") != "qt" or not report.get("scenarios") or not live:
        return [report]
    platform = dict(report.get("platform", {}))
    sample = live.get("platform") or {}
    live_report = {
        "schema": report.get("schema"),
        "client": "qt",
        "created_at": live.get("created_at", report.get("created_at")),
        "platform": {"os": platform.get("os"), "label": sample.get("label") or platform.get("label"), "representative": True},
        "resources": {"running_gui": live},
    }
    bench = dict(report)
    bench["resources"] = {k: v for k, v in (report.get("resources") or {}).items() if k != "running_gui"}
    return [bench, live_report]


def build(reports, probe=None, created=None, summary=None):
    reports = [part for r in reports for part in expand(r)]
    clients = [r for r in reports if kind_of(r) == "client"]
    ordered = clients + [r for r in reports if kind_of(r) == "qt"] + [r for r in reports if kind_of(r) == "qt-live"]
    created = created or max((r.get("created_at", "") for r in reports), default="")
    body = [
        "<h1>Замеры нового клиента Zerus</h1>",
        f"<p class=\"meta\">Задача zerus-5wmh.4, данные от {esc(created[:10] or 'н/д')}. "
        "Один тяжёлый экран: список сессий и длинная Activity. Худший из прогонов после разогрева.</p>",
    ]
    if summary:
        body.append(f"<section class=\"summary\">{summary}</section>")
    body += [
        "<h2>Критерий 1: частота кадров равна частоте экрана</h2>",
        "<p>Правила спецификации: T = 1000 / частота экрана; среднее интервалов rAF в пределах T ± 1%, "
        "медиана в пределах T ± 0,5 мс, не меньше 99% интервалов до 1,5 T, не больше 0,2% выше 2,5 T, "
        "меньше 1% в полосе 15,0–18,5 мс. На экране 60 Гц полоса совпадает с самим кадром, поэтому её проверка "
        "не применяется (н/п).</p>",
        criteria_section(clients),
        "<h2>Qt и новый клиент</h2>",
        "<p>У клиента время кадра при прокрутке это интервал между кадрами rAF: он не меньше периода экрана и растёт, "
        "когда кадры пропускаются. У Qt это время работы одного кадра в GUI-потоке (шаг прокрутки, синхронная "
        "отрисовка, обработка событий), без ожидания экрана: так считались цифры прокрутки Activity в "
        "zerus-tllp и zerus-pgo9. Задержка ввода у клиента идёт до следующего кадра rAF, у Qt до конца отрисовки "
        "виджета. Открытие сессии: от выбора до первой отрисовки Activity.</p>",
        comparison_section(ordered),
    ]
    probes = [r for r in reports if kind_of(r) == "probe"]
    if probes or probe:
        body.append("<h2>Настройка WebKit и частота экрана на Mac</h2>")
    if probes:
        body.append(probe_section(probes))
    if probe:
        body.append(f"<pre>{esc(probe)}</pre>")
    body += ["<h2>Все прогоны</h2>", details_section(ordered)]
    page = (
        "<!doctype html>\n<html lang=\"ru\"><head><meta charset=\"utf-8\">"
        "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">"
        f"<title>Замеры клиента Zerus</title><style>{STYLE}</style></head><body><main>"
        + "".join(body)
        + "</main></body></html>\n"
    )
    bad = [c for c in SEPARATORS if c in page]
    if bad:
        raise ValueError("separator dots in the page: " + ", ".join(f"U+{ord(c):04X}" for c in bad))
    return page


def display_rate(report, rates):
    """Judges a client report whose rate was estimated from rAF against the display's
    measured rate instead: the estimate reads 60 exactly when WebKit caps rendering."""
    platform = report.get("platform", {})
    if report.get("client") != "tauri" or platform.get("hz_source") != "estimated":
        return report
    hz = rates.get(platform.get("os"))
    if not hz:
        return report
    report = dict(report)
    report["platform"] = platform | {
        "hz": hz, "T_ms": 1000.0 / hz, "estimated_hz": platform.get("hz"),
        "hz_source": f"экран, {num(hz, 0, 'Гц')} по замеру дисплея; оценка по rAF {num(platform.get('hz'), 0, 'Гц')}",
    }
    return report


def parse_rates(values):
    rates = {}
    for value in values or []:
        os_name, _, hz = value.partition("=")
        try:
            rates[os_name] = float(hz)
        except ValueError:
            sys.exit(f"--display-hz takes OS=HZ, not {value!r}")
    return rates


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("results", nargs="+", type=pathlib.Path)
    parser.add_argument("--probe", type=pathlib.Path, help="text output of client/perf-probe-mac")
    parser.add_argument("--display-hz", action="append", metavar="OS=HZ",
                        help="the display's measured rate for client reports that only estimated it (macos=120)")
    parser.add_argument("--summary", type=pathlib.Path, help="an HTML fragment shown first: the results in short and the decision")
    parser.add_argument("--out", type=pathlib.Path)
    args = parser.parse_args(argv)
    rates = parse_rates(args.display_hz)
    reports = []
    for path in args.results:
        report = json.loads(path.read_text(encoding="utf-8"))
        if report.get("schema") != "zerus-perf-v1" or not kind_of(report):
            sys.exit(f"{path}: not a zerus-perf-v1 result")
        reports.append(display_rate(report, rates))
    probe = args.probe.read_text(encoding="utf-8") if args.probe else None
    summary = args.summary.read_text(encoding="utf-8") if args.summary else None
    page = build(reports, probe, summary=summary)
    if args.out:
        args.out.write_text(page, encoding="utf-8")
    else:
        sys.stdout.write(page)


if __name__ == "__main__":
    main()
