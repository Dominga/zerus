// Records how Qt 6 writes and reads the client's settings keys in INI form:
// `record write FILE` writes the INI with QSettings; `record read FILE` prints, per key,, what Qt's readers
// get back (toBool, toDouble, toInt(&ok), toLongLong, toString, toStringList).
#include <QCoreApplication>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSettings>
#include <QStringList>
#include <cstdio>

int main(int argc, char **argv) {
    QCoreApplication app(argc, argv);
    const QString path = QString::fromLocal8Bit(argv[2]);
    // "write" and "read" run as separate processes, so the read parses the
    // file instead of answering from QSettings' in-process cache.
    if (qstrcmp(argv[1], "write") == 0) {
        QSettings s(path, QSettings::IniFormat);
        s.clear();
        s.setValue("workspace/theme", "dark");
        s.setValue("workspace/language", "ru");
        s.setValue("workspace/usageIndicator", "marker-countdown");
        s.setValue("workspace/contentScale", 1.25);
        s.setValue("workspace/activityWidth", 1300);
        s.setValue("workspace/activityFullWidth", true);
        s.setValue("workspace/alwaysOnTop", false);
        s.setValue("workspace/compact", true);
        s.setValue("workspace/hideEmptyProjects", false);
        s.setValue("notifications/optionalQuestions", true);
        s.setValue("processes/enabled", true);
        s.setValue("processes/activeSeconds", 99.5);
        s.setValue("processes/backgroundSeconds", 2);
        s.setValue("processes/loadingSeconds", QStringLiteral("abc"));
        s.setValue("speech/host", "studio");
        s.setValue("speech/profile", QStringLiteral("@profile"));
        s.setValue("speech/machine_id", QStringLiteral("  spaced, \"quoted\"\tand\nnew line  "));
        s.setValue("speech/profile_revision", 7.0);
        s.setValue("speech/preview_before_send", QStringLiteral("False"));
        s.setValue("speech/video_mode", 1);
        s.setValue("speech/microphone", QByteArray("mic-id"));
        s.setValue("downloads/cacheBytes", qint64(5368709120));
        s.setValue("downloads/folder", QStringLiteral("/srv/Загрузки, общие"));
        s.setValue("updates/channel", "dev");
        s.setValue("updates/automatic", false);
        s.setValue("automaticApprovals/enabled", true);
        s.setValue("shortcuts/queue.sendNow", QStringList{"Ctrl+Return", "Meta+Return"});
        s.setValue("shortcuts/message.push", QStringList{});
        s.setValue("shortcuts/app.settings", QStringList{"Ctrl+,"});
        s.setValue("shortcuts/app.find", QStringList{""});
        s.setValue("General key", "top level");
        s.sync();
        return 0;
    }
    QSettings s(path, QSettings::IniFormat);
    QJsonObject out;
    for (const QString &key : s.allKeys()) {
        const QVariant v = s.value(key);
        bool intOk = false, doubleOk = false, longOk = false;
        const int i = v.toInt(&intOk);
        const double d = v.toDouble(&doubleOk);
        const qlonglong l = v.toLongLong(&longOk);
        QJsonObject read;
        read["bool"] = v.toBool();
        read["double"] = d;
        read["double_ok"] = doubleOk;
        read["int"] = i;
        read["int_ok"] = intOk;
        read["long"] = QString::number(l);
        read["long_ok"] = longOk;
        read["string"] = v.toString();
        read["list"] = QJsonArray::fromStringList(v.toStringList());
        read["valid"] = v.isValid();
        out[key] = read;
    }
    std::fputs(QJsonDocument(out).toJson(QJsonDocument::Indented).constData(), stdout);
    return 0;
}
