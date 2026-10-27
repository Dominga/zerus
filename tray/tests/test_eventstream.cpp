#include <QtTest>

#include <QSignalSpy>
#include <QTemporaryDir>

#include "EventStream.h"
#include "FleetState.h"
#include "FakeCli.h"
#include "JsonParse.h"

// The desktop's consumer of `hgs watch --json` (EventStream.h), against a fake
// CLI that speaks the stream: listings and Activity arrive as the values polls
// deliver, parsed off the GUI thread, and a computer without the marker polls.
class TestEventStream : public QObject {
    Q_OBJECT
private slots:
    void init() { QVERIFY(m_dir.isValid()); }
    void listingAndActivityArriveFromTheStream();
    void peerListingsKeepTheAlias();
    void computerWithoutTheMarkerIsUnsupported();
    void endedStreamFailsAndRestarts();
    void streamEndingBeforePublicationNeverGoesLive();
    void unchangedListingIsAppliedAgain();

private:
    QString install(const QString &script);
    QTemporaryDir m_dir;
};

namespace {
const char *Stream = R"PY(#!/usr/bin/env python3
import json, sys
args = [a for a in sys.argv[1:] if not a.startswith('@')]
host = next((a[1:] for a in sys.argv[1:] if a.startswith('@')), '')
if args == ['--help']:
    print('usage: hgs ...\n       Event stream ABI: watch-v1')
    sys.exit(0)
if args != ['watch', '--json']:
    sys.exit(2)
seq = 0
def send(t, **fields):
    global seq
    seq += 1
    print(json.dumps(dict(v=1, seq=seq, t=t, **fields)), flush=True)
row = lambda name, run, activity: {'name': name, 'cmd': 'codex', 'run_id': run, 'conversation_id': 'c-' + run, 'activity': activity}
send('hello', machine_id=None, host='fixture', hgs_version='1.57.0', stream='s', heartbeat_ms=10000, max_frame_bytes=1048576,
     topics=['catalog', 'catalog.meta', 'activity'], limits={})
send('snapshot', topic='catalog', items=[{'key': 'run:r2', 'row': row('codex/zeta', 'r2', 'idle')},
                                          {'key': 'run:r1', 'row': row('codex/Alpha', 'r1', 'idle')}], page=0, last=True, cursor=1)
send('snapshot', topic='catalog.meta', value={'host': 'fixture', 'ok': True, 'peers': [], 'hgs_version': '1.57.0'}, page=0, last=True, cursor=1)
for text in sys.stdin:
    op = json.loads(text)
    if op.get('op') == 'subscribe':
        key = 'run:' + op['session']['run_id']
        send('events', topic='activity', key=key, events=[{'seq': 41, 'type': 'UserPromptSubmit', 'detail': 'go'}], cursor=41)
        send('state', topic='activity', key=key, value={'run_id': op['session']['run_id'], 'conversation_id': 'c-r1', 'activity': 'busy'}, rev=1)
        send('delta', topic='catalog', upsert=[{'key': 'run:r1', 'row': row('codex/Alpha', 'r1', 'busy')}], remove=[{'key': 'run:r2', 'reason': 'archived'}], cursor=2)
        send('events', topic='activity', key=key, events=[{'seq': 42, 'type': 'Stop', 'detail': 'done'}], cursor=42)
)PY";
}

QString TestEventStream::install(const QString &script)
{
    const auto path = m_dir.filePath(QStringLiteral("hgs-%1").arg(QUuid::createUuid().toString(QUuid::Id128)));
    if (!FakeCli::install(path, script.toUtf8())) return {};
    return FakeCli::program(path);
}

void TestEventStream::listingAndActivityArriveFromTheStream()
{
    int guiThreadParses = 0;
    JsonParse::setViolationHandler([&guiThreadParses](const char *, int) { ++guiThreadParses; });
    EventStream stream(install(QString::fromUtf8(Stream)), {});
    QSignalSpy listings(&stream, &EventStream::listingReady), activity(&stream, &EventStream::activityReady);
    stream.start();
    QTRY_VERIFY(stream.live());
    QTRY_VERIFY(!listings.isEmpty());
    auto box = listings.first().first().value<BoxState>();
    QCOMPARE(box.host, QStringLiteral("fixture"));
    QCOMPARE(box.sessions.size(), 2);
    // Listing order, as ls --json sorts names.
    QCOMPARE(box.sessions.at(0).name, QStringLiteral("codex/Alpha"));
    QCOMPARE(box.sessions.at(1).name, QStringLiteral("codex/zeta"));

    stream.follow(QStringLiteral("codex/Alpha"), QStringLiteral("r1"), QStringLiteral("c-r1"), 0);
    QTRY_VERIFY(activity.size() >= 1);
    QTRY_COMPARE(listings.last().first().value<BoxState>().sessions.size(), 1);
    box = listings.last().first().value<BoxState>();
    QCOMPARE(box.sessions.first().activity, QStringLiteral("busy"));
    // Increments are inspect-shaped: the summary with the new events and cursor.
    const auto received = [&activity] {
        qsizetype count = 0;
        for (const auto &arguments : activity) count += arguments.at(1).toJsonObject().value("events").toArray().size();
        return count;
    };
    QTRY_COMPARE(received(), 2);
    QJsonArray events; qint64 cursor = 0;
    for (const auto &arguments : activity) {
        const auto inspection = arguments.at(1).toJsonObject();
        QCOMPARE(arguments.at(0).toString(), QStringLiteral("codex/Alpha"));
        QCOMPARE(inspection.value("run_id").toString(), QStringLiteral("r1"));
        for (const auto &event : inspection.value("events").toArray()) events.append(event);
        cursor = inspection.value("cursor").toInteger();
    }
    QCOMPARE(events.at(0).toObject().value("seq").toInt(), 41);
    QCOMPARE(events.at(1).toObject().value("seq").toInt(), 42);
    QCOMPARE(cursor, 42);
    QCOMPARE(guiThreadParses, 0);
    JsonParse::setViolationHandler({});
    stream.stop();
    QCOMPARE(stream.state(), EventStream::State::Stopped);
}

