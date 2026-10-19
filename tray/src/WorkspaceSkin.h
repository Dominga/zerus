#pragma once
#include <QColor>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonDocument>
#include <QJsonObject>
#include <QList>
#include <QRegularExpression>
#include <QSettings>
#include <QString>
#include <utility>

// WhiteWh fork: switchable workspace skins. A skin names the colors the
// workspace stylesheet is built from, for the dark and the light theme, and may
// append its own Qt stylesheet. Built-in skins live here; user skins are JSON
// files in the "skins" folder beside the settings file, and a running Zerus
// reloads the selected one when it changes. Schema 1:
//   {"schema":1,"name":"…","dark":{token:color…},"light":{…},"stylesheet":"…",
//    "views":{"newSession":"classic|dense"},"window":{"dialogs":"native|frameless"}}
// A view arranges a screen's widgets; its logic stays the same. Missing or
// invalid tokens keep the Zerus colors, and unknown views the classic ones.
namespace WorkspaceSkin {

struct Palette {
    QString background, foreground, muted, surface, border, accent, accentText, accentHover, hover, selected, inset;
};

struct Skin {
    QString id, name, path, stylesheet;
    bool builtin = false;
    Palette dark, light;
    QJsonObject views;
    QString dialogFrame = QStringLiteral("native");
    const Palette &palette(bool darkTheme) const { return darkTheme ? dark : light; }
    QString view(const QString &screen) const { return views[screen].toString(QStringLiteral("classic")); }
    bool framelessDialogs() const { return dialogFrame == QLatin1String("frameless"); }
};

// The Zerus colors, exactly as the workspace used them before skins.
inline Palette zerus(bool dark)
{
    return dark ? Palette{"#161b21", "#e8edf4", "#a2adbc", "#1c2229", "#333c47", "#8bdfc0", "#12372b", "#aae9d2", "#2b3540", "#233d35", "#151b21"}
                : Palette{"#f5f7f9", "#1a2733", "#627082", "#ffffff", "#dce2e8", "#167357", "#ffffff", "#115d46", "#edf2f5", "#e0f0e9", "#f3f6f8"};
}

inline QStringList tokens()
{
    return {"background", "foreground", "muted", "surface", "border", "accent", "accentText", "accentHover", "hover", "selected", "inset"};
}

inline QString *token(Palette &palette, const QString &name)
{
    QString *values[] = {&palette.background, &palette.foreground, &palette.muted, &palette.surface, &palette.border, &palette.accent,
                         &palette.accentText, &palette.accentHover, &palette.hover, &palette.selected, &palette.inset};
    const int index = tokens().indexOf(name);
    return index < 0 ? nullptr : values[index];
}

inline QJsonObject toJson(const Palette &palette)
{
    QJsonObject result;
    Palette copy = palette;
    for (const auto &name : tokens()) result[name] = *token(copy, name);
    return result;
}

inline Skin parse(const QJsonObject &data, const QString &id, const QString &path, bool builtin)
{
    Skin skin{id, data["name"].toString().trimmed().left(80), path, data["stylesheet"].toString().left(256 * 1024), builtin, zerus(true), zerus(false), {}, QStringLiteral("native")};
    if (skin.name.isEmpty()) skin.name = id;
    static const QRegularExpression viewName(QStringLiteral("\\A[a-z][a-z0-9-]{0,31}\\z"));
    const auto views = data["views"].toObject();
    for (auto it = views.begin(); it != views.end(); ++it)
        if (viewName.match(it.key()).hasMatch() && viewName.match(it.value().toString()).hasMatch()) skin.views[it.key()] = it.value().toString();
    if (data["window"].toObject()["dialogs"].toString() == QLatin1String("frameless")) skin.dialogFrame = QStringLiteral("frameless");
    for (const auto &[key, palette] : {std::pair{QStringLiteral("dark"), &skin.dark}, std::pair{QStringLiteral("light"), &skin.light}}) {
        const auto values = data[key].toObject();
        for (const auto &name : tokens()) {
            const auto value = values[name].toString().trimmed();
            // Plain color names only: they are pasted into a stylesheet.
            static const QRegularExpression plain(QStringLiteral("\\A(?:#[0-9A-Fa-f]{3,8}|[A-Za-z]{3,30})\\z"));
            if (plain.match(value).hasMatch() && QColor::isValidColorName(value)) *token(*palette, name) = value;
        }
    }
    return skin;
}

// The fork of Zerus that this fork's own look grows from; it starts identical.
inline const char *whiteWhJson()
{
    return R"({"schema":1,"name":"WhiteWh",
"dark":{"background":"#161b21","foreground":"#e8edf4","muted":"#a2adbc","surface":"#1c2229","border":"#333c47","accent":"#8bdfc0",
        "accentText":"#12372b","accentHover":"#aae9d2","hover":"#2b3540","selected":"#233d35","inset":"#151b21"},
"light":{"background":"#f5f7f9","foreground":"#1a2733","muted":"#627082","surface":"#ffffff","border":"#dce2e8","accent":"#167357",
         "accentText":"#ffffff","accentHover":"#115d46","hover":"#edf2f5","selected":"#e0f0e9","inset":"#f3f6f8"},
"views":{"newSession":"dense"},"window":{"dialogs":"frameless"},
"stylesheet":""})";
}

