#include <QtTest>
#include "Settings.h"

#include <QSettings>
#include <QSignalSpy>
#include <QTemporaryDir>

#include <thread>

#include "FakeCli.h"
#include "HgsClient.h"
#include "JsonParse.h"
#include "ProcessRunner.h"
#include "SettingsJson.h"

// The GUI thread applies values that workers prepared (JsonParse.h). Every
// suite fails on a parse there; this one checks the rule itself and that the
// client's polls parse off it.
class TestJsonParse : public QObject {
    Q_OBJECT
private slots:
    void initTestCase()
    {
        QVERIFY(m_settings.isValid());
        QCoreApplication::setOrganizationName(QStringLiteral("hgs-tests"));
        QCoreApplication::setApplicationName(QStringLiteral("jsonparse"));
        QSettings::setDefaultFormat(QSettings::IniFormat);
        QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, m_settings.path());
    }
    void init()
    {
        m_sites.clear();
        JsonParse::setViolationHandler([this](const char *file, int line) {
            m_sites << QStringLiteral("%1:%2").arg(QFileInfo(QString::fromUtf8(file)).fileName()).arg(line);
        });
    }
    void cleanup() { JsonParse::setViolationHandler({}); }
    void parsingOnTheGuiThreadIsAViolation();
    void workersParseWithoutViolation();
    void settingsReadPreparedValuesWithoutParsing();
    void settingsChangedElsewhereAreParsedOffTheGuiThread();
    void settingsPreloadAndWarmExpandGroups();
    void clientPollsParseOffTheGuiThread();

private:
    // Applies after every prepare step posted before it (ProcessRunner.h).
    static void preparationBarrier()
    {
        QObject owner; bool done = false;
        ProcessRunner::post(&owner, [] { return true; }, [&done](bool) { done = true; });
        QTRY_VERIFY(done);
    }
    QTemporaryDir m_settings;
    QStringList m_sites;
};

void TestJsonParse::parsingOnTheGuiThreadIsAViolation()
{
    QVERIFY(JsonParse::onGuiThread());
    const int before = JsonParse::violations();
    QString error;
    const int line = __LINE__ + 1;
    const auto object = JsonParse::object(R"({"a":1})", &error);
    QVERIFY(error.isEmpty()); QCOMPARE(object.value("a").toInt(), 1);
    QCOMPARE(JsonParse::violations(), before + 1);
    QCOMPARE(m_sites, QStringList{QStringLiteral("test_jsonparse.cpp:%1").arg(line)});
    JsonParse::array(R"({"a":1})", &error);
    QCOMPARE(error, QStringLiteral("expected a JSON array"));
}

void TestJsonParse::workersParseWithoutViolation()
{
    QJsonObject object; bool guiThread = true;
    std::thread([&] { guiThread = JsonParse::onGuiThread(); object = JsonParse::object(R"({"b":2})"); }).join();
    QVERIFY(!guiThread); QCOMPARE(object.value("b").toInt(), 2);
    bool applied = false; QObject owner;
    ProcessRunner::post(&owner, [] { return JsonParse::object(R"({"c":3})"); },
                        [&applied](const QJsonObject &value) { applied = value.value("c").toInt() == 3; });
    QTRY_VERIFY(applied);
    QVERIFY(m_sites.isEmpty());
}

void TestJsonParse::settingsReadPreparedValuesWithoutParsing()
{
    const QString key = QStringLiteral("test/stored");
    SettingsJson::store(key, QJsonDocument(QJsonObject{{"a", 1}}));
    QCOMPARE(Settings::value(key).toByteArray(), QByteArray(R"({"a":1})"));
    QCOMPARE(SettingsJson::object(key).value("a").toInt(), 1);
    QSettings settings; SettingsJson::store(settings, key, QJsonDocument(QJsonObject{{"a", 2}}));
    QCOMPARE(SettingsJson::object(key).value("a").toInt(), 2);
    // A removed value needs no parse either.
    QSettings().remove(key);
    QVERIFY(SettingsJson::object(key).isEmpty()); QVERIFY(SettingsJson::document(key).isNull());
    QVERIFY2(m_sites.isEmpty(), qPrintable(m_sites.join(", ")));
}

