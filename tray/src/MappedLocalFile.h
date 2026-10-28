#pragma once
#include <QString>
#include <memory>

// A Windows original is never treated as a downloaded copy. Keep read handles
// on the checked path until the explicit open/reveal dispatch has returned.
namespace MappedLocalFile {
struct Guard;
struct Result {
    QString path, error;
    std::shared_ptr<Guard> guard;
    bool ok() const { return error.isEmpty() && !path.isEmpty() && bool(guard); }
};
bool safeDrivePath(const QString &path);
// Blocking bounded I/O: call on a worker, never from the GUI event loop.
Result verify(const QString &path, const QString &sha256, qint64 bytes);
}
