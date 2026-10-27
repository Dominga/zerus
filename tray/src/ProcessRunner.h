#pragma once

#include <QByteArray>
#include <QProcess>
#include <QProcessEnvironment>
#include <QString>
#include <QStringList>

#include <functional>
#include <memory>
#include <optional>
#include <type_traits>
#include <utility>

class QObject;
class QThread;

// Starts short-lived helper processes on one shared worker thread. fork/exec of
// the large GUI process, pipe I/O and child reaping then never stall painting:
// the GUI thread only posts a request and later receives one result. Polls start
// about one hgs child per second, and on macOS each fork() cost ~4 ms there.
//
// Every result then passes one shared preparation thread before it reaches its
// owner. A caller with a prepare step parses and shapes the output there, so the
// owner's thread only applies a finished value. Results keep the order in which
// their children finished, with or without a prepare step.
namespace ProcessRunner {

struct Request {
    QString program;
    QStringList arguments;
    std::optional<QProcessEnvironment> environment;
    // Written once the child starts, then stdin is closed; an empty value only
    // closes it. Without a value stdin stays open, as with a plain QProcess.
    std::optional<QByteArray> input;
    int timeoutMs = 0;   // 0: no deadline
    // Combined stdout/stderr retention bound. First excess reports an explicit
    // failure and kills only this direct child; retained output stays bounded.
    // Zero preserves the existing unrestricted callers.
    qsizetype maximumOutputBytes = 0;
};

struct Result {
    enum Outcome { Finished, FailedToStart, TimedOut, OutputLimitExceeded };
    Outcome outcome = Finished;
    int exitCode = -1;
    QProcess::ExitStatus exitStatus = QProcess::NormalExit;
    // The first QProcess error, e.g. Crashed before a CrashExit finish.
    QProcess::ProcessError error = QProcess::UnknownError;
    QString errorString;
    QByteArray standardOutput, standardError;
    bool outputLimitExceeded = false;
};

// Reports exactly once on owner's thread, which must be the application thread.
// A deadline kills the child and reports TimedOut at once. Destroying owner
// kills and reaps its running children before returning and drops their results.
void run(Request request, QObject *owner, std::function<void(const Result &)> done);

// A prepared value, type-erased between the two threads.
using Prepared = std::shared_ptr<void>;
void runPrepared(Request request, QObject *owner, std::function<Prepared(Result)> prepare,
                 std::function<void(Prepared)> apply);

// As run(), but prepare(Result) runs on the preparation thread and its value goes
// to apply on owner's thread. prepare must not touch owner or any other
// application-thread object: it sees only the result and what it captured by
// value. A destroyed owner still drops the value instead of applying it.
template <typename Prepare, typename Apply>
void run(Request request, QObject *owner, Prepare prepare, Apply apply)
{
    using Value = std::decay_t<std::invoke_result_t<Prepare &, Result>>;
    runPrepared(std::move(request), owner,
        [prepare = std::move(prepare)](Result result) mutable -> Prepared {
            return std::make_shared<Value>(prepare(std::move(result)));
        },
        [apply = std::move(apply)](Prepared value) mutable { apply(std::move(*std::static_pointer_cast<Value>(value))); });
}

// Prepares data that did not come from a child of run(), such as a direct
// QProcess result or a file, the same way: prepare() on the preparation thread,
// then apply(value) on owner's thread unless owner is destroyed first.
void postPrepared(QObject *owner, std::function<Prepared()> prepare, std::function<void(Prepared)> apply);
template <typename Prepare, typename Apply>
void post(QObject *owner, Prepare prepare, Apply apply)
{
    using Value = std::decay_t<std::invoke_result_t<Prepare &>>;
    postPrepared(owner,
        [prepare = std::move(prepare)]() mutable -> Prepared { return std::make_shared<Value>(prepare()); },
        [apply = std::move(apply)](Prepared value) mutable { apply(std::move(*std::static_pointer_cast<Value>(value))); });
}

// True on the shared preparation thread.
bool isPreparationThread();

// Tests: the worker and preparation threads, and a hook run on the worker
// right before each start.
QThread *workerThread();
QThread *preparationThread();
// Tests: how children start, "posix_spawn" on macOS and "QProcess" elsewhere.
const char *launchBackend();
void setLaunchObserver(std::function<void()> observer);

}
