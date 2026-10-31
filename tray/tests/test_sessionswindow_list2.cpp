// U62 (#712): The session list: rows, states, filters, selection and menus, from test_sessionswindow.
#include "SessionsWindowFixture.h"

class TestSessionsWindow : public SessionsWindowFixture {
    Q_OBJECT
private slots:
    void rebuildKeepsProjectOrderAtScale_data();
    void rebuildKeepsProjectOrderAtScale();
};

void TestSessionsWindow::rebuildKeepsProjectOrderAtScale_data()
{
    QTest::addColumn<int>("count");
    QTest::addColumn<bool>("storedOrder");
    for (int count : {400, 3000, 6000}) {
        QTest::newRow(qPrintable(QString("observed-%1").arg(count))) << count << false;
        QTest::newRow(qPrintable(QString("stored-%1").arg(count))) << count << true;
    }
}

void TestSessionsWindow::rebuildKeepsProjectOrderAtScale()
{
    QFETCH(int, count);
    QFETCH(bool, storedOrder);
    if (count > 400 && qEnvironmentVariableIsSet("HGS_SKIP_QT_SCALE_ROWS"))
        QSKIP("zerus-5wmh.79: hosted macOS CI skips the 3000 and 6000 scale rows");
    BoxState box; box.host = "arch"; box.ok = true;
    box.metrics = QJsonObject{{"platform", "linux"}};
    QStringList names, identities;
    for (int i = 0; i < count; ++i) {
        SessionInfo session; session.name = QString("codex/scaling/session-%1").arg(i, 5, 10, QChar('0'));
        session.cmd = "codex"; session.project = "scaling"; session.tag = QString::number(i);
        session.state = "running"; session.activity = "idle"; session.phase = "idle"; session.tracked = true;
        session.muted = i % 2;
        names.append(session.name); identities.append(box.host + '\n' + session.name); box.sessions.append(session);
    }
    auto ordered = identities;
    if (storedOrder) std::reverse(ordered.begin(), ordered.end());
    SessionOrganization organization(QJsonObject{{"projects", QJsonArray{QJsonObject{
        {"id", "ungrouped"}, {"name", "General"},
        {"sessions", storedOrder ? QJsonArray::fromStringList(ordered) : QJsonArray{}}}}}});
    organization.markImported(box.host);
    setJsonSetting("workspace/organization", QJsonDocument(organization.toJson()).toJson(QJsonDocument::Compact));
    // The large saved layout is parsed off the GUI thread first, as at desktop startup (SettingsJson::preload).
    QObject warming; bool warmed = false;
    SettingsJson::warm(&warming, {QStringLiteral("workspace/organization")});
    ProcessRunner::post(&warming, [] { return true; }, [&warmed](bool) { warmed = true; });
    QTRY_VERIFY(warmed);
    FleetState state; state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program()); keepDetailsPollsOut(window); window.show();
    QElapsedTimer elapsed; elapsed.start(); window.setFleet(state);
    const auto first = elapsed.nsecsElapsed();
    auto *list = window.findChild<SessionList *>("sessionList"); QVERIFY(list);
    QCOMPARE(list->count(), count);
    for (int i = 0; i < count; ++i) {
        QCOMPARE(list->item(i)->data(SessionRoles::Identity).toString(), ordered[i]);
        const auto source = storedOrder ? count - 1 - i : i;
        QCOMPARE(list->item(i)->data(SessionRoles::Muted).toBool(), bool(source % 2));
    }
    window.showSession({}, names.last());
    QCOMPARE(list->currentItem()->data(SessionRoles::Identity).toString(), identities.last());
    elapsed.restart(); window.setFleet(state);
    const auto refresh = elapsed.nsecsElapsed();
    QCOMPARE(list->currentItem()->data(SessionRoles::Identity).toString(), identities.last());
    QCOMPARE(list->count(), count);
    for (int i = 0; i < count; ++i) QCOMPARE(list->item(i)->data(SessionRoles::Identity).toString(), ordered[i]);
    qInfo().nospace() << "REBUILD_SCALING count=" << count << " stored=" << storedOrder
                     << " first_ns=" << first << " refresh_ns=" << refresh;
    // Timings are observations for an identical base/fixed campaign, never a
    // wall-clock assertion. The row count, order, Mute and selection are exact.
}

QTEST_MAIN(TestSessionsWindow)
#include "test_sessionswindow_list2.moc"