inline QList<Skin> builtins()
{
    Skin base{"zerus", "Zerus", {}, {}, true, zerus(true), zerus(false), {}, QStringLiteral("native")};
    return {base, parse(QJsonDocument::fromJson(whiteWhJson()).object(), "whitewh", {}, true)};
}

// The user skins folder, beside the desktop settings file.
inline QString directory()
{
    return QFileInfo(QSettings().fileName()).absolutePath() + QStringLiteral("/skins");
}

inline QList<Skin> available()
{
    auto result = builtins();
    QStringList ids;
    for (const auto &skin : result) ids << skin.id;
    const auto files = QDir(directory()).entryInfoList({QStringLiteral("*.json")}, QDir::Files, QDir::Name);
    for (const auto &file : files.mid(0, 200)) {
        const auto id = file.completeBaseName().toLower();
        static const QRegularExpression valid(QStringLiteral("\\A[a-z0-9][a-z0-9_-]{0,63}\\z"));
        if (ids.contains(id) || !valid.match(id).hasMatch() || file.size() > 512 * 1024) continue;
        QFile input(file.absoluteFilePath());
        if (!input.open(QIODevice::ReadOnly)) continue;
        const auto document = QJsonDocument::fromJson(input.readAll());
        if (!document.isObject() || document.object()["schema"].toInt() != 1) continue;
        result << parse(document.object(), id, file.absoluteFilePath(), false);
        ids << id;
    }
    return result;
}

inline QString currentId() { return QSettings().value(QStringLiteral("workspace/skin"), QStringLiteral("zerus")).toString(); }

inline Skin current()
{
    const auto skins = available();
    for (const auto &skin : skins) if (skin.id == currentId()) return skin;
    return skins.first();
}

// Writes a copy of `skin` named `name` into the user folder and returns its id,
// or an empty id with `error` set. The copy is selected by the caller.
inline QString duplicate(const Skin &skin, const QString &name, QString *error)
{
    const auto trimmed = name.trimmed().left(80);
    auto base = trimmed.toLower();
    base.replace(QRegularExpression(QStringLiteral("[^a-z0-9_-]+")), QStringLiteral("-"));
    base.remove(QRegularExpression(QStringLiteral("\\A-+|-+\\z")));
    if (base.isEmpty()) base = QStringLiteral("skin");
    base = base.left(56);
    QStringList taken;
    for (const auto &existing : available()) taken << existing.id;
    auto id = base;
    for (int n = 2; taken.contains(id); ++n) id = base + '-' + QString::number(n);
    if (!QDir().mkpath(directory())) { if (error) *error = QObject::tr("Could not create %1.").arg(QDir::toNativeSeparators(directory())); return {}; }
    QJsonObject data{{"schema", 1}, {"name", trimmed.isEmpty() ? skin.name : trimmed}, {"dark", toJson(skin.dark)},
                     {"light", toJson(skin.light)}, {"views", skin.views}, {"window", QJsonObject{{"dialogs", skin.dialogFrame}}},
                     {"stylesheet", skin.stylesheet}};
    QFile output(QDir(directory()).filePath(id + QStringLiteral(".json")));
    if (!output.open(QIODevice::NewOnly | QIODevice::WriteOnly) || output.write(QJsonDocument(data).toJson()) < 0) {
        if (error) *error = QObject::tr("Could not write %1.").arg(QDir::toNativeSeparators(output.fileName()));
        return {};
    }
    return id;
}
}
