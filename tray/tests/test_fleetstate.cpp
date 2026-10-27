#include <QtTest>
#include "FleetState.h"
#include "SessionPresentation.h"

static BoxState box(const QString &host, bool ok, int sessions)
{
    BoxState b;
    b.host = host;
    b.ok = ok;
    for (int i = 0; i < sessions; ++i) {
        SessionInfo s;
        s.name = QStringLiteral("claude/p%1").arg(i);
        b.sessions << s;
    }
    return b;
}

static SessionInfo shared(const QString &reply = "7:100.5", const QString &signature = {})
{
    SessionInfo s; s.name = "codex/project/review"; s.cmd = "codex"; s.state = "running"; s.runId = "run-1";
    s.conversationId = "conversation-1"; s.created = 100; s.replyId = reply; s.sharedRead = true;
    s.attentionSignature = signature;
    if (!signature.isEmpty()) { s.phase = "input"; s.attentionId = "call-1"; }
    return s;
}

class TestFleetState : public QObject {
    Q_OBJECT
private slots:
    void sumsAcrossBoxes();
    void attentionMarksFollowConversationAndRejectStaleActions();
    void attentionMarksIgnoreRoutineEventsButKeepNewRequestsUnread();
    void archivesDoNotCountAsActiveFleet();
    void peerNeverPolledIsStale();
    void freshPeerIsNotStale();
    void stalePeerStillCounts();
    void localIsAlwaysFresh();
    void peerGoingOfflineKeepsLastKnownSessions();
    void peerNeverSeenAliveCountsAsZero();
    void tooltipShowsPollAgeForOfflinePeer();
    void tooltipAgeIsMinuteGrained();
    void sharedReadComesFromTheSessionHost();
    void sharedMarksApplyAtOnceAndSettleFromSnapshots();
    void unknownOptionalQuestionsKeepAttentionMarks();
    void queuedMarksNeverHideNewerRepliesOrRequests();
    void legacyMarksMigrateOnlyCurrentValues();
    void olderHostsKeepLocalReadStateAndQueueNothing();
    void recoveryAttentionWithoutSignatureStaysLocal();
    void controlOnlyComputerHasNoLocalCount();
    void tooltipCountsUseSingularForOne();
    void idleShellWorkKeepsActiveAndPreservesInputAndAttention();
    void processInspectionUpdatesWithoutNewHookAndRejectsOtherRuns();
    void intermediateRepliesStayUnreadUntilWorkFinishes();
    void unchangedListingRefreshesOnlyObservationFields();
    void refreshedPeersStayFreshAndChangedListingsApplyInFull();
    void viewCopiesTakeRefreshedObservations();
    void combinedReadStateMatchesSeparatePasses();
};

void TestFleetState::intermediateRepliesStayUnreadUntilWorkFinishes()
{
    for (const bool sharedRead : {false, true}) {
        BoxState box; box.ok = true; box.host = "computer"; box.sessions = {shared()};
        auto &s = box.sessions[0]; s.sharedRead = sharedRead; s.tracked = true;
        s.activity = "busy"; s.phase = "working";
        FleetState fleet;
        const auto waiting = [&] {
            fleet.setLocal(box, 0);
            const auto &observed = fleet.local().sessions[0];
            QVERIFY(observed.unreadReply); QVERIFY(!observed.visibleUnreadReply());
            QVERIFY(!observed.needsAttention()); QCOMPARE(fleet.attentionSessions(0), 0);
            QVERIFY(SessionPresentation::active(observed));
            QCOMPARE(observed.replyId, s.replyId); QVERIFY(observed.readReplyId.isEmpty());
            QVERIFY(fleet.readReplies().isEmpty()); QVERIFY(fleet.takeNewReadMarks().isEmpty());
        };
        waiting();
        s.activity = "idle"; s.phase = "idle"; s.backgroundProcesses = 1; waiting();
        s.backgroundProcesses = 0; s.subagentActiveCount = 1; waiting();
        s.reviewLater = true; QVERIFY(s.needsAttention()); s.reviewLater = false;
        s.phase = "approval"; QVERIFY(s.needsAttention()); s.phase = "idle";
        s.subagentActiveCount = 0; fleet.setLocal(box, 1);
        QVERIFY(fleet.local().sessions[0].unreadReply);
        QVERIFY(fleet.local().sessions[0].visibleUnreadReply()); QCOMPARE(fleet.attentionSessions(1), 1);
        QCOMPARE(fleet.local().sessions[0].replyId, s.replyId);
        QVERIFY(fleet.markReplyRead({}, s.name, s.conversationId, s.replyId));
        QVERIFY(!fleet.local().sessions[0].visibleUnreadReply());
    }
}

