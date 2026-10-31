#pragma once

#include "OffGuiThread.h"
#include "Settings.h"
#include "SettingsJson.h"

#include <QByteArray>
#include <QJsonDocument>
#include <QSettings>
#include <QString>
#include <QVariant>

// Stores a JSON value in QSettings as another process or an earlier build would,
// then parses it off the GUI thread, as the tray does before its windows exist
// (SettingsJson.h). A widget the test creates next reads it without a parse; a
// GUI-thread read of a value nobody parsed fails the suite.
inline void setJsonSetting(const QString &key, const QVariant &value)
{
    // A value with a file of its own (SettingsJson.h) is written there, as the
    // tray would, and read back from the file.
    if (SettingsJson::inFile(key)) {
        const QByteArray bytes = value.toByteArray();
        offGuiThread([&key, &bytes] {
            SettingsJson::store(key, bytes, QJsonDocument::fromJson(bytes));
            SettingsJson::flush();
            SettingsJson::forget(key);
            SettingsJson::document(key);
            return true;
        });
        return;
    }
    QSettings().setValue(key, value);
    offGuiThread([&key] { SettingsJson::document(key); return true; });
}

// The stored bytes of a JSON value, from its file or the INI.
inline QByteArray jsonSetting(const QString &key)
{
    SettingsJson::flush();
    return SettingsJson::bytes(key);
}

// QSettings::remove() for a test, a JSON value's own file included.
inline void removeSetting(const QString &key)
{
    if (SettingsJson::inFile(key)) { SettingsJson::remove(key); SettingsJson::flush(); return; }
    QSettings().remove(key);
}

// QSettings::clear() for a test, the JSON values' own files included.
inline void clearSettings()
{
    SettingsJson::reset();
    Settings::flush();QSettings().clear();
}