void TestEventStream::peerListingsKeepTheAlias()
{
    EventStream stream(install(QString::fromUtf8(Stream)), QStringLiteral("laptop"));
    QSignalSpy listings(&stream, &EventStream::listingReady);
    stream.start();
    QTRY_VERIFY(!listings.isEmpty());
    const auto box = listings.first().first().value<BoxState>();
    // A peer is found, polled and addressed by its saved alias.
    QCOMPARE(box.host, QStringLiteral("laptop"));
    QCOMPARE(box.reportedHost, QStringLiteral("fixture"));
}

void TestEventStream::computerWithoutTheMarkerIsUnsupported()
{
    // Fake CLIs are Python scripts, which the Windows fake runs too.
    EventStream stream(install(QStringLiteral("#!/usr/bin/env python3\nprint('usage: hgs ...')\n")), {});
    stream.start();
    QTRY_COMPARE(stream.state(), EventStream::State::Unsupported);
    QVERIFY(!stream.live());
}

void TestEventStream::endedStreamFailsAndRestarts()
{
    const QString once = QStringLiteral(R"PY(#!/usr/bin/env python3
import json, sys
if sys.argv[1:] == ['--help']:
    print('Event stream ABI: watch-v1'); sys.exit(0)
print(json.dumps({'v': 1, 'seq': 1, 't': 'hello', 'host': 'fixture'}), flush=True)
print(json.dumps({'v': 1, 'seq': 2, 't': 'snapshot', 'topic': 'catalog', 'items': [], 'page': 0, 'last': True, 'cursor': 1}), flush=True)
print(json.dumps({'v': 1, 'seq': 3, 't': 'snapshot', 'topic': 'catalog.meta', 'value': {'host': 'fixture', 'ok': True}, 'page': 0, 'last': True, 'cursor': 1}), flush=True)
import time; time.sleep(1)
)PY");
    EventStream stream(install(once), {});
    QSignalSpy states(&stream, &EventStream::stateChanged);
    stream.start();
    QTRY_VERIFY(states.contains(QVariantList{QVariant::fromValue(EventStream::State::Live)}));
    QTRY_COMPARE(stream.state(), EventStream::State::Failed);
    QCOMPARE(EventStream::restartDelayMs(1), 5000);
    QCOMPARE(EventStream::restartDelayMs(9), 300000);
}

void TestEventStream::streamEndingBeforePublicationNeverGoesLive()
{
    // Frames prepared just before the child exits are dropped with it: a failed
    // stream never reports Live afterwards, so callers keep polling.
    const QString brief = QStringLiteral(R"PY(#!/usr/bin/env python3
import json, sys
if sys.argv[1:] == ['--help']:
    print('Event stream ABI: watch-v1'); sys.exit(0)
print(json.dumps({'v': 1, 'seq': 1, 't': 'snapshot', 'topic': 'catalog', 'items': [], 'page': 0, 'last': True, 'cursor': 1}), flush=True)
print(json.dumps({'v': 1, 'seq': 2, 't': 'snapshot', 'topic': 'catalog.meta', 'value': {'ok': True}, 'page': 0, 'last': True, 'cursor': 1}), flush=True)
)PY");
    EventStream stream(install(brief), {});
    QSignalSpy states(&stream, &EventStream::stateChanged);
    stream.start();
    QTRY_COMPARE(stream.state(), EventStream::State::Failed);
    QTest::qWait(200);
    QCOMPARE(stream.state(), EventStream::State::Failed);
    QVERIFY(!states.contains(QVariantList{QVariant::fromValue(EventStream::State::Live)}));
}

void TestEventStream::unchangedListingIsAppliedAgain()
{
    // Callers date a computer's data by its last listing (automatic approvals
    // need a peer listed within 30 s), so a quiet stream re-applies it.
    EventStream::setListingTimingForTests(100, 300);
    EventStream stream(install(QString::fromUtf8(Stream)), QStringLiteral("laptop"));
    QSignalSpy listings(&stream, &EventStream::listingReady);
    stream.start();
    QTRY_VERIFY(!listings.isEmpty());
    QTRY_VERIFY_WITH_TIMEOUT(listings.size() >= 3, 3000);
    // The same listing has the same digest, so the desktop only refreshes its
    // poll time (FleetState::refreshListing); a changed one has another.
    const auto first = listings.first().first().value<BoxState>(), again = listings.at(2).first().value<BoxState>();
    QCOMPARE(first.digest.size(), 32); QCOMPARE(again.digest, first.digest);
    FleetState fleet; BoxState local; local.host = "arch"; local.ok = true; local.peersKnown = true; local.peers = {"laptop"};
    fleet.setLocal(local, 0); fleet.setPeer(first, 1000);
    QVERIFY(fleet.refreshListing("laptop", again, 2000)); QCOMPARE(fleet.peerPolledAt("laptop"), qint64(2000));
    stream.follow(QStringLiteral("codex/Alpha"), QStringLiteral("r1"), QStringLiteral("c-r1"), 0);
    QTRY_COMPARE(listings.last().first().value<BoxState>().sessions.size(), 1);
    QVERIFY(listings.last().first().value<BoxState>().digest != first.digest);
    EventStream::setListingTimingForTests(2000, 10000);
}

QTEST_MAIN(TestEventStream)
#include "test_eventstream.moc"