void TestFleetState::idleShellWorkKeepsActiveAndPreservesInputAndAttention()
{
    auto s = shared(); s.tracked = true; s.activity = "idle"; s.phase = "idle";
    s.processState = "running"; s.backgroundProcesses = 1; s.backgroundStarted = 100;
    s.backgroundCommand = "build --wait"; s.scheduledJobs = 1;
    QCOMPARE(SessionPresentation::status(s), QString("Background"));
    QVERIFY(SessionPresentation::working(s)); QVERIFY(SessionPresentation::active(s));
    QVERIFY(!SessionPresentation::scheduled(s)); QVERIFY(!s.needsAction());
    QVERIFY(SessionPresentation::currentAction(s).contains("build --wait"));
    QVERIFY(!SessionPresentation::working(s, false)); QVERIFY(!SessionPresentation::active(s, false));
    QCOMPARE(SessionPresentation::status(s, false), QString("Offline"));
    s.phase = "approval";
    QCOMPARE(SessionPresentation::status(s), QString("Needs approval"));
    QVERIFY(!SessionPresentation::working(s));
    s.phase = "working"; s.activity = "busy";
    QCOMPARE(SessionPresentation::status(s), QString("Working"));
    s.phase = "idle"; s.activity = "idle"; s.backgroundProcesses = 0;
    QVERIFY(SessionPresentation::scheduled(s));
    s.scheduledJobs = 0;
    QCOMPARE(SessionPresentation::status(s), QString("Ready"));
    s.backgroundProcesses = -1;
    QVERIFY(!SessionPresentation::working(s));
}

void TestFleetState::processInspectionUpdatesWithoutNewHookAndRejectsOtherRuns()
{
    auto s = shared(); s.lastEventAt = 100; s.backgroundObservedAt = 100;
    QJsonObject inspection{{"tracked", true}, {"run_id", s.runId}, {"conversation_id", s.conversationId},
        {"last_event_at", 100}, {"process_work", QJsonObject{{"count", 1}, {"sampled_at", 101}, {"command", "build"}}}};
    auto current = SessionPresentation::inspected(s, inspection);
    QCOMPARE(current.backgroundProcesses, 1);
    QCOMPARE(current.lastEventAt, 100.0);
    inspection["run_id"] = "another-run";
    QCOMPARE(SessionPresentation::inspected(s, inspection).backgroundProcesses, -1);
    inspection["run_id"] = s.runId;
    inspection["process_work"] = QJsonObject{{"count", 0}, {"sampled_at", 99}};
    QCOMPARE(SessionPresentation::inspected(current, inspection).backgroundProcesses, 1);
}

void TestFleetState::attentionMarksFollowConversationAndRejectStaleActions()
{
    auto local = box("arch", true, 1); auto &s = local.sessions[0];
    s.state = "running"; s.cmd = "claude"; s.phase = "approval";
    s.runId = "run1"; s.conversationId = "conversation1"; s.created = 100; s.lastEventAt = 101;
    FleetState f; f.setLocal(local, 1000);
    QVERIFY(f.setReviewLater({}, s, true)); QVERIFY(f.local().sessions[0].reviewLater);
    const auto marks = f.attentionMarks();
    FleetState restart; restart.setAttentionMarks(marks); restart.setLocal(local, 1000);
    QVERIFY(restart.local().sessions[0].reviewLater);
    QVERIFY(restart.markSessionRead({}, s)); QVERIFY(restart.local().sessions[0].needsAction());
    QVERIFY(!restart.local().sessions[0].needsAttention());
    restart.setLocal(local, 2000); QVERIFY(!restart.local().sessions[0].needsAttention());
    const auto old = s; s.lastEventAt = 102; restart.setLocal(local, 3000);
    QVERIFY(restart.local().sessions[0].needsAttention()); QVERIFY(!restart.markSessionRead({}, old));
    QVERIFY(restart.setReviewLater({}, s, true));
    s.name = "claude/renamed"; s.runId = "resumed"; s.created = 200; restart.setLocal(local, 4000);
    QVERIFY(restart.local().sessions[0].reviewLater); QVERIFY(!restart.markSessionRead({}, old));
    s.conversationId = "different"; restart.setLocal(local, 5000); QVERIFY(!restart.local().sessions[0].reviewLater);
    auto remote = local; remote.host = "mac"; restart.setPeer(remote, 5000);
    QVERIFY(restart.setReviewLater("mac", s, true));
    restart.setPeer(box("mac", false, 0), 6000);
    QVERIFY(restart.peer("mac")->sessions[0].reviewLater);
    QVERIFY(restart.attentionSessions(6000 + FleetState::kPeerStaleMs) >= 1);
}

void TestFleetState::attentionMarksIgnoreRoutineEventsButKeepNewRequestsUnread()
{
    auto local = box("arch", true, 1); auto &s = local.sessions[0];
    s.state = "running"; s.cmd = "claude"; s.phase = "input";
    s.runId = "run"; s.conversationId = "conversation"; s.created = 100;
    s.attentionId = "question-1"; s.replyId = "reply-1"; s.lastEventAt = 101;
    s.subagents = {{"worker", QJsonObject{{"state", "working"}, {"summary", "First tool"}}},
        {"waiting", QJsonObject{{"state", "input"}, {"question_id", "child-question-1"}, {"last_event_at", 101}}}};
    FleetState state; state.setLocal(local, 1000); const auto opened = s;
    s.lastEventAt = 102;
    auto worker = s.subagents["worker"].toObject(); worker["summary"] = "Next tool"; s.subagents["worker"] = worker;
    auto child = s.subagents["waiting"].toObject(); child["last_event_at"] = 102; s.subagents["waiting"] = child;
    state.setLocal(local, 2000);
    QVERIFY(state.markSessionRead({}, opened)); QVERIFY(!state.local().sessions[0].needsAttention());
    s.lastEventAt = 103; state.setLocal(local, 3000);
    QVERIFY(!state.local().sessions[0].needsAttention());

    s.attentionId = "question-2"; state.setLocal(local, 4000);
    QVERIFY(state.local().sessions[0].needsAttention()); QVERIFY(!state.markSessionRead({}, opened));
    QVERIFY(state.markSessionRead({}, s));
    const auto beforeChild = s; child["question_id"] = "child-question-2"; s.subagents["waiting"] = child;
    state.setLocal(local, 5000);
    QVERIFY(!state.local().sessions[0].attentionAcknowledged); QVERIFY(!state.markSessionRead({}, beforeChild));
    QVERIFY(state.markSessionRead({}, s));
    const auto beforeReply = s; s.replyId = "reply-2"; state.setLocal(local, 6000);
    QVERIFY(state.local().sessions[0].unreadReply); QVERIFY(!state.markSessionRead({}, beforeReply));
    const auto beforeRun = s; s.runId = "another-run"; state.setLocal(local, 7000);
    QVERIFY(!state.markSessionRead({}, beforeRun));
}

