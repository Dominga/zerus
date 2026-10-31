#include <QtTest>
#include "Settings.h"
#include <QDBusMessage>
#include <QDBusPendingReply>
#include <QFile>
#include <QJSEngine>
#include <QJsonArray>
#include <QJsonDocument>
#include <QSettings>
#include <QTemporaryDir>
#include <QTextEdit>
#include "KWinWindowLayer.h"
#include "WindowLayer.h"

namespace {
const QString id = QStringLiteral("{11111111-1111-4111-8111-111111111111}");
const QString appId = QStringLiteral("org.example.ZerusWindowLayerTest");
QJsonObject window(const QString &caption, const QString &uuid = id) {
    return {{"pid", 123}, {"resourceClass", appId}, {"caption", caption},
        {"normalWindow", true}, {"dialog", false}, {"deleted", false},
        {"internalId", uuid}, {"keepAbove", false}};
}
}

class ScriptRunner : public QObject {
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.kde.kwin.Script")
public:
    std::function<void()> execute;
public slots:
    void run() { if (execute) execute(); }
};

class FakeKWin : public QObject {
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.kde.kwin.Scripting")
public:
    explicit FakeKWin(const QDBusConnection &connection) : bus(connection) {
        bus.registerObject("/Scripting", this, QDBusConnection::ExportAllSlots);
        bus.registerObject("/Scripting/Script7", &runner, QDBusConnection::ExportAllSlots);
        runner.execute = [this] { if (!hold) report(bus, config.value("token").toString()); };
    }
    QDBusConnection bus;
    ScriptRunner runner;
    QJsonObject config;
    QString file, plugin;
    bool hold = false, reject = false, above = false;
    QString error;
    int unloaded = 0;
    void report(const QDBusConnection &sender, const QString &token) {
        auto message = QDBusMessage::createMethodCall(config["service"].toString(), config["path"].toString(),
            "org.hgdev.Zerus.WindowLayer", "Report");
        message.setArguments({token, id, above, error}); sender.asyncCall(message);
    }
public slots:
    int loadScript(const QString &path, const QString &name) {
        file = path; plugin = name;
        QFile input(path); if (!input.open(QIODevice::ReadOnly)) return -1;
        const auto bytes = input.readAll();
        config = QJsonDocument::fromJson(bytes.mid(15, bytes.indexOf(";\n") - 15)).object();
        return reject ? -1 : 7;
    }
    bool unloadScript(const QString &name) { if (name == plugin) ++unloaded; return true; }
};

class WindowInfo : public QObject {
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.kde.KWin")
public:
    QVariantMap info;
public slots:
    QVariantMap getWindowInfo(const QString &) { return info; }
};

