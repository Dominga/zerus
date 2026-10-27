#pragma once

#include "OffGuiThread.h"
#include "SettingsJson.h"

#include <QSettings>
#include <QString>
#include <QVariant>

// Stores a JSON value in QSettings as another process or an earlier build would,
// then parses it off the GUI thread, as the tray does before its windows exist
// (SettingsJson.h). A widget the test creates next reads it without a parse; a
// GUI-thread read of a value nobody parsed fails the suite.
inline void setJsonSetting(const QString &key, const QVariant &value)
{
    QSettings().setValue(key, value);
    offGuiThread([&key] { SettingsJson::document(key); return true; });
}