void TestFleetState::sumsAcrossBoxes()
{
    FleetState f;
    f.setLocal(box(QStringLiteral("arch"), true, 4), 1000);
    f.setPeer(box(QStringLiteral("mac"), true, 2), 1000);
    QCOMPARE(f.totalSessions(), 6);
}

void TestFleetState::archivesDoNotCountAsActiveFleet()
{
    auto local = box("arch", true, 2); local.sessions[1].state = "archived";
    auto peer = box("mac", true, 3); peer.sessions[1].state = "archived"; peer.sessions[2].state = "paused";
    FleetState fleet; fleet.setLocal(local, 1000); fleet.setPeer(peer, 1000);
    QCOMPARE(fleet.totalSessions(), 3);
    QVERIFY(fleet.tooltip(1000).contains("1 here"));
    QVERIFY(fleet.tooltip(1000).contains("2 on mac"));
    fleet.setPeer(box("mac", false, 0), 2000);
    QCOMPARE(fleet.totalSessions(), 3); // Offline retention excludes archives too.
}

void TestFleetState::peerNeverPolledIsStale()
{
    FleetState f;
    f.setLocal(box(QStringLiteral("arch"), true, 4), 1000);
    f.registerPeer(QStringLiteral("mac"));      // объявлен, но ещё не опрошен
    QCOMPARE(f.totalSessions(), 4);
    QVERIFY(f.isStale(1000));
}

void TestFleetState::freshPeerIsNotStale()
{
    FleetState f;
    f.setLocal(box(QStringLiteral("arch"), true, 4), 1000);
    f.setPeer(box(QStringLiteral("mac"), true, 2), 1000);
    QVERIFY(!f.isStale(1000 + FleetState::kPeerStaleMs - 1));
}

void TestFleetState::stalePeerStillCounts()
{
    // Ноут закрыли: сессии не умерли, они на паузе. Считаем их, но метим цифру точкой.
    FleetState f;
    f.setLocal(box(QStringLiteral("arch"), true, 4), 1000);
    f.setPeer(box(QStringLiteral("mac"), true, 2), 1000);
    const qint64 later = 1000 + FleetState::kPeerStaleMs + 1;
    QCOMPARE(f.totalSessions(), 6);
    QVERIFY(f.isStale(later));
}

void TestFleetState::localIsAlwaysFresh()
{
    FleetState f;
    f.setLocal(box(QStringLiteral("arch"), true, 4), 1000);
    QVERIFY(!f.isStale(1000 + 10 * FleetState::kPeerStaleMs));
}

void TestFleetState::peerGoingOfflineKeepsLastKnownSessions()
{
    // Ноут был на связи с 2 сессиями, потом заснул: hgs присылает ok:false с sessions:[]
    // (спросить уже некого), но реальные сессии не исчезли — цифра не должна просесть.
    FleetState f;
    f.setLocal(box(QStringLiteral("arch"), true, 4), 1000);
    f.setPeer(box(QStringLiteral("mac"), true, 2), 1000);
    QCOMPARE(f.totalSessions(), 6);

    BoxState offline = box(QStringLiteral("mac"), false, 0);
    offline.error = QStringLiteral("offline");
    f.setPeer(offline, 2000);

    QCOMPARE(f.totalSessions(), 6);        // 4 arch + 2 из последнего живого ответа mac
    QVERIFY(f.isStale(2000));              // но помечено несвежим сразу же (ok:false)
    QCOMPARE(f.peerPolledAt(QStringLiteral("mac")), qint64(2000));  // спросили и получили ответ
}

void TestFleetState::peerNeverSeenAliveCountsAsZero()
{
    // Первый же опрос застал пира спящим: сессии неизвестны (не 0 по факту, но и не
    // подтверждены), считать их как известные нельзя.
    FleetState f;
    f.setLocal(box(QStringLiteral("arch"), true, 4), 1000);
    BoxState offline = box(QStringLiteral("mac"), false, 0);
    offline.error = QStringLiteral("offline");
    f.setPeer(offline, 1000);

    QCOMPARE(f.totalSessions(), 4);
    QVERIFY(f.isStale(1000));
}

void TestFleetState::tooltipShowsPollAgeForOfflinePeer()
{
    // "unreachable" само по себе не говорит, спросили ли мы только что или час назад —
    // а это ровно то, чем "ноут спит" отличается от "потеряли контакт".
    FleetState f;
    f.setLocal(box(QStringLiteral("arch"), true, 4), 1000);
    BoxState offline = box(QStringLiteral("mac"), false, 0);
    offline.error = QStringLiteral("offline");
    f.setPeer(offline, 1000);

    const QString tip = f.tooltip(1000 + 20000);   // 20 секунд спустя
    QVERIFY2(tip.contains(QStringLiteral("unreachable")), qPrintable(tip));
    QVERIFY2(tip.contains(QStringLiteral("just now")), qPrintable(tip));
}