class TestKWinWindowLayer : public QObject {
    Q_OBJECT
    QTemporaryDir settings;
private slots:
    void initTestCase() {
        QCoreApplication::setOrganizationName("ZerusWindowLayerTests");
        QCoreApplication::setApplicationName(appId); QGuiApplication::setDesktopFileName(appId);
        QVERIFY(settings.isValid()); QSettings::setDefaultFormat(QSettings::IniFormat);
        QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, settings.path());
    }
    void scriptTargetsOnlyOneOwnedWindow_data() {
        QTest::addColumn<QString>("scenario"); QTest::addColumn<QString>("error"); QTest::addColumn<bool>("on");
        QTest::newRow("exact") << "exact" << "" << true;
        QTest::newRow("wrong-pid") << "pid" << "missing" << false;
        QTest::newRow("wrong-app") << "app" << "missing" << false;
        QTest::newRow("dialog") << "dialog" << "missing" << false;
        QTest::newRow("deleted") << "deleted" << "missing" << false;
        QTest::newRow("ambiguous") << "ambiguous" << "ambiguous" << false;
        QTest::newRow("missing-id") << "id" << "missing" << false;
        QTest::newRow("unsupported") << "unsupported" << "unsupported" << false;
        QTest::newRow("forced-rule") << "denied" << "denied" << false;
        QTest::newRow("read-only-probe") << "probe" << "" << false;
        QTest::newRow("literal-caption") << "caption" << "" << true;
    }
    void scriptTargetsOnlyOneOwnedWindow() {
        QFETCH(QString, scenario); QFETCH(QString, error); QFETCH(bool, on);
        const auto caption = scenario == "caption" ? QString("probe\"; throw Error('injection'); //\n") : QString("probe");
        auto target = window(caption); auto other = window("other", "{22222222-2222-4222-8222-222222222222}");
        if (scenario == "pid") target["pid"] = 124;
        if (scenario == "app") target["resourceClass"] = "org.example.Other";
        if (scenario == "dialog") target["dialog"] = true;
        if (scenario == "deleted") target["deleted"] = true;
        if (scenario == "unsupported") target.remove("keepAbove");
        QJsonArray windows{target, other}; if (scenario == "ambiguous") windows.append(target);
        QJsonObject config{{"pid", 123}, {"appId", appId}, {"caption", caption},
            {"id", scenario == "id" ? "{33333333-3333-4333-8333-333333333333}" : ""},
            {"desired", scenario == "probe" ? QJsonValue() : QJsonValue(true)},
            {"service", "org.example.Test"}, {"path", "/test"}, {"token", "nonce"}, {"plugin", "test-plugin"}};
        QJSEngine engine;
        engine.evaluate("var windows = " + QString::fromUtf8(QJsonDocument(windows).toJson(QJsonDocument::Compact)) +
            "; var calls = []; var workspace = {windowList: function(){return windows;}};"
            "function callDBus(){calls.push(Array.prototype.slice.call(arguments));}");
        if (scenario == "denied") engine.evaluate("Object.defineProperty(windows[0], 'keepAbove', {get:function(){return false;},set:function(){}});");
        const auto result = engine.evaluate(KWinWindowLayer::script(config));
        QVERIFY2(!result.isError(), qPrintable(result.toString()));
        QCOMPARE(engine.evaluate("calls.length").toInt(), 2);
        QCOMPARE(engine.evaluate("calls[0][7]").toString(), error);
        QCOMPARE(engine.evaluate("calls[0][6]").toBool(), on);
        QVERIFY(!engine.evaluate("windows[1].keepAbove").toBool());
        QCOMPARE(engine.evaluate("calls[1][3]").toString(), QString("unloadScript"));
    }
    void callbacksRequireOwnerAndNonceAndCleanUp() {
        if (qEnvironmentVariableIsSet("ZERUS_TEST_KWIN")) QSKIP("Fake compositor runs only on the private test bus.");
        auto server = QDBusConnection::connectToBus(QDBusConnection::SessionBus, "kwin-server");
        const auto client = QDBusConnection::connectToBus(QDBusConnection::SessionBus, "kwin-client");
        const auto stranger = QDBusConnection::connectToBus(QDBusConnection::SessionBus, "kwin-stranger");
        QVERIFY(server.registerService("org.kde.KWin"));
        {
            FakeKWin fake(server); fake.hold = true;
            KWinWindowLayer layer(nullptr, client); QSignalSpy results(&layer, &KWinWindowLayer::finished);
            layer.request("probe", {}, std::nullopt); QTRY_VERIFY(!fake.config.isEmpty());
            QCOMPARE(fake.config["pid"].toInteger(), QCoreApplication::applicationPid());
            layer.Report(fake.config["token"].toString(), id, true, {});
            fake.report(stranger, fake.config["token"].toString()); fake.report(server, "wrong-nonce");
            QTest::qWait(60); QCOMPARE(results.size(), 0);
            fake.above = true; fake.report(server, fake.config["token"].toString());
            QTRY_COMPARE(results.size(), 1); QCOMPARE(results[0][0].toString(), id); QVERIFY(results[0][1].toBool());
            QTRY_VERIFY(fake.unloaded > 0); QVERIFY(!QFile::exists(fake.file));
            fake.config = {}; layer.request("probe", id, false); QTRY_VERIFY(!fake.config.isEmpty());
            const auto stale = fake.config["token"].toString(); layer.cancel(); fake.report(server, stale);
            QTest::qWait(60); QCOMPARE(results.size(), 1); QVERIFY(!QFile::exists(fake.file));
            fake.config = {}; fake.hold = false; fake.reject = true;
            layer.request("probe", id, true); QTRY_COMPARE(results.size(), 2);
            QCOMPARE(results.last()[2].toString(), QString("unavailable")); QVERIFY(!QFile::exists(fake.file));
            fake.reject = false; fake.hold = true; layer.request("probe", id, true);
            QTRY_COMPARE_WITH_TIMEOUT(results.size(), 3, 5000);
            QCOMPARE(results.last()[2].toString(), QString("unavailable")); QVERIFY(!QFile::exists(fake.file));
        }
        server.unregisterObject("/Scripting"); server.unregisterObject("/Scripting/Script7"); server.unregisterService("org.kde.KWin");
    }
    void inspectionRejectsOtherWindows() {
        if (qEnvironmentVariableIsSet("ZERUS_TEST_KWIN")) QSKIP("Fake compositor runs only on the private test bus.");
        auto server = QDBusConnection::connectToBus(QDBusConnection::SessionBus, "kwin-info-server");
        const auto client = QDBusConnection::connectToBus(QDBusConnection::SessionBus, "kwin-info-client");
        QVERIFY(server.registerService("org.kde.KWin"));
        WindowInfo fake; QVERIFY(server.registerObject("/KWin", &fake, QDBusConnection::ExportAllSlots));
        KWinWindowLayer layer(nullptr, client); QSignalSpy results(&layer, &KWinWindowLayer::finished);
        fake.info = {{"uuid", id}, {"pid", QCoreApplication::applicationPid()}, {"resourceClass", appId}, {"keepAbove", true}};
        layer.inspect(id); QTRY_COMPARE(results.size(), 1); QVERIFY(results.last()[1].toBool()); QVERIFY(results.last()[2].toString().isEmpty());
        fake.info["pid"] = 1; layer.inspect(id); QTRY_COMPARE(results.size(), 2); QCOMPARE(results.last()[2].toString(), QString("missing"));
        fake.info["pid"] = QCoreApplication::applicationPid(); fake.info["resourceClass"] = "org.example.Other";
        layer.inspect(id); QTRY_COMPARE(results.size(), 3); QCOMPARE(results.last()[2].toString(), QString("missing"));
        fake.info["resourceClass"] = appId; fake.info["uuid"] = "other";
        layer.inspect(id); QTRY_COMPARE(results.size(), 4); QCOMPARE(results.last()[2].toString(), QString("missing"));
        fake.info["uuid"] = id; fake.info["keepAbove"] = "true";
        layer.inspect(id); QTRY_COMPARE(results.size(), 5); QCOMPARE(results.last()[2].toString(), QString("unsupported"));
        server.unregisterObject("/KWin"); server.unregisterService("org.kde.KWin");
    }
    void nativeKWinChangesOnlyOurWindowAndRestores() {
        if (!qEnvironmentVariableIsSet("ZERUS_TEST_KWIN")) QSKIP("Opt-in native compositor check.");
        QVERIFY(QGuiApplication::platformName().startsWith("wayland"));
        QSettings().remove("workspace/alwaysOnTop");
        QWidget owned, peer; owned.setWindowTitle("Zerus isolated Keep Above test"); peer.setWindowTitle("Zerus unrelated test window");
        auto *editor = new QTextEdit(&owned); editor->setPlainText("Synthetic unsent draft");
        owned.resize(350, 160); peer.resize(200, 120); peer.show(); owned.show();
        WindowLayer controller(&owned); QTRY_VERIFY_WITH_TIMEOUT(controller.supported() && !controller.busy(), 6000);
        QVERIFY(!controller.onTop());
        KWinWindowLayer backend; QSignalSpy results(&backend, &KWinWindowLayer::finished);
        backend.request(owned.windowTitle(), {}, std::nullopt); QTRY_COMPARE(results.size(), 1);
        const auto ownId = results.last()[0].toString(); QVERIFY(!ownId.isEmpty());
        controller.request(true); QTRY_VERIFY(controller.onTop() && !controller.busy());
        backend.inspect(ownId); QTRY_COMPARE(results.size(), 2); QVERIFY(results.last()[1].toBool());
        QVERIFY(owned.isVisible()); QCOMPARE(editor->toPlainText(), QString("Synthetic unsent draft"));
        owned.hide(); owned.show(); QTRY_VERIFY(controller.supported() && !controller.busy()); QVERIFY(controller.onTop());
        backend.request(owned.windowTitle(), {}, std::nullopt); QTRY_COMPARE(results.size(), 3);
        backend.request(owned.windowTitle(), results.last()[0].toString(), false); QTRY_COMPARE(results.size(), 4);
        QTRY_VERIFY(!controller.onTop() && !controller.busy()); QVERIFY(!Settings::value("workspace/alwaysOnTop").toBool());
        backend.request(peer.windowTitle(), {}, std::nullopt); QTRY_COMPARE(results.size(), 5);
        QVERIFY(results.last()[2].toString().isEmpty()); QVERIFY(!results.last()[1].toBool());
        QSettings().remove("workspace/alwaysOnTop");
    }
};
QTEST_MAIN(TestKWinWindowLayer)
#include "test_kwinwindowlayer.moc"
