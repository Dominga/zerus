#include <QtTest>
#include "Settings.h"
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QSettings>
#include <QTemporaryDir>
#include "IronRulesPanel.h"

// WhiteWh fork: a project's iron rules travel ahead of every message and are
// removed again before Activity shows or matches it.
class TestIronRules : public QObject {
    Q_OBJECT
    QTemporaryDir m_settings;
private slots:
    void initTestCase()
    {
        QCoreApplication::setOrganizationName("hgs-tests");
        QCoreApplication::setApplicationName("iron-rules");
        QSettings::setDefaultFormat(QSettings::IniFormat);
        QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, m_settings.path());
    }
    void init() { Settings::flush();QSettings().clear(); }
    void wrapAndStripRoundTrip()
    {
        QCOMPARE(IronRules::wrap({}, "Hello"), QString("Hello"));
        const auto sent = IronRules::wrap({"Never push without asking", "Text in English"}, "Build it\nand test it");
        QVERIFY(sent.startsWith(IronRules::opening() + "\n1. Never push without asking\n2. Text in English\n[/Iron rules]\n\n"));
        bool found = false;
        QCOMPARE(IronRules::strip(sent, &found), QString("Build it\nand test it")); QVERIFY(found);
        QCOMPARE(IronRules::strip("Just a message", &found), QString("Just a message")); QVERIFY(!found);
        // Only a leading block counts.
        QCOMPARE(IronRules::strip("Quote: " + sent, &found), "Quote: " + sent); QVERIFY(!found);
    }
    void rulesBelongToTheirProject()
    {
        IronRules::setRules("dcb", {"  Run   the tests ", "", "Stay in English"});
        QCOMPARE(IronRules::rules("dcb"), QStringList({"Run the tests", "Stay in English"}));
        QVERIFY(IronRules::rules("zerus").isEmpty()); QVERIFY(IronRules::rules({}).isEmpty());
        QStringList many; for (int i = 0; i < 30; ++i) many << QString("Rule %1").arg(i);
        IronRules::setRules("zerus", many); QCOMPARE(IronRules::rules("zerus").size(), IronRules::MaxRules);
        IronRules::setRules("dcb", {}); QVERIFY(IronRules::rules("dcb").isEmpty());
    }
    void importFailureKeepsTheLegacyRulesAndSuccessfulImportUsesRust()
    {
        IronRules::setRules("legacy", {"Keep this rule"});
        bool fail = true; int imports = 0; QJsonObject last;
        IronRules::Transport send = [&](const QString &, const QString &op, const QJsonObject &input, IronRules::Reply done) {
            if (op == "get") { done({{"rules", QJsonArray{}}, {"revision", 0}}, {}); return; }
            ++imports; last = input;
            if (fail) done({}, "Import failed");
            else done({{"rules", input.value("rules")}, {"revision", 1}, {"migrated", true}}, {});
        };
        QString error; QJsonObject result;
        IronRules::loadProject(send, {}, "legacy", [&](const auto &v, const auto &e) { result = v; error = e; });
        QCOMPARE(error, QString("Import failed")); QCOMPARE(IronRules::rules("legacy"), QStringList{"Keep this rule"});
        fail = false;
        IronRules::loadProject(send, {}, "legacy", [&](const auto &v, const auto &e) { result = v; error = e; });
        QVERIFY(error.isEmpty()); QCOMPARE(imports, 2); QCOMPARE(result.value("rules").toArray(), QJsonArray{"Keep this rule"});
        QCOMPARE(last.value("project").toString(), QString("legacy"));
        QCOMPARE(IronRules::rules("legacy"), QStringList{"Keep this rule"});
    }
    void startupImportsEveryProjectSequentiallyAndKeepsTheBackup()
    {
        IronRules::setRules("one", {"First project rule"}); IronRules::setRules("two", {"Second project rule"});
        QStringList imported;
        IronRules::Transport send = [&](const QString &, const QString &op, const QJsonObject &input, IronRules::Reply done) {
            if (op == "get") done({{"rules", QJsonArray{}}, {"revision", 0}}, {});
            else { imported << input.value("project").toString(); done({{"migrated", true}}, {}); }
        };
        IronRules::migrateLegacy(send, this, [](const QString &) { QFAIL("Import should succeed"); });
        QTRY_COMPARE(imported.size(), 2); QCOMPARE(imported, QStringList({"one", "two"}));
        QCOMPARE(IronRules::rules("one"), QStringList{"First project rule"});
        QCOMPARE(IronRules::rules("two"), QStringList{"Second project rule"});
    }
    void sessionPanelUsesExactIdentityAndKeepsDraftOnWriteFailure()
    {
        IronRulesPanel panel; QJsonObject request;
        panel.setBackend([&](const QString &, const QString &op, const QJsonObject &input, IronRules::Reply done) {
            if (op == "set") { request = input; done({}, "Identity changed"); return; }
            done({{"rules", QJsonArray{}}, {"revision", 4}, {"migrated", true}}, {});
        });
        panel.setContext("peer", "claude/x/one", "r1", "c1"); panel.setProject("p1", "Project"); panel.show();
        panel.findChild<QComboBox *>("ironRulesScope")->setCurrentIndex(1);
        auto *field = panel.findChild<QLineEdit *>("ironRuleField"); field->setText("Own addition");
        panel.findChild<QPushButton *>("addIronRule")->click();
        QCOMPARE(request.value("scope").toString(), QString("session"));
        QCOMPARE(request.value("expected_run_id").toString(), QString("r1"));
        QCOMPARE(request.value("expected_conversation_id").toString(), QString("c1"));
        QCOMPARE(field->text(), QString("Own addition")); QVERIFY(panel.rules().isEmpty());
        QCOMPARE(panel.findChild<QLabel *>("ironRulesError")->text(), QString("Identity changed"));
    }
    void panelAddsAndRemovesRules()
    {
        IronRulesPanel panel; int reported = -1; panel.changed = [&](int count) { reported = count; };
        panel.show();
        panel.setProject("dcb", "DCB"); QVERIFY(panel.isVisible());
        auto *field = panel.findChild<QLineEdit *>("ironRuleField"); auto *add = panel.findChild<QPushButton *>("addIronRule");
        QVERIFY(!add->isEnabled());
        field->setText("Never push without asking"); QVERIFY(add->isEnabled()); add->click();
        field->setText("Text in English"); QTest::keyClick(field, Qt::Key_Return);
        QCOMPARE(reported, 2); QCOMPARE(IronRules::rules("dcb"), QStringList({"Never push without asking", "Text in English"}));
        QCOMPARE(panel.findChildren<QLabel *>("ironRuleText").size(), 2); QVERIFY(field->text().isEmpty());
        panel.findChildren<QPushButton *>("removeIronRule").first()->click();
        QCOMPARE(reported, 1); QCOMPARE(IronRules::rules("dcb"), QStringList({"Text in English"}));
        panel.setProject("zerus", "Zerus"); QCOMPARE(panel.findChildren<QLabel *>("ironRuleText").size(), 0);
    }
};

QTEST_MAIN(TestIronRules)
#include "test_ironrules.moc"