void TestFleetState::tooltipAgeIsMinuteGrained()
{
    // Возраст в секундах менял бы текст тултипа почти на каждом 2-секундном тике, а на
    // маке каждая такая смена -- обновление NSStatusItem (см. QuietTrayIcon.h). Вопрос
    // же, на который отвечает возраст, -- «ноут спит» или «потеряли контакт», --
    // решается минутами. «just now» держится полторы минуты: столько покрывает штатный
    // минутный опрос вместе с задержкой ssh, и здоровый пир не мигает «1m ago».
    FleetState f;
    f.setLocal(box(QStringLiteral("arch"), true, 4), 1000);
    BoxState offline = box(QStringLiteral("mac"), false, 0);
    offline.error = QStringLiteral("offline");
    f.setPeer(offline, 1000);

    QVERIFY2(f.tooltip(1000 + 89000).contains(QStringLiteral("just now")),
             qPrintable(f.tooltip(1000 + 89000)));
    QVERIFY2(f.tooltip(1000 + 90000).contains(QStringLiteral("1m ago")),
             qPrintable(f.tooltip(1000 + 90000)));
    QVERIFY2(f.tooltip(1000 + 150000).contains(QStringLiteral("2m ago")),
             qPrintable(f.tooltip(1000 + 150000)));
}

void TestFleetState::sharedReadComesFromTheSessionHost()
{
    BoxState local = box("arch", true, 0); local.sessions = {shared()};
    FleetState f; f.setReadReplies({{"[\"\",\"codex\",\"conversation-1\"]", "7:100.5"}}); // an old local mark
    f.setLocal(local, 1000);
    QVERIFY(f.local().sessions[0].unreadReply); // the host has no mark: unread on every device
    local.sessions[0].readReplyId = "7:100.5"; f.setLocal(local, 2000);
    QVERIFY(!f.local().sessions[0].unreadReply);
    auto peer = local; peer.host = "mac"; peer.sessions[0].readReplyId.clear(); f.setPeer(peer, 2000);
    QVERIFY(f.peer("mac")->sessions[0].unreadReply);
}

void TestFleetState::sharedMarksApplyAtOnceAndSettleFromSnapshots()
{
    const QString signature(64, 'a');
    BoxState local = box("arch", true, 0); local.sessions = {shared("7:100.5", signature)};
    FleetState f; f.setLocal(local, 1000);
    const auto displayed = f.local().sessions[0];
    QVERIFY(displayed.needsAttention());
    QVERIFY(f.markSessionRead({}, displayed));
    QVERIFY(!f.local().sessions[0].needsAttention());
    QVERIFY(!f.takeNewReadMarks().isEmpty()); QVERIFY(f.takeNewReadMarks().isEmpty());
    const ReadMark key{{}, displayed.name, {}, displayed.conversationId, {}, {}};
    const auto queued = ReadMark::fromJson(f.pendingReads().value(key.key()).toObject());
    QCOMPARE(queued.runId, QString("run-1")); QCOMPARE(queued.replyId, QString("7:100.5"));
    QCOMPARE(queued.attentionSignature, signature);
    QCOMPARE(queued.payload(), (QJsonObject{{"run_id", "run-1"}, {"conversation_id", "conversation-1"},
        {"reply_id", "7:100.5"}, {"attention_signature", signature}}));
    // The host has not stored it yet: the queued mark keeps the session read.
    f.setLocal(local, 2000); QVERIFY(!f.local().sessions[0].needsAttention()); QVERIFY(f.takeSettledReadMarks().isEmpty());
    FleetState restarted; restarted.setPendingReads(f.pendingReads()); restarted.setLocal(local, 3000);
    QVERIFY(!restarted.local().sessions[0].needsAttention());
    // Once the host shows the mark, the queue entry is finished.
    local.sessions[0].readReplyId = "7:100.5"; local.sessions[0].readAttentionSignature = signature; f.setLocal(local, 4000);
    QCOMPARE(f.takeSettledReadMarks().size(), 1); QVERIFY(f.pendingReads().isEmpty());
    QVERIFY(!f.local().sessions[0].needsAttention());
}

void TestFleetState::unknownOptionalQuestionsKeepAttentionMarks()
{
    // optional_questions is null both for an index still catching up and for
    // one that cannot be read, whose signature hgs read acknowledges. The
    // desktop cannot tell them apart and never drops the acknowledgement.
    const QString signature(64, 'a');
    BoxState local = box("arch", true, 0); local.sessions = {shared("7:100.5", signature)};
    local.sessions[0].optionalQuestionsReported = true; local.sessions[0].optionalQuestions = -1;
    FleetState f; f.setLocal(local, 1000);
    QVERIFY(f.markSessionRead({}, f.local().sessions[0]));
    const ReadMark key{{}, local.sessions[0].name, {}, local.sessions[0].conversationId, {}, {}};
    QCOMPARE(ReadMark::fromJson(f.pendingReads().value(key.key()).toObject()).attentionSignature, signature);
    QVERIFY(f.local().sessions[0].attentionAcknowledged);
    local.sessions[0].readReplyId = "7:100.5"; local.sessions[0].readAttentionSignature = signature; f.setLocal(local, 2000);
    QCOMPARE(f.takeSettledReadMarks().size(), 1); QVERIFY(f.local().sessions[0].attentionAcknowledged);
}

