#include <QtTest>

#include <QSignalSpy>
#include <QTemporaryDir>

#include <thread>

#include "FakeCli.h"
#include "HgsClient.h"
#include "JsonParse.h"
#include "ProcessRunner.h"

// The GUI thread applies values that workers prepared (JsonParse.h). Every
// suite fails on a parse there; this one checks the rule itself and that the
// client's polls parse off it.
class TestJsonParse : public QObject {
    Q_OBJECT
private slots:
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
    void localValuesAreLimited();
    void clientPollsParseOffTheGuiThread();

private:
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

void TestJsonParse::localValuesAreLimited()
{
    QCOMPARE(JsonParse::localObject(R"({"small":true})").value("small").toBool(), true);
    QVERIFY(m_sites.isEmpty());
    const QByteArray large = "[\"" + QByteArray(JsonParse::LocalLimit, 'x') + "\"]";
    QCOMPARE(JsonParse::localArray(large).size(), 1);
    QCOMPARE(m_sites.size(), 1);
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
