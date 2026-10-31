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