void TestFleetState::queuedMarksNeverHideNewerRepliesOrRequests()
{
    const QString first(64, 'a'), second(64, 'b');
    BoxState local = box("arch", true, 0); local.sessions = {shared("7:100.5", first)};
    FleetState f; f.setLocal(local, 1000); const auto displayed = f.local().sessions[0];
    // A newer request arrived after the user opened the menu.
    local.sessions[0].attentionSignature = second; f.setLocal(local, 1500);
    QVERIFY(!f.markSessionRead({}, displayed)); QVERIFY(f.takeNewReadMarks().isEmpty());
    local.sessions[0].attentionSignature = first; f.setLocal(local, 1600);
    QVERIFY(f.markSessionRead({}, f.local().sessions[0]));
    local.sessions[0].replyId = "8:200.5"; local.sessions[0].attentionSignature = second; local.sessions[0].attentionId = "call-2";
    f.setLocal(local, 2000);
    QVERIFY(f.local().sessions[0].unreadReply); QVERIFY(!f.local().sessions[0].attentionAcknowledged);
    QCOMPARE(f.takeSettledReadMarks().size(), 1); // the host would report both fields stale
    // Marks for a session the host no longer lists are finished too.
    f.setLocal(local, 2100); QVERIFY(f.markReplyRead({}, local.sessions[0].name, "conversation-1", "8:200.5"));
    local.sessions.clear(); f.setLocal(local, 2200);
    QCOMPARE(f.takeSettledReadMarks().size(), 1); QVERIFY(f.pendingReads().isEmpty());
}

void TestFleetState::legacyMarksMigrateOnlyCurrentValues()
{
    const QString signature(64, 'a');
    BoxState local = box("arch", true, 0); local.sessions = {shared("7:100.5", signature)};
    auto other = shared("3:30.5"); other.name = "codex/project/old"; other.conversationId = "conversation-2";
    local.sessions << other;
    auto unshared = local; for (auto &s : unshared.sessions) s.sharedRead = false;
    unshared.sessions[1].replyId = "2:20.5"; // the old GUI read an earlier reply
    FleetState before; before.setLocal(unshared, 1000);
    QVERIFY(before.markSessionRead({}, before.local().sessions[0]));
    QVERIFY(before.markReplyRead({}, other.name, other.conversationId, "2:20.5"));
    QVERIFY(before.takeNewReadMarks().isEmpty()); // older hosts never get shared marks
    FleetState f; f.setReadReplies(before.readReplies()); f.setAttentionMarks(before.attentionMarks()); f.setLocal(local, 2000);
    QVERIFY(f.supportsSharedRead({})); QVERIFY(!f.supportsSharedRead("mac"));
    const auto marks = f.legacyReadMarks({});
    QCOMPARE(marks.size(), 1);
    QCOMPARE(marks[0].name, local.sessions[0].name); QCOMPARE(marks[0].runId, QString("run-1"));
    QCOMPARE(marks[0].replyId, QString("7:100.5")); QCOMPARE(marks[0].attentionSignature, signature);
}

void TestFleetState::olderHostsKeepLocalReadStateAndQueueNothing()
{
    BoxState local = box("arch", true, 0); auto s = shared(); s.sharedRead = false; local.sessions = {s};
    FleetState f; f.setLocal(local, 1000); QVERIFY(f.local().sessions[0].unreadReply);
    QVERIFY(f.markReplyRead({}, s.name, s.conversationId, s.replyId));
    QVERIFY(!f.local().sessions[0].unreadReply);
    QVERIFY(f.takeNewReadMarks().isEmpty()); QVERIFY(f.pendingReads().isEmpty());
    QVERIFY(!f.supportsSharedRead({}));
}

void TestFleetState::recoveryAttentionWithoutSignatureStaysLocal()
{
    BoxState local = box("arch", true, 0); auto s = shared(); s.phase = "working"; s.recovery = {{"state", "blocked"}};
    local.sessions = {s}; FleetState f; f.setLocal(local, 1000);
    QVERIFY(f.local().sessions[0].needsAction());
    QVERIFY(f.markSessionRead({}, f.local().sessions[0]));
    QVERIFY(f.local().sessions[0].attentionAcknowledged);
    f.setLocal(local, 2000); QVERIFY(f.local().sessions[0].attentionAcknowledged);
}

void TestFleetState::controlOnlyComputerHasNoLocalCount()
{
    BoxState local; local.host = "windows-pc"; local.ok = true; local.localSessions = false;
    BoxState mac; mac.host = "mac"; mac.ok = true; SessionInfo s; s.name = "codex/p"; mac.sessions = {s};
    FleetState state; state.setLocal(local, 1000); state.registerPeer("mac"); state.setPeer(mac, 1000);
    const auto lines = state.tooltip(1000).split('\n');
    QCOMPARE(lines.size(), 2);
    QCOMPARE(lines[0], QString("0 need attention, 1 session total"));
    QCOMPARE(lines[1], QString("1 on mac (just now)"));
    local.localSessions = true; state.setLocal(local, 1000);
    QVERIFY(state.tooltip(1000).contains("0 here"));
}

