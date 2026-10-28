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
    void gitFoldersFollowTheStreamAndResubscribeAfterRestart();
    void providerMessagesMergeByIdRepeatAndTime();

private:
    QString install(const QString &script);
    QTemporaryDir m_dir;
};

namespace {
// A stream starts two fake CLI processes (marker probe, then the stream); a
// loaded runner needs more than QTRY's default 5 s.
constexpr int StartupMs = 15000;
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
        before = {'type': 'AgentMessage', 'source': 'codex_transcript', 'at': 1.0, 'message_id': '', 'agent_id': '', 'detail': 'Before.'}
        send('state', topic='activity', key=key, value={'run_id': op['session']['run_id'], 'conversation_id': 'c-r1', 'activity': 'busy',
             'provider_messages': [before]}, rev=1)
        send('delta', topic='catalog', upsert=[{'key': 'run:r1', 'row': row('codex/Alpha', 'r1', 'busy')}], remove=[{'key': 'run:r2', 'reason': 'archived'}], cursor=2)
        send('events', topic='activity', key=key, events=[{'seq': 42, 'type': 'Stop', 'detail': 'done'}], cursor=42)
        # Messages between tools: a new one, a repeat of the summary's, then a revision.
        checking = dict(before, at=2.0, message_id='m-1', detail='Checking.')
        send('event', topic='activity', key=key, id='m-1', rev=1, event=checking)
        send('event', topic='activity', key=key, id='AgentMessage@1.5#0', rev=1, event=dict(before, at=1.5))
        send('event', topic='activity', key=key, id='m-1', rev=2, event=dict(checking, detail='Checked.'))
)PY";

// Answers git subscriptions with state frames and logs every git operation
// as "RUN OP PATH". Its first run ends after it answers a folder named
// "restart"; "refused" folders get too_many_subscriptions.
const char *GitStream = R"PY(#!/usr/bin/env python3
import json, os, sys
args = [a for a in sys.argv[1:] if not a.startswith('@')]
if args == ['--help']:
    print('usage: hgs ...\n       Event stream ABI: watch-v1')
    sys.exit(0)
if args != ['watch', '--json']:
    sys.exit(2)
runs = __file__ + '.runs'
run = (int(open(runs).read()) if os.path.exists(runs) else 0) + 1
open(runs, 'w').write(str(run))
seq = 0
def send(t, **fields):
    global seq
    seq += 1
    print(json.dumps(dict(v=1, seq=seq, t=t, **fields)), flush=True)
send('hello', machine_id=None, host='fixture', hgs_version='1.57.0', stream='s%d' % run, heartbeat_ms=10000,
     max_frame_bytes=1048576, topics=['catalog', 'catalog.meta', 'activity', 'git'], limits={'git_subscriptions': 64})
