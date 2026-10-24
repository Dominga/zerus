#include <QtTest>
#include <QFile>
#include <QJsonDocument>
#include <QSettings>
#include <QTemporaryDir>
#include <memory>

#include "CacheKeepalive.h"
#include "FakeCli.h"

// WhiteWh fork: the tray's cache keepalive (CacheKeepalive.h).
class TestCacheKeepalive : public QObject {
    Q_OBJECT
    std::unique_ptr<QTemporaryDir> m_settings;
private slots:
    void initTestCase() {
        m_settings = std::make_unique<QTemporaryDir>(); QVERIFY(m_settings->isValid());
        QSettings::setDefaultFormat(QSettings::IniFormat);
        QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, m_settings->path());
    }
    void hoursAreBoundedPerConversation();
    void warmsBeforeExpiryAndKeepsItsRepliesQuiet();
};

namespace {
SessionInfo session(const QString &reply, double at)
{
    SessionInfo s; s.name = "codex/project/main"; s.cmd = "codex"; s.state = "running"; s.processState = "running";
    s.activity = "idle"; s.phase = "idle"; s.runId = "run"; s.conversationId = "conversation"; s.replyId = reply; s.replyAt = at;
    return s;
}
BoxState box(const SessionInfo &s) { BoxState b; b.ok = true; b.sessions = {s}; return b; }
QStringList sent(const QString &calls)
{
    QFile file(calls); if (!file.open(QIODevice::ReadOnly)) return {};
    QStringList texts;
    for (const auto &line : file.readAll().split('\n'))
        if (!line.isEmpty()) texts << QJsonDocument::fromJson(line).object().value("text").toString();
    return texts;
}
}

void TestCacheKeepalive::hoursAreBoundedPerConversation()
{
    const auto s = session("1:100", 100);
    QCOMPARE(CacheKeepalive::hours({}, s), 0);
    CacheKeepalive::setHours({}, s, 30); QCOMPARE(CacheKeepalive::hours({}, s), int(CacheKeepalive::MaxHours));
    QCOMPARE(CacheKeepalive::hours("peer", s), 0);
    auto other = s; other.conversationId = "other"; QCOMPARE(CacheKeepalive::hours({}, other), 0);
    CacheKeepalive::setHours({}, s, 0); QCOMPARE(CacheKeepalive::hours({}, s), 0);
    QCOMPARE(CacheKeepalive::ttl({}), int(CacheKeepalive::DefaultTtl));
    QCOMPARE(CacheKeepalive::ttl(QJsonObject{{"session_usage", QJsonObject{{"prompt_cache", QJsonObject{{"ttl_seconds", 3600}}}}}}), 3600);
}

void TestCacheKeepalive::warmsBeforeExpiryAndKeepsItsRepliesQuiet()
{
    QTemporaryDir dir; QVERIFY(dir.isValid());
    const auto fixture = dir.filePath("hgs");
    QVERIFY(FakeCli::install(fixture, R"PY(#!/usr/bin/env python3
import sys,json
a=sys.argv[1:]
if 'send' in a:
    request=json.load(sys.stdin)
    open(__file__+'.calls','a').write(json.dumps(request)+'\n')
    print(json.dumps(dict(status='submitted',request_id=request['request_id'],agent_id='',name=a[a.index('send')+1],
        run_id=request['expected_run_id'],conversation_id=request['expected_conversation_id'])))
)PY"));
    const auto calls = fixture + ".calls";
    CacheKeepalive keepalive(FakeCli::program(fixture));
    auto s = session("1:100", 100);
    // Off: nothing is sent.
    QVERIFY(keepalive.observe({}, box(s), 280).isEmpty());
    CacheKeepalive::setHours({}, s, 1);
    // Five minutes of cache; the keepalive goes out in the last 90 seconds.
    QVERIFY(keepalive.observe({}, box(s), 300).isEmpty());
    QTest::qWait(200); QVERIFY(sent(calls).isEmpty());
    QVERIFY(keepalive.observe({}, box(s), 315).isEmpty());
    QTRY_COMPARE(sent(calls).size(), 1);
    QVERIFY(sent(calls).first().startsWith("<zerus-cache-keepalive>"));
    QVERIFY(keepalive.pending({}, s));
    // hgs left the reply as it was: idle again later means the keepalive ran.
    keepalive.observe({}, box(s), 340); QVERIFY(!keepalive.pending({}, s));
    keepalive.observe({}, box(s), 500); QCOMPARE(sent(calls).size(), 1);
    // The next one counts from the keepalive; its counted reply is handed back.
    keepalive.observe({}, box(s), 530); QTRY_COMPARE(sent(calls).size(), 2);
    auto answered = session("2:540", 540);
    const auto quiet = keepalive.observe({}, box(answered), 545);
    QCOMPARE(quiet.size(), 1); QCOMPARE(quiet.first().reply, QString("2:540")); QCOMPARE(quiet.first().conversation, QString("conversation"));
    // A real reply is the person's: not handed back.
    auto real = session("3:600", 600);
    QVERIFY(keepalive.observe({}, box(real), 605).isEmpty());
    // A busy turn uses the cache; a cold cache is never revived.
    auto busy = real; busy.activity = "busy"; busy.phase = "working";
    keepalive.observe({}, box(busy), 700);
    keepalive.observe({}, box(real), 1100); QTest::qWait(200); QCOMPARE(sent(calls).size(), 2);
    // A new real turn warms again until the hour without one has passed.
    auto later = session("4:2000", 2000);
    keepalive.observe({}, box(later), 2000);
    keepalive.observe({}, box(later), 2215); QTRY_COMPARE(sent(calls).size(), 3);
    QTest::qWait(150);
    for (double now = 2240; now < 2000 + 3600 + 600; now += 25) {
        const bool was = keepalive.pending({}, later); const int before = sent(calls).size();
        keepalive.observe({}, box(later), now);
        if (!was && keepalive.pending({}, later)) { QTRY_COMPARE(sent(calls).size(), before + 1); QTest::qWait(150); }
    }
    const int warmed = sent(calls).size();
    QVERIFY2(warmed >= 3 + 3600 / 300 - 1 && warmed <= 3 + 3600 / 210 + 1, qPrintable(QString::number(warmed)));
    for (double now = 2000 + 3600 + 600; now < 2000 + 3600 + 1800; now += 25) keepalive.observe({}, box(later), now);
    QTest::qWait(200); QCOMPARE(sent(calls).size(), warmed);
}

QTEST_GUILESS_MAIN(TestCacheKeepalive)
#include "test_cachekeepalive.moc"