void TestFleetState::tooltipCountsUseSingularForOne()
{
    BoxState local = box("arch", true, 0);
    SessionInfo waiting = shared("7:100.5", "approval-1"); waiting.name = "codex/a";
    local.sessions = {waiting};
    FleetState state; state.setLocal(local, 1000);
    QCOMPARE(state.attentionSessions(1000), 1);
    QCOMPARE(state.tooltip(1000).section('\n', 0, 0), QString("1 needs attention, 1 session total"));
    SessionInfo other; other.name = "codex/b";
    local.sessions = {waiting, other}; state.setLocal(local, 2000);
    QCOMPARE(state.tooltip(2000).section('\n', 0, 0), QString("1 needs attention, 2 sessions total"));
}

namespace {
QString key(const QJsonArray &parts) { return QString::fromUtf8(QJsonDocument(parts).toJson(QJsonDocument::Compact)); }
// Everything the read inputs decide, as one comparable text.
QString readOutcome(const FleetState &f, qint64 now)
{
    QStringList lines;
    const auto rows = [&lines](const QString &host, const BoxState &b) {
        for (const auto &s : b.sessions)
            lines << QStringLiteral("%1/%2 unread=%3 acknowledged=%4 review=%5 attention=%6")
                         .arg(host, s.name).arg(s.unreadReply).arg(s.attentionAcknowledged).arg(s.reviewLater).arg(s.needsAttention());
    };
    rows({}, f.local());
    for (const auto &peer : f.peerNames()) rows(peer, *f.peer(peer));
    for (const auto &object : {f.readReplies(), f.attentionMarks(), f.pendingReads(), f.unreadReplies()})
        lines << QString::fromUtf8(QJsonDocument(object).toJson(QJsonDocument::Compact));
    lines << QString::number(f.attentionSessions(now));
    return lines.join('\n');
}
// The stored queue after the caller removes the settled marks (ReadMarkStore::remove).
QJsonObject afterRemoval(QJsonObject stored, const QList<ReadMark> &settled)
{
    for (const auto &mark : settled)
        if (ReadMark::fromJson(stored.value(mark.key()).toObject()) == mark) stored.remove(mark.key());
    return stored;
}
}

void TestFleetState::unchangedListingRefreshesOnlyObservationFields()
{
    BoxState local = box("arch", true, 0); local.digest = "listing-1"; local.metrics = {{"cpu_percent", 5}};
    auto s = shared(); s.backgroundObservedAt = 10; local.sessions = {s};
    FleetState fleet; QVERIFY(!fleet.refreshListing({}, local, 0)); // nothing listed yet
    fleet.setLocal(local, 0);
    QVERIFY(fleet.local().sessions[0].unreadReply);
    QVERIFY(fleet.markReplyRead({}, s.name, s.conversationId, s.replyId));
    const auto pending = fleet.pendingReads(), read = fleet.readReplies(); QVERIFY(!pending.isEmpty());
    // The same listing, observed later: new metrics and process sample time.
    auto later = local; later.metrics = {{"cpu_percent", 50}}; later.sessions[0].backgroundObservedAt = 12;
    QVERIFY(fleet.refreshListing({}, later, 100));
    QCOMPARE(fleet.local().metrics.value("cpu_percent").toInt(), 50);
    QCOMPARE(fleet.local().sessions[0].backgroundObservedAt, 12.0);
    QVERIFY(!fleet.local().sessions[0].unreadReply);
    QCOMPARE(fleet.pendingReads(), pending); QCOMPARE(fleet.readReplies(), read);
    QVERIFY(fleet.takeSettledReadMarks().isEmpty());
    // Any other listing is applied in full by the caller; refresh changes nothing.
    auto changed = later; changed.digest = "listing-2"; changed.sessions[0].backgroundObservedAt = 14;
    QVERIFY(!fleet.refreshListing({}, changed, 200));
    auto unprepared = later; unprepared.digest.clear(); unprepared.sessions[0].backgroundObservedAt = 14;
    QVERIFY(!fleet.refreshListing({}, unprepared, 200));
    QCOMPARE(fleet.local().sessions[0].backgroundObservedAt, 12.0);
    // A confirmed preference write changes the row: the next listing is applied in full.
    const QJsonObject suppressed{{"reply_id", QJsonValue()}, {"attention_signature", QJsonValue()},
        {"optional_question_ids", QJsonArray()}, {"main_attention", QJsonValue()}, {"child_attention", QJsonObject()}};
    const QJsonObject confirmed{{"conversation_id", s.conversationId}, {"muted", false}, {"revision", 0}, {"suppressed", suppressed}};
    auto preferred = local; preferred.sessions[0].sharedNotifications = true; preferred.sessions[0].notificationPreferences = confirmed;
    fleet.setLocal(preferred, 300); QVERIFY(fleet.refreshListing({}, preferred, 350));
    auto muted = confirmed; muted["muted"] = true; muted["revision"] = 1;
    QVERIFY(fleet.applyNotificationPreferences({}, fleet.local().sessions[0], muted));
    QVERIFY(!fleet.refreshListing({}, preferred, 400));
}

