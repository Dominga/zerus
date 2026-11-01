#include <QtTest>
#include "Settings.h"
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QSettings>
#include <QTemporaryDir>
#include <QProcess>
#include <QProcessEnvironment>
#include <QFileInfo>
#include <QDir>
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
    void aRemoteProofCannotOmitTheLocalProjectAndFailedImportKeepsLegacyDelivery()
    {
        IronRules::setRules("p1", {"Legacy local rule"});
        bool failImport=false; QString delivered,error;
        IronRules::Transport send=[&](const QString &,const QString &op,const QJsonObject &input,IronRules::Reply done) {
            if(op=="migrate") { done({},"Import failed"); return; }
            if(input.value("scope")=="project") { done({{"rules",QJsonArray{"Local project rule"}},{"migrated",!failImport},{"revision",0}},{});return; }
            done({{"effective_rules","Peer project rule\n"},{"project","p1"},{"start_project","p1"},{"start_delivery_supported",true}},{});
        };
        const auto done=[&](const QString &text,const QString &failure){delivered=text;error=failure;};
        IronRules::prepare(send,"peer","p1","claude/x/one","r1","c1","Hello",done);
        QVERIFY(error.isEmpty()); QVERIFY(delivered.contains("Local project rule")); QVERIFY(delivered.contains("Peer project rule"));
        QCOMPARE(IronRules::strip(delivered),QString("Hello"));
        failImport=true;
        IronRules::prepare(send,{},"p1","claude/x/one","r1","c1","Hello",done);
        QVERIFY(error.isEmpty()); QVERIFY(delivered.contains("Legacy local rule")); QCOMPARE(IronRules::strip(delivered),QString("Hello"));
    }
    void panelAddsAndRemovesThroughTheActualRustCli()
    {
        QTemporaryDir isolated;
#ifdef Q_OS_WIN
        const auto binary=QDir(QCoreApplication::applicationDirPath()).absoluteFilePath("../../../target/debug/hgs.exe");
#else
        const auto binary=QDir(QCoreApplication::applicationDirPath()).absoluteFilePath("../../../target/debug/hgs");
#endif
        QVERIFY2(QFileInfo::exists(binary),qPrintable(binary));
        IronRulesPanel panel;
        panel.setBackend([&](const QString &,const QString &op,QJsonObject input,IronRules::Reply done) {
            QProcess process; auto env=QProcessEnvironment::systemEnvironment();
            for(const auto &key:{"HGS_SESSION","HGS_RUN_ID","HGS_HOST_PANE","DSH_SESSION_ID","TMUX"}) env.remove(key);
            for(const auto &key:{"HOME","USERPROFILE"}) env.insert(key,isolated.path()+"/home");
            env.insert("HGS_STATE_DIR",isolated.path()+"/state"); env.insert("HGS_CONFIG_DIR",isolated.path()+"/config");
            process.setProcessEnvironment(env); input.insert("by","human");
            process.start(binary,{"rules",op,"--json"});
            if(!process.waitForStarted(5000)){done({},process.errorString());return;}
            process.write(QJsonDocument(input).toJson(QJsonDocument::Compact));process.closeWriteChannel();
            if(!process.waitForFinished(5000)){process.kill();process.waitForFinished();done({},"CLI timeout");return;}
            const auto output=QJsonDocument::fromJson(process.readAllStandardOutput()).object();
            done(output,process.exitCode()==0?QString():QString::fromUtf8(process.readAllStandardError()));
        });
        panel.setProject("native-cli-project","Native project"); panel.show();
        auto *field=panel.findChild<QLineEdit *>("ironRuleField"); field->setText("Persist this rule");
        panel.findChild<QPushButton *>("addIronRule")->click();
        QCOMPARE(panel.rules(),QStringList{"Persist this rule"});
        QVERIFY(panel.findChild<QLabel *>("ironRulesError")->text().isEmpty());
        panel.findChild<QPushButton *>("removeIronRule")->click(); QVERIFY(panel.rules().isEmpty());
        panel.findChild<QPushButton *>("reloadIronRules")->click(); QVERIFY(panel.rules().isEmpty());
        QVERIFY(panel.findChild<QLabel *>("ironRulesError")->text().isEmpty());
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