void TestJsonParse::settingsChangedElsewhereAreParsedOffTheGuiThread()
{
    const QString key = QStringLiteral("test/external");
    SettingsJson::store(key, QJsonDocument(QJsonObject{{"a", 1}}));
    // Another process stores a new value: the GUI thread reports the read,
    // keeps the value it had and parses the new one on the preparation thread.
    QSettings().setValue(key, QByteArray(R"({"a":2})"));
    const int line = __LINE__ + 1;
    QCOMPARE(SettingsJson::object(key).value("a").toInt(), 1);
    QCOMPARE(m_sites, QStringList{QStringLiteral("test_jsonparse.cpp:%1").arg(line)});
    preparationBarrier(); m_sites.clear();
    QCOMPARE(SettingsJson::object(key).value("a").toInt(), 2);
    QVERIFY(m_sites.isEmpty());
    // A value read for the first time reads as empty until then.
    QSettings().setValue(QStringLiteral("test/unknown"), QByteArray("[1,2,3]"));
    QVERIFY(SettingsJson::array(QStringLiteral("test/unknown")).isEmpty()); QCOMPARE(m_sites.size(), 1);
    preparationBarrier(); m_sites.clear();
    QCOMPARE(SettingsJson::array(QStringLiteral("test/unknown")).size(), 3);
    // Other threads parse a changed value themselves.
    QSettings().setValue(key, QByteArray(R"({"a":3})"));
    int worker = 0;
    std::thread([&] { worker = SettingsJson::object(key).value("a").toInt(); }).join();
    QCOMPARE(worker, 3); QCOMPARE(SettingsJson::object(key).value("a").toInt(), 3);
    // An oversized value reads as empty without a parse.
    QCOMPARE(SettingsJson::document(key, 4).isNull(), true);
    QVERIFY2(m_sites.isEmpty(), qPrintable(m_sites.join(", ")));
}

void TestJsonParse::settingsPreloadAndWarmExpandGroups()
{
    QSettings settings;
    settings.setValue(QStringLiteral("test/cards/one/request"), QByteArray(R"({"n":1})"));
    settings.setValue(QStringLiteral("test/cards/two/request"), QByteArray(R"({"n":2})"));
    settings.setValue(QStringLiteral("test/cards/two/title"), QStringLiteral("kept"));
    settings.setValue(QStringLiteral("test/preloaded"), QByteArray(R"(["x"])"));
    SettingsJson::preload({QStringLiteral("test/preloaded"), QStringLiteral("test/cards/*/request")});
    QCOMPARE(SettingsJson::array(QStringLiteral("test/preloaded")), QJsonArray{"x"});
    QCOMPARE(SettingsJson::object(QStringLiteral("test/cards/one/request")).value("n").toInt(), 1);
    QCOMPARE(SettingsJson::object(QStringLiteral("test/cards/two/request")).value("n").toInt(), 2);
    settings.setValue(QStringLiteral("test/cards/three/request"), QByteArray(R"({"n":3})"));
    QObject owner;
    SettingsJson::warm(&owner, {QStringLiteral("test/cards/*/request")});
    preparationBarrier();
    QCOMPARE(SettingsJson::object(QStringLiteral("test/cards/three/request")).value("n").toInt(), 3);
    QVERIFY2(m_sites.isEmpty(), qPrintable(m_sites.join(", ")));
}

void TestJsonParse::clientPollsParseOffTheGuiThread()
{
    QTemporaryDir dir; QVERIFY(dir.isValid());
    const auto fixture = dir.filePath("hgs");
    QVERIFY(FakeCli::install(fixture, R"PY(#!/usr/bin/env python3
import json, sys
args = sys.argv[1:]
if args[:1] == ["ls"]:
    print(json.dumps({"host": "fixture", "ok": True, "sessions": [{"name": "codex/demo", "cmd": "codex", "activity": "idle"}]}))
elif args[:1] == ["inspect"]:
    print(json.dumps({"cursor": 7, "events": [{"seq": 7, "kind": "Stop"}]}))
else:
    sys.exit(2)
)PY"));
    HgsClient client(FakeCli::program(fixture));
    QSignalSpy local(&client, &HgsClient::localReady), inspected(&client, &HgsClient::inspectionReady);
    client.requestLocal();
    QVERIFY(client.requestInspection({}, QStringLiteral("codex/demo")));
    QTRY_COMPARE(local.size(), 1); QTRY_COMPARE(inspected.size(), 1);
    QCOMPARE(local.first().first().value<BoxState>().sessions.first().name, QStringLiteral("codex/demo"));
    QCOMPARE(inspected.first().at(2).toJsonObject().value("cursor").toInt(), 7);
    QVERIFY2(m_sites.isEmpty(), qPrintable(m_sites.join(", ")));
}

QTEST_MAIN(TestJsonParse)
#include "test_jsonparse.moc"