void TestFleetState::refreshedPeersStayFreshAndChangedListingsApplyInFull()
{
    BoxState local = box("arch", true, 0); local.peersKnown = true; local.peers = {"mac"}; local.digest = "arch";
    FleetState fleet; fleet.setLocal(local, 0);
    BoxState mac = box("mac", true, 2); mac.digest = "mac-1"; mac.sessions[0].phase = "approval"; mac.sessions[0].state = "running";
    QVERIFY(!fleet.refreshListing("mac", mac, 1000)); // never polled
    fleet.setPeer(mac, 1000);
    const qint64 late = 1000 + FleetState::kPeerStaleMs + 1;
    QVERIFY(fleet.isStale(late)); QCOMPARE(fleet.attentionSessions(late), 0);
    QVERIFY(fleet.refreshListing("mac", mac, late - 10));
    QCOMPARE(fleet.peerPolledAt("mac"), late - 10);
    QVERIFY(!fleet.isStale(late)); QCOMPARE(fleet.attentionSessions(late), 1);
    QVERIFY(fleet.tooltip(late).contains("2 on mac (just now)"));
    QVERIFY(fleet.freshness(late) != fleet.freshness(late + FleetState::kPeerStaleMs));
    // A real change is not a refresh.
    auto news = mac; news.digest = "mac-2"; news.sessions[0].phase = "idle";
    QVERIFY(!fleet.refreshListing("mac", news, late));
    fleet.setPeer(news, late); QCOMPARE(fleet.attentionSessions(late), 0);
    // A machine outside the configured list is ignored, as setPeer ignores it.
    BoxState air = box("air", true, 1); air.digest = "air";
    QVERIFY(!fleet.refreshListing("air", air, late)); QVERIFY(!fleet.peer("air"));
    // The same offline answer again refreshes the poll time; the last rows stay.
    BoxState offline; offline.host = "mac"; offline.error = "offline"; offline.digest = "mac-offline";
    fleet.setPeer(offline, late + 1);
    QVERIFY(fleet.refreshListing("mac", offline, late + 2));
    QCOMPARE(fleet.peerPolledAt("mac"), late + 2); QVERIFY(!fleet.peer("mac")->ok); QCOMPARE(fleet.peer("mac")->sessions.size(), 2);
    // A failed read the caller records (no listing, no digest) makes the next one apply in full.
    BoxState failed; failed.host = "mac"; failed.error = "timed out";
    fleet.setPeer(failed, late + 3);
    QVERIFY(!fleet.refreshListing("mac", offline, late + 4)); QCOMPARE(fleet.peerPolledAt("mac"), late + 3);
}

void TestFleetState::viewCopiesTakeRefreshedObservations()
{
    BoxState local = box("arch", true, 0); local.peersKnown = true; local.peers = {"mac"}; local.digest = "arch";
    auto s = shared(); local.sessions = {s};
    BoxState mac = box("mac", true, 1); mac.digest = "mac"; mac.metrics = {{"load_1", 1.0}};
    FleetState source; source.setLocal(local, 0); source.setPeer(mac, 1000);
    // A view's copy with its own read state, as SessionsWindow keeps one.
    FleetState view = source;
    QVERIFY(view.markReplyRead({}, s.name, s.conversationId, s.replyId));
    auto later = local; later.sessions[0].backgroundObservedAt = 20; later.metrics = {{"cpu_percent", 9}};
    auto macLater = mac; macLater.metrics = {{"load_1", 2.0}};
    QVERIFY(source.refreshListing({}, later, 5000)); QVERIFY(source.refreshListing("mac", macLater, 6000));
    QVERIFY(view.refreshObservations(source));
    QCOMPARE(view.peerPolledAt("mac"), qint64(6000));
    QCOMPARE(view.peer("mac")->metrics.value("load_1").toDouble(), 2.0);
    QCOMPARE(view.local().metrics.value("cpu_percent").toInt(), 9);
    QCOMPARE(view.local().sessions[0].backgroundObservedAt, 20.0);
    QVERIFY(!view.local().sessions[0].unreadReply); QVERIFY(source.local().sessions[0].unreadReply);
    QCOMPARE(view.freshness(6000), source.freshness(6000));
    // A listing applied in full is no refresh of the view's copy.
    source.setPeer(macLater, 7000);
    QVERIFY(!view.refreshObservations(source)); QCOMPARE(view.peerPolledAt("mac"), qint64(6000));
    view = source; source.setLocal(later, 8000);
    QVERIFY(!view.refreshObservations(source));
}