send('snapshot', topic='catalog', items=[], page=0, last=True, cursor=1)
send('snapshot', topic='catalog.meta', value={'host': 'fixture', 'ok': True}, page=0, last=True, cursor=1)
rev = 0
for text in sys.stdin:
    op = json.loads(text)
    if op.get('topic') != 'git':
        continue
    with open(__file__ + '.ops', 'a') as log:
        log.write('%d %s %s\n' % (run, op['op'], op['path']))
    if op['op'] != 'subscribe':
        continue
    if 'refused' in op['path']:
        send('error', code='too_many_subscriptions', message='at most 64', topic='git', key=op['path'], fatal=False)
        continue
    rev += 1
    send('state', topic='git', key=op['path'], rev=rev, value={'state': 'ok', 'path': op['path'], 'root': op['path'],
         'branch': 'main', 'changed_files': rev, 'remote_state': 'no_upstream', 'sampled_at': 1.0})
    if run == 1 and op['path'].endswith('/restart'):
        sys.exit(0)
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
    stream.watchGit({QStringLiteral("/repo")});
    stream.start();
    QTRY_VERIFY_WITH_TIMEOUT(stream.live(), StartupMs);
    QTRY_VERIFY_WITH_TIMEOUT(!listings.isEmpty(), StartupMs);
    // A stream without the git topic leaves its folders to git-status polls.
    QVERIFY(!stream.coversGit(QStringLiteral("/repo")));
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
    // Messages between tools join the summary's provider_messages.
    const auto details = [&activity] {
        QStringList texts;
        for (const auto &message : activity.last().at(1).toJsonObject().value("provider_messages").toArray())
            texts << message.toObject().value("detail").toString();
        return texts;
    };
    QTRY_COMPARE(details(), (QStringList{"Before.", "Checked."}));
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
    QTRY_VERIFY_WITH_TIMEOUT(!listings.isEmpty(), StartupMs);
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
    QTRY_COMPARE_WITH_TIMEOUT(stream.state(), EventStream::State::Unsupported, StartupMs);
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
    QTRY_VERIFY_WITH_TIMEOUT(states.contains(QVariantList{QVariant::fromValue(EventStream::State::Live)}), StartupMs);
    QTRY_COMPARE_WITH_TIMEOUT(stream.state(), EventStream::State::Failed, StartupMs);
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
    QTRY_COMPARE_WITH_TIMEOUT(stream.state(), EventStream::State::Failed, StartupMs);
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
    QTRY_VERIFY_WITH_TIMEOUT(!listings.isEmpty(), StartupMs);
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

void TestEventStream::gitFoldersFollowTheStreamAndResubscribeAfterRestart()
{
    int guiThreadParses = 0;
    JsonParse::setViolationHandler([&guiThreadParses](const char *, int) { ++guiThreadParses; });
    const auto program = install(QString::fromUtf8(GitStream));
    // The script beside the program (FakeCli.h), for its sidecar log.
    auto script = program;
    if (script.endsWith(QStringLiteral(".exe"))) script.chop(4);
    const auto operations = [&] {
        QFile log(script + QStringLiteral(".ops"));
        return log.open(QIODevice::ReadOnly) ? QString::fromUtf8(log.readAll()).split('\n', Qt::SkipEmptyParts) : QStringList();
    };
    EventStream stream(program, {});
    QSignalSpy states(&stream, &EventStream::gitStatusReady);
    // Duplicates and empty paths are dropped; nothing is covered before the stream is live.
    stream.watchGit({"/repo/a", "/repo/a", "", "/repo/b", "/repo/refused"});
    QVERIFY(!stream.coversGit("/repo/a"));
    stream.start();
    QTRY_VERIFY_WITH_TIMEOUT(stream.live(), StartupMs);
    QTRY_COMPARE(states.size(), 2);
    QSet<QString> paths;
    for (const auto &arguments : std::as_const(states)) paths << arguments.at(0).toString();
    QCOMPARE(paths, (QSet<QString>{"/repo/a", "/repo/b"}));
    QCOMPARE(states.first().at(1).toJsonObject().value("branch").toString(), QStringLiteral("main"));
    // A refused folder goes back to polling; the others need no poll.
    QTRY_VERIFY(!stream.coversGit("/repo/refused"));
    QVERIFY(stream.coversGit("/repo/a")); QVERIFY(stream.coversGit("/repo/b")); QVERIFY(!stream.coversGit("/repo/other"));
    QCOMPARE(operations(), (QStringList{"1 subscribe /repo/a", "1 subscribe /repo/b", "1 subscribe /repo/refused"}));

    // A new set unsubscribes the folders it leaves out and subscribes the new ones.
    stream.watchGit({"/repo/b", "/repo/c"});
    QVERIFY(!stream.coversGit("/repo/a")); QVERIFY(stream.coversGit("/repo/c"));
    QTRY_COMPARE(states.size(), 3);
    QCOMPARE(states.last().at(0).toString(), QStringLiteral("/repo/c"));
    QTRY_COMPARE(operations().size(), 5);
    QCOMPARE(operations().mid(3), (QStringList{"1 unsubscribe /repo/a", "1 subscribe /repo/c"}));

    // The first 64 folders count; the caller passes its preferred ones first.
    QStringList many;
    for (int i = 0; i < EventStream::GitFolders + 6; ++i) many << QStringLiteral("/repo/many-%1").arg(i);
    stream.watchGit(many);
    QVERIFY(stream.coversGit("/repo/many-63")); QVERIFY(!stream.coversGit("/repo/many-64"));
    QTRY_COMPARE(states.size(), 3 + EventStream::GitFolders);
    QVERIFY(!operations().contains("1 subscribe /repo/many-64"));

    // A restarted stream subscribes its folders again; meanwhile they are polled.
    stream.watchGit({"/repo/b", "/repo/restart"});
    QTRY_COMPARE_WITH_TIMEOUT(stream.state(), EventStream::State::Failed, StartupMs);
    QVERIFY(!stream.coversGit("/repo/b"));
    QTRY_VERIFY_WITH_TIMEOUT(stream.live(), 20000);
    QTRY_VERIFY(operations().contains("2 subscribe /repo/b") && operations().contains("2 subscribe /repo/restart"));
    QVERIFY(stream.coversGit("/repo/b"));
    QCOMPARE(guiThreadParses, 0);
    JsonParse::setViolationHandler({});
}

