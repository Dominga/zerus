// Drives the Qt client's own AnalyticsData (tray/src/AnalyticsData.cpp) so
// client/core/src/analytics.rs can be checked against it: the core's ignored
// test analytics::tests::qt_and_the_client_read_replies_alike runs it
// (ZERUS_QT_ANALYTICS_CHECK=<binary>). Build with Qt 6 from the repository
// root:
//   c++ -std=c++17 -fPIC -Itray/src client/scripts/qt-analytics-check.cpp \
//     tray/src/AnalyticsData.cpp tray/src/JsonParse.cpp \
//     $(pkg-config --cflags --libs Qt6Core) -o qt-analytics-check
// Usage (days yyyy-MM-dd; prints one line of compact JSON, or "invalid"):
//   qt-analytics-check summary FILE FROM THROUGH   the checked summary
//   qt-analytics-check speech FILE FROM THROUGH    the checked speech statistics
//   qt-analytics-check fleet FILE FROM THROUGH     FILE holds [{route, agent,
//       voice, current, voice_current}]; prints the unique computers' routes,
//       their buckets' totals and histogram totals, and the diagnostic
#include "AnalyticsData.h"
#include "JsonParse.h"
#include <QCoreApplication>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <cstdio>

static QByteArray read(const QString &path) {
    QFile file(path);
    return file.open(QIODevice::ReadOnly) ? file.readAll() : QByteArray();
}
static QByteArray compact(const QJsonObject &object) { return QJsonDocument(object).toJson(QJsonDocument::Compact); }
static int print(const QJsonObject &object) { std::printf("%s\n", compact(object).constData()); return 0; }

int main(int argc, char **argv) {
    QCoreApplication app(argc, argv);
    const auto args = app.arguments();
    if (args.size() != 5) return 2;
    const auto from = QDate::fromString(args[3], Qt::ISODate), through = QDate::fromString(args[4], Qt::ISODate);
    const auto bytes = read(args[2]);
    if (args[1] == "summary" || args[1] == "speech") {
        const auto parsed = args[1] == "summary" ? AnalyticsData::summary(bytes, from, through) : AnalyticsData::speech(bytes, from, through);
        if (!parsed) { std::printf("invalid\n"); return 0; }
        return print(*parsed);
    }
    if (args[1] != "fleet") return 2;
    QList<AnalyticsData::Observation> observations;
    for (const auto &value : JsonParse::array(bytes)) {
        const auto row = value.toObject();
        AnalyticsData::Observation observation;
        observation.route = row.value("route").toString();
        if (const auto agent = AnalyticsData::summary(compact(row.value("agent").toObject()), from, through)) observation.agent = *agent;
        if (const auto voice = AnalyticsData::speech(compact(row.value("voice").toObject()), from, through)) observation.voice = *voice;
        observation.current = row.value("current").toBool();
        observation.voiceCurrent = row.value("voice_current").toBool();
        observations.append(observation);
    }
    QJsonArray routes, buckets;
    for (const auto &observation : AnalyticsData::uniqueComputers(observations)) {
        routes.append(observation.route);
        for (const auto &bucket : observation.agent.value("buckets").toArray()) buckets.append(bucket);
    }
    return print({{"unique", routes}, {"totals", AnalyticsData::totals(buckets)},
        {"histograms", AnalyticsData::histogramTotals(buckets)}, {"diagnostic", AnalyticsData::diagnostic(observations, from, through)}});
}
