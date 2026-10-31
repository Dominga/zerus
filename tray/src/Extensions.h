#pragma once
#include "Settings.h"
#include <QCoreApplication>
#include <QList>
#include <QSettings>
#include <QString>

// WhiteWh fork: optional extensions, integrations with third-party tools that
// stay outside the Zerus core. Each is switched in Settings → Extensions and
// is off unless the build turns extensions on by default
// (ZERUS_EXTENSIONS_ON_BY_DEFAULT: the WhiteWh fork builds). The CLI side of
// an extension is a cargo feature of the same id (src/extensions.rs).
namespace Extensions {
struct Extension {
    QString id, name, description;
};

inline QList<Extension> available()
{
    return {
        {QStringLiteral("socraticode"), QStringLiteral("SocratiCode"),
         QCoreApplication::translate("Extensions",
             "Semantic code search for agents (github.com/giancarloerra/socraticode). Details shows the index of a session's folder "
             "and installs, starts and connects SocratiCode. It needs an hgs built with the socraticode feature.")},
    };
}

inline bool enabledByDefault()
{
#ifdef ZERUS_EXTENSIONS_ON_BY_DEFAULT
    return true;
#else
    return false;
#endif
}

inline QString key(const QString &id) { return QStringLiteral("extensions/") + id; }
inline bool enabled(const QString &id) { return Settings::value(key(id), enabledByDefault()).toBool(); }
inline void setEnabled(const QString &id, bool on) { Settings::set(key(id), on); }
}
