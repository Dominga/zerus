// Checks client/core/src/qt_lock.rs against Qt's own QLockFile: the core's
// ignored test qt_lock::tests::qt_and_the_client_honour_each_others_locks
// runs it (ZERUS_QT_LOCK_CHECK=<binary>). Build with Qt 6:
//   c++ -std=c++17 -fPIC qt-lock-check.cpp $(pkg-config --cflags --libs Qt6Core) -o qt-lock-check
// Usage:
//   qt-lock-check try PATH      tries the lock for 500 ms, no stale age;
//                               prints "locked" or "busy" (and keeps nothing)
//   qt-lock-check hold PATH     takes the lock, prints "held", then waits
//                               until it is killed or stdin closes
#include <QCoreApplication>
#include <QLockFile>
#include <QTextStream>
#include <cstdio>
#include <iostream>

int main(int argc, char **argv) {
    QCoreApplication app(argc, argv);
    if (argc != 3) return 2;
    const QString mode = QString::fromLocal8Bit(argv[1]);
    QLockFile lock(QString::fromLocal8Bit(argv[2]));
    lock.setStaleLockTime(0);
    if (mode == "try") {
        const bool locked = lock.tryLock(500);
        std::printf("%s\n", locked ? "locked" : "busy");
        return 0;
    }
    if (mode == "hold") {
        if (!lock.tryLock(0)) { std::printf("busy\n"); return 1; }
        std::printf("held\n");
        std::fflush(stdout);
        std::string line;
        std::getline(std::cin, line);
        return 0;
    }
    return 2;
}
