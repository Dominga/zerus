// Drives the Qt client's own DownloadCache (tray/src/DownloadCache.cpp) on a
// folder, so client/core/src/download_cache.rs can be checked against it:
// the core's ignored test download_cache::tests::qt_and_the_client_share_a_folder
// runs it (ZERUS_QT_DOWNLOAD_CACHE_CHECK=<binary>). Build with Qt 6 from the
// repository root:
//   c++ -std=c++17 -fPIC -Itray/src client/scripts/qt-download-cache-check.cpp \
//     tray/src/DownloadCache.cpp tray/src/JsonParse.cpp \
//     $(pkg-config --cflags --libs Qt6Core) -o qt-download-cache-check
// Usage (times in ms since the epoch; prints one line):
//   qt-download-cache-check publish ROOT NAME HOST PATH CONTENT NOW  -> the copy's path
//   qt-download-cache-check lookup ROOT HOST PATH NOW                -> the path or "miss"
//   qt-download-cache-check trim ROOT LIMIT NOW                      -> "BYTES FILES"
//   qt-download-cache-check usage ROOT                               -> "BYTES FILES"
// An error prints "error: TEXT".
#include "DownloadCache.h"
#include <QCoreApplication>
#include <QFile>
#include <cstdio>

static int print(const QString &line) { std::printf("%s\n", qPrintable(line)); return 0; }
static int usageLine(const DownloadCache::Usage &usage) {
    return usage.error.isEmpty() ? print(QString("%1 %2").arg(usage.bytes).arg(usage.files)) : print("error: " + usage.error);
}

int main(int argc, char **argv) {
    QCoreApplication app(argc, argv);
    const auto args = app.arguments();
    const auto mode = args.value(1);
    if (mode == "publish" && args.size() == 8) {
        const auto prepared = DownloadCache::prepare(args[2]);
        if (!prepared.error.isEmpty()) return print("error: " + prepared.error);
        QFile in(args[6]), out(prepared.pending->path);
        if (!in.open(QIODevice::ReadOnly) || !out.open(QIODevice::WriteOnly)) return print("error: io");
        out.write(in.readAll()); out.close();
        const auto result = DownloadCache::publish(prepared.pending, args[3], args[7].toLongLong(), {args[4], args[5]});
        return print(result.error.isEmpty() ? result.path : "error: " + result.error);
    }
    if (mode == "lookup" && args.size() == 6) {
        const auto result = DownloadCache::lookup(args[2], {args[3], args[4]}, args[5].toLongLong());
        if (!result.error.isEmpty()) return print("error: " + result.error);
        return print(result.path.isEmpty() ? "miss" : result.path);
    }
    if (mode == "trim" && args.size() == 5) return usageLine(DownloadCache::trim(args[2], args[3].toLongLong(), args[4].toLongLong()));
    if (mode == "usage" && args.size() == 3) return usageLine(DownloadCache::usage(args[2]));
    return 2;
}
