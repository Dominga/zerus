#pragma once

#include <QByteArray>
#include <QCryptographicHash>
#include <QHash>
#include <QMutex>

// SHA-256 of a short identity, remembered: rows hash the same identities on
// every listing, and on macOS each QCryptographicHash call set up an OpenSSL
// digest (3.9 % of the GUI thread in a 2026-10-27 sample). Bounded; any thread.
namespace IdentityHash {
inline QByteArray sha256(const QByteArray &input)
{
    static QMutex mutex;
    static QHash<QByteArray, QByteArray> cache;
    {
        QMutexLocker lock(&mutex);
        if (const auto found = cache.constFind(input); found != cache.cend()) return *found;
    }
    const QByteArray digest = QCryptographicHash::hash(input, QCryptographicHash::Sha256);
    QMutexLocker lock(&mutex);
    if (cache.size() >= 16384) cache.clear();
    cache.insert(input, digest);
    return digest;
}
}