void TestFleetState::combinedReadStateMatchesSeparatePasses()
{
    const QString signature(64, 'a'), newer(64, 'b');
    // This computer: shared-read rows (read on the host, queued, superseded by a
    // listing that shows it read) and an older host's rows (local reply marks, an
    // acknowledgement stored under the fallback key, review later).
    auto queued = shared("5:50.5"); queued.name = "codex/project/queued"; queued.conversationId = "c-queued";
    auto shown = shared("6:60.5", signature); shown.name = "codex/project/shown"; shown.conversationId = "c-shown";
    shown.readReplyId = "6:60.5"; shown.readAttentionSignature = signature;
    auto moved = shared("8:80.5", newer); moved.name = "codex/project/moved"; moved.conversationId = "c-moved";
    auto legacy = shared("2:20.5"); legacy.sharedRead = false; legacy.name = "claude/project/legacy"; legacy.cmd = "claude"; legacy.conversationId = "c-legacy";
    auto asking = shared(); asking.sharedRead = false; asking.name = "claude/project/asking"; asking.cmd = "claude";
    asking.conversationId = "c-asking"; asking.phase = "approval"; asking.attentionId = "call-9"; asking.lastEventAt = 9;
    auto later = shared("4:40.5"); later.sharedRead = false; later.name = "claude/project/later"; later.cmd = "claude"; later.conversationId = "c-later";
    BoxState before = box("arch", true, 0); before.peersKnown = true; before.peers = {"mac", "air"};
    before.sessions = {queued, shown, moved, legacy, asking, later};
    // The poll's listing: the host now shows one queued mark.
    BoxState local = before; local.sessions[2].readReplyId = "8:80.5"; local.sessions[2].readAttentionSignature = newer;
    // A reachable peer whose host no longer lists a queued session, and an offline one.
    auto peerRow = shared("1:10.5"); peerRow.name = "codex/remote/one"; peerRow.conversationId = "c-remote";
    BoxState mac = box("mac", true, 0); mac.sessions = {peerRow};
    BoxState air = box("air", false, 0); air.error = "offline";

    const auto mark = [](const QString &host, const SessionInfo &s, const QString &reply, const QString &attention = {}) {
        return ReadMark{host, s.name, s.runId, s.conversationId, reply, attention};
    };
    QJsonObject oldPending;
    for (const auto &m : {mark({}, queued, "5:50.5"), mark({}, shown, "6:60.5", signature), mark({}, moved, "8:80.5", newer),
                          mark("mac", peerRow, "1:10.5")})
        oldPending[m.key()] = m.toJson();
    auto pending = oldPending;
    const auto gone = ReadMark{"mac", "codex/remote/gone", "run-1", "c-gone", "3:30.5", {}};
    const auto offline = ReadMark{"air", "codex/remote/off", "run-1", "c-off", "3:30.5", {}};
    pending[gone.key()] = gone.toJson(); pending[offline.key()] = offline.toJson();
    pending.remove(mark("mac", peerRow, {}).key());

    FleetState probe; probe.setLocal(before, 0);
    QVERIFY(probe.markSessionRead({}, probe.local().sessions[4]));
    const auto acknowledged = probe.attentionMarks().value(key({"", "claude", "c-asking"})).toObject().value("acknowledged").toString();
    QVERIFY(!acknowledged.isEmpty());
    const QJsonObject oldReplies{{key({"", "claude", "c-later"}), "4:40.5"}};
    const QJsonObject replies{{key({"", "claude", "c-legacy"}), "2:20.5"}, {key({"", "claude", "c-later"}), "4:40.5"}};
    const QJsonObject oldMarks{{key({"", "claude", "claude/project/later", 100}), QJsonObject{{"review", true}}}};
    const QJsonObject marks{{key({"", "claude", "claude/project/asking", 100}), QJsonObject{{"acknowledged", acknowledged}}},
                            {key({"", "claude", "c-later"}), QJsonObject{{"review", true}}}};

    const auto initial = [&] {
        FleetState f; f.setReadReplies(oldReplies); f.setAttentionMarks(oldMarks); f.setPendingReads(oldPending);
        f.setLocal(before, 1000); f.setPeer(mac, 1000); f.setPeer(air, 1000);
        f.takeSettledReadMarks(); f.takeNewReadMarks();
        return f;
    };
    FleetState separate = initial(), combined = initial();
    QCOMPARE(readOutcome(combined, 1000), readOutcome(separate, 1000));
    QVERIFY(combined.pendingReads().contains(mark({}, moved, {}).key()));
    // The read inputs of a poll, then its listing and the reloaded queue (TrayAgent).
    separate.setReadReplies(replies); separate.setAttentionMarks(marks); separate.setPendingReads(pending);
    combined.setReadState(replies, marks, pending);
    QCOMPARE(readOutcome(combined, 2000), readOutcome(separate, 2000));
    for (auto *f : {&separate, &combined}) { f->setLocal(local, 2000); f->setPendingReads(pending); }
    QCOMPARE(readOutcome(combined, 2000), readOutcome(separate, 2000));
    const auto separateSettled = separate.takeSettledReadMarks(), combinedSettled = combined.takeSettledReadMarks();
    QCOMPARE(afterRemoval(pending, combinedSettled), afterRemoval(pending, separateSettled));
    for (const auto &settled : combinedSettled) QVERIFY(separateSettled.contains(settled));
    // The fixture covers what the passes decide: marks settled by either
    // listing, dropped and kept, a queued read, a local acknowledgement under
    // its fallback key, local reply marks and review later.
    const auto remaining = afterRemoval(pending, combinedSettled);
    QVERIFY(!remaining.contains(mark({}, shown, {}).key())); QVERIFY(!remaining.contains(mark({}, moved, {}).key()));
    QVERIFY(!remaining.contains(gone.key())); QVERIFY(remaining.contains(offline.key()));
    QVERIFY(remaining.contains(mark({}, queued, {}).key()));
    QVERIFY(!combined.local().sessions[0].unreadReply); QVERIFY(!combined.local().sessions[2].unreadReply);
    QVERIFY(combined.local().sessions[4].needsAction()); QVERIFY(combined.local().sessions[4].attentionAcknowledged);
    QVERIFY(combined.local().sessions[5].reviewLater); QVERIFY(!combined.local().sessions[3].unreadReply);
    QVERIFY(combined.peer("mac")->sessions[0].unreadReply);
    QVERIFY(combined.attentionMarks().contains(key({"", "claude", "c-asking"})));
}

QTEST_APPLESS_MAIN(TestFleetState)
#include "test_fleetstate.moc"