QTEST_MAIN(TestEventStream)
void TestEventStream::providerMessagesMergeByIdRepeatAndTime()
{
    const auto message = [](const QString &type, double at, const QString &id, const QString &detail) {
        return QJsonObject{{"type", type}, {"source", "claude_transcript"}, {"at", at}, {"message_id", id}, {"detail", detail}};
    };
    const auto texts = [](const QJsonObject &summary) {
        QStringList out;
        for (const auto &value : summary.value("provider_messages").toArray()) out << value.toObject().value("detail").toString();
        return out;
    };
    QJsonObject summary{{"name", "claude/p/t"}};
    EventStream::mergeProviderMessage(summary, message("AgentMessage", 10, "a", "Ten."));
    EventStream::mergeProviderMessage(summary, message("AgentMessage", 30, "c", "Thirty."));
    // Inserted in time order.
    EventStream::mergeProviderMessage(summary, message("AgentMessage", 20, "b", "Twenty."));
    QCOMPARE(texts(summary), (QStringList{"Ten.", "Twenty.", "Thirty."}));
    // The same message ID and type replaces; thinking with that ID is another message.
    EventStream::mergeProviderMessage(summary, message("AgentMessage", 20, "b", "Twenty, revised."));
    EventStream::mergeProviderMessage(summary, message("AgentThinking", 20, "b", "Twenty."));
    QCOMPARE(texts(summary), (QStringList{"Ten.", "Twenty, revised.", "Twenty.", "Thirty."}));
    // Without an ID, the same type and text within 3 s is a repeat; later it is new.
    EventStream::mergeProviderMessage(summary, message("AgentMessage", 12, "", "Ten."));
    QCOMPARE(texts(summary).size(), 4);
    EventStream::mergeProviderMessage(summary, message("AgentMessage", 40, "", "Ten."));
    QCOMPARE(texts(summary).last(), QStringLiteral("Ten."));
    QCOMPARE(texts(summary).size(), 5);
    // The newest 100 stay.
    for (int i = 0; i < 120; ++i) EventStream::mergeProviderMessage(summary, message("AgentMessage", 100 + i, QString::number(i), QString::number(i)));
    const auto kept = texts(summary);
    QCOMPARE(kept.size(), 100);
    QCOMPARE(kept.first(), QStringLiteral("20"));
    QCOMPARE(kept.last(), QStringLiteral("119"));
    QCOMPARE(summary.value("name").toString(), QStringLiteral("claude/p/t"));
}

#include "test_eventstream.moc"
