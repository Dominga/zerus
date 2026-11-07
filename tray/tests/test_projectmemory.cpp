#include <QtTest>
#include <QCheckBox>
#include <QLabel>
#include <QPushButton>
#include "ProjectMemoryPanel.h"

// Task 126: project memory on Projects, over a fake `hgs project config`.
class TestProjectMemory : public QObject {
    Q_OBJECT
    static QJsonObject view(bool enabled, qint64 records, qint64 revision, const QString &store = "/state/lore/projects/p-1")
    {
        return {{"project", "p-1"}, {"enabled", enabled}, {"store", store}, {"default_store", "/state/lore/projects/p-1"},
                {"default", store == "/state/lore/projects/p-1"}, {"revision", revision}, {"moved_from", QJsonArray{}},
                {"status", QJsonObject{{"exists", records > 0 || enabled}, {"records", records}, {"files", 2}, {"bytes", 3 * 1024 * 1024}, {"index", "stale"}}}};
    }
private slots:
    void statusNamesRecordsSizeAndIndex()
    {
        QCOMPARE(ProjectMemory::statusText(view(true, 12, 1)), QString("12 records, 3.0 MiB, index out of date"));
        QVERIFY(ProjectMemory::statusText(view(false, 0, 0)).startsWith("No store yet"));
        QCOMPARE(ProjectMemory::sizeText(512), QString("512 B"));
        QCOMPARE(ProjectMemory::viewOf({{"status", "moved"}, {"view", view(true, 1, 2)}}).value("revision").toInteger(), 2);
    }
    void theSwitchWritesAtTheRevisionItRead()
    {
        QList<QPair<QString, QJsonObject>> calls; QJsonObject state = view(false, 0, 0);
        ProjectMemoryPanel panel;
        panel.setBackend([&](const QString &, const QString &op, const QJsonObject &input, IronRules::Reply done) {
            calls.append({op, input});
            if (op == "set") { state = view(input.value("enabled").toBool(), 0, state.value("revision").toInteger() + 1); done({{"status", "updated"}, {"view", state}}, {}); return; }
            done(state, {});
        });
        panel.setProject("p-1");
        QCOMPARE(calls.size(), 1); QCOMPARE(calls[0].first, QString("get")); QCOMPARE(calls[0].second.value("project").toString(), QString("p-1"));
        auto *toggle = panel.findChild<QCheckBox *>("projectMemorySwitch"); QVERIFY(toggle); QVERIFY(!toggle->isChecked());
        toggle->click();
        QCOMPARE(calls.size(), 2); QCOMPARE(calls[1].first, QString("set"));
        QCOMPARE(calls[1].second.value("enabled").toBool(), true); QCOMPARE(calls[1].second.value("expected_revision").toInteger(), 0);
        QVERIFY(!calls[1].second.value("request_id").toString().isEmpty());
        QVERIFY(toggle->isChecked()); QCOMPARE(panel.view().value("revision").toInteger(), 1);
    }
    void browseRepointsAnEmptyStoreAndMovesOneWithRecordsAfterConfirming()
    {
        QList<QPair<QString, QJsonObject>> calls; QJsonObject state = view(true, 0, 3);
        ProjectMemoryPanel panel;
        bool confirm = false; int asked = 0;
        panel.pickFolder = [](const QString &) { return QString("/srv/memory"); };
        panel.confirmMove = [&](const QString &from, const QString &to, qint64 records) { ++asked; return from == "/state/lore/projects/p-1" && to == "/srv/memory" && records == 4 && confirm; };
        panel.setBackend([&](const QString &, const QString &op, const QJsonObject &input, IronRules::Reply done) {
            calls.append({op, input});
            if (op == "move") { state = view(true, 4, 5, input.value("to").toString());
                done({{"status", "moved"}, {"from", "/state/lore/projects/p-1"}, {"to", "/srv/memory"}, {"files", 3}, {"bytes", 2048}, {"old_kept", true}, {"view", state}}, {}); return; }
            if (op == "set") { state = view(true, 0, 4, input.value("store").toString()); done({{"status", "updated"}, {"view", state}}, {}); return; }
            done(state, {});
        });
        panel.setProject("p-1");
        panel.browse();
        QCOMPARE(calls.last().first, QString("set")); QCOMPARE(calls.last().second.value("store").toString(), QString("/srv/memory"));
        QCOMPARE(asked, 0);
        // With records: the owner is asked; Cancel sends nothing.
        state = view(true, 4, 4);
        panel.reload();
        const auto before = calls.size();
        panel.browse();
        QCOMPARE(asked, 1); QCOMPARE(calls.size(), before);
        confirm = true;
        panel.browse();
        QCOMPARE(calls.last().first, QString("move")); QCOMPARE(calls.last().second.value("to").toString(), QString("/srv/memory"));
        QCOMPARE(calls.last().second.value("expected_revision").toInteger(), 4);
        QVERIFY(panel.findChild<QLabel *>("projectMemoryNotice")->text().contains("The old store is kept at /state/lore/projects/p-1"));
        QCOMPARE(panel.view().value("store").toString(), QString("/srv/memory"));
    }
    void aFailedWriteReadsAgainAndAnOlderHgsIsReadOnly()
    {
        int reads = 0;
        ProjectMemoryPanel panel;
        panel.setBackend([&](const QString &, const QString &op, const QJsonObject &, IronRules::Reply done) {
            if (op == "get") { ++reads; done(view(false, 0, 0), {}); return; }
            done({}, "timed out");
        });
        panel.setProject("p-1");
        panel.findChild<QCheckBox *>("projectMemorySwitch")->click();
        QCOMPARE(reads, 2);
        QCOMPARE(panel.findChild<QLabel *>("projectMemoryError")->text(), QString("timed out"));
        ProjectMemoryPanel old;
        old.setBackend([](const QString &, const QString &, const QJsonObject &, IronRules::Reply done) { done({{"unsupported", true}}, {}); });
        old.setProject("p-1");
        QVERIFY(!old.editable());
        QVERIFY(old.findChild<QLabel *>("projectMemoryError")->text().contains("Update this computer's hgs"));
    }
};

QTEST_MAIN(TestProjectMemory)
#include "test_projectmemory.moc"
