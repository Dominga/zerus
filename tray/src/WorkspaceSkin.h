#pragma once
#include <QColor>
#include <QDateTime>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QHash>
#include <QJsonDocument>
#include <QJsonObject>
#include <QList>
#include <QRegularExpression>
#include <QSettings>
#include <QString>
#include <tuple>
#include <utility>

// WhiteWh fork: switchable workspace skins. A skin names the colors the
// workspace stylesheet is built from, for the dark and the light theme, and may
// append its own Qt stylesheet. Built-in skins live here; user skins are JSON
// files in the "skins" folder beside the settings file, and a running Zerus
// reloads the selected one when it changes. Schema 1:
//   {"schema":1,"name":"…","dark":{token:color…},"light":{…},"stylesheet":"…",
//    "views":{"newSession":"classic|dense","sessions":"classic|cards"},"window":{"dialogs":"native|frameless","main":"native|frameless"},
//    "viewColors":{"dark":{name:color…},"light":{…}}}
// viewColors name the colors of views, such as the dense New session, apart
// from the workspace tokens, and override the design tokens below (U25), for
// instance "text3" or a role's "roleCoder".
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
    QString dialogFrame = QStringLiteral("native"), mainFrame = QStringLiteral("native");
    QHash<QString, QString> darkViewColors, lightViewColors;
    // A view color the skin names, or an empty string for the view's own default.
    QString named(bool darkTheme, const QString &name) const { return (darkTheme ? darkViewColors : lightViewColors).value(name); }
    const Palette &palette(bool darkTheme) const { return darkTheme ? dark : light; }
    QString view(const QString &screen) const { return views[screen].toString(QStringLiteral("classic")); }
    bool framelessDialogs() const { return dialogFrame == QLatin1String("frameless"); }
    bool framelessMain() const { return mainFrame == QLatin1String("frameless"); }
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
    // Screens are named like newSession, views like dense.
    static const QRegularExpression screenName(QStringLiteral("\\A[a-z][A-Za-z0-9]{0,31}\\z"));
    static const QRegularExpression viewName(QStringLiteral("\\A[a-z][a-z0-9-]{0,31}\\z"));
    const auto views = data["views"].toObject();
    for (auto it = views.begin(); it != views.end(); ++it)
        if (screenName.match(it.key()).hasMatch() && viewName.match(it.value().toString()).hasMatch()) skin.views[it.key()] = it.value().toString();
    if (data["window"].toObject()["dialogs"].toString() == QLatin1String("frameless")) skin.dialogFrame = QStringLiteral("frameless");
    if (data["window"].toObject()["main"].toString() == QLatin1String("frameless")) skin.mainFrame = QStringLiteral("frameless");
    static const QRegularExpression colorPlain(QStringLiteral("\\A(?:#[0-9A-Fa-f]{3,8}|[A-Za-z]{3,30})\\z"));
    for (const auto &[key, colors] : {std::pair{QStringLiteral("dark"), &skin.darkViewColors}, std::pair{QStringLiteral("light"), &skin.lightViewColors}}) {
        const auto values = data["viewColors"].toObject()[key].toObject();
        for (auto it = values.begin(); it != values.end() && colors->size() < 128; ++it) {
            const auto value = it.value().toString().trimmed();
            if (screenName.match(it.key()).hasMatch() && colorPlain.match(value).hasMatch() && QColor::isValidColorName(value)) colors->insert(it.key(), value);
        }
    }
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

// This fork's own look: the palette of the design prototype, the dense New
// session and session cards. Its light workspace palette keeps the Zerus
// colors; its view colors carry the prototype's light tokens.
inline const char *whiteWhJson()
{
    return R"({"schema":1,"name":"Compact",
"dark":{"background":"#14181d","foreground":"#e6e8eb","muted":"#9aa3ad","surface":"#1b1f26","border":"#2c323b","accent":"#7dd3b0",
        "accentText":"#0f1a16","accentHover":"#a8e6cc","hover":"#222830","selected":"#1b2621","inset":"#151920"},
"light":{"background":"#f5f7f9","foreground":"#1a2733","muted":"#627082","surface":"#ffffff","border":"#dce2e8","accent":"#167357",
         "accentText":"#ffffff","accentHover":"#115d46","hover":"#edf2f5","selected":"#e0f0e9","inset":"#f3f6f8"},
"views":{"newSession":"dense","sessions":"cards","composer":"compact","inspector":"foldable","activity":"bubbles"},"window":{"dialogs":"frameless","main":"frameless"},
"viewColors":{"dark":{"dialog":"#1b1f26","field":"#11151a","selectedAgent":"#1f3a31","segmentOn":"#26443a","buttonNeutral":"#232830",
  "disabled":"#2a3038","border":"#2c323b","fieldBorder":"#39404a","neutralBorder":"#4a525d","track":"#2c323b","foreground":"#e6e8eb",
  "text2":"#c4cad1","text3":"#aab1ba","muted":"#9aa3ad","label":"#8f97a2","dim":"#7b8590","accent":"#7dd3b0","accentText":"#0f1a16",
  "warn":"#e8a75a","danger":"#ef7d6d","folder":"#7fb6e0",
  "card":"#181c22","cardBorder":"#232932","cardHover":"#1e232b","cardSelected":"#1b2621","cardSelectedBorder":"#2f5a4a"},
 "light":{"field":"#ffffff","selectedAgent":"#e0f0e9","segmentOn":"#e0f0e9","border":"#d6dde4","text2":"#2c3a47","text3":"#415060",
  "muted":"#5d6b7c","label":"#647181","dim":"#6f7b88","warn":"#9a5a0c","danger":"#c4402f","folder":"#2f6ea3","hover":"#edf1f4",
  "card":"#ffffff","cardBorder":"#e1e6eb","cardHover":"#eef2f5","cardSelected":"#e7f3ee","cardSelectedBorder":"#9fd3bd"}},
"stylesheet":"QWidget#socratiCodePanel{background:rgba(199,162,240,.08);border:1px solid rgba(199,162,240,.38);border-radius:10px}QLabel#inspectorPanelTitle{font-weight:600}QWidget#socratiCodePanel QLabel#inspectorPanelTitle{color:#b48ee6}QFrame#composerCard{background:rgba(128,138,150,.07);border:1px solid rgba(128,138,150,.30);border-radius:12px}QFrame#composerCard QPlainTextEdit#messageInput{border:0;background:transparent;padding:0}QFrame#composerCard QPlainTextEdit#messageInput:focus{border:0}QFrame#composerCard QPushButton#attachMessageFile{border:0;background:transparent;border-radius:9px}QFrame#composerCard QPushButton#sendMessage{border-radius:9px;padding:0}QPushButton#sessionModelSettings{border:1px solid transparent;background:transparent}"})";
}

inline QList<Skin> builtins()
{
    Skin base{"zerus", "Zerus", {}, {}, true, zerus(true), zerus(false), {}, QStringLiteral("native")};
    return {base, parse(QJsonDocument::fromJson(whiteWhJson()).object(), "whitewh", {}, true)};
}

// U25: the design tokens of the owner's prototype (Zerus-prototype.html,
// `.app` and `.app.light`): name, dark, light. The role palette `--r-<role>`
// is role<Name>. A skin's viewColors override any of them by name.
inline const QList<std::tuple<const char *, const char *, const char *>> &designTokens()
{
    static const QList<std::tuple<const char *, const char *, const char *>> values{
        {"bg", "#14181d", "#f5f7f9"}, {"surface", "#1b1f26", "#ffffff"}, {"card", "#181c22", "#ffffff"},
        {"cardBorder", "#232932", "#e1e6eb"}, {"cardHover", "#1e232b", "#eef2f5"}, {"cardSel", "#1b2621", "#e7f3ee"},
        {"cardSelBorder", "#2f5a4a", "#9fd3bd"}, {"field", "#11151a", "#ffffff"}, {"border", "#2c323b", "#d6dde4"},
        {"selAgent", "#1f3a31", "#e0f0e9"}, {"segOn", "#26443a", "#e0f0e9"}, {"hover", "#222830", "#edf1f4"},
        {"accent", "#7dd3b0", "#167357"}, {"accentText", "#0f1a16", "#ffffff"}, {"accentHover", "#a8e6cc", "#115d46"},
        {"accentMuted", "#5c9e86", "#4f8f78"}, {"warn", "#e8a75a", "#9a5a0c"}, {"danger", "#ef7d6d", "#c4402f"},
        {"folder", "#7fb6e0", "#2f6ea3"}, {"violet", "#b79cff", "#6f4fd8"}, {"fg", "#e6e8eb", "#1a2733"},
        {"text2", "#c4cad1", "#2c3a47"}, {"text3", "#aab1ba", "#415060"}, {"muted", "#9aa3ad", "#5d6b7c"},
        {"label", "#8f97a2", "#647181"}, {"dim", "#7b8590", "#6f7b88"}, {"tile", "#0f1318", "#eef2f5"},
        {"page", "#0e1115", "#e4e9ee"}, {"bubble", "#1f3a31", "#e0f0e9"}, {"bubbleBorder", "#2f5a4a", "#9fd3bd"},
        {"roleLead", "#b79cff", "#6f4fd8"}, {"roleArchitect", "#8e9bff", "#3d4bc4"}, {"roleBackend", "#6fa8ff", "#2f66c9"},
        {"roleFrontend", "#f08bb4", "#b83a72"}, {"roleTester", "#56c8e0", "#137f95"}, {"roleReviewer", "#d9b38c", "#8a5a2b"},
        {"roleIntegrator", "#a3b1c6", "#4d5d75"}, {"roleResearcher", "#b9d36b", "#5f7a12"}, {"roleSecurity", "#e48de0", "#a2349c"},
        {"roleSecretary", "#7fcf9a", "#2f7d4a"}, {"roleCoder", "#e8a75a", "#a35f10"}, {"roleQa", "#4fd1c5", "#0f7a72"},
        {"roleDesigner", "#ef7d8d", "#b8384b"}, {"roleLeadgpt", "#9aa5ff", "#4652c4"}, {"roleExpert", "#e0b070", "#8a5d16"},
        {"roleProexpert", "#ef7d6d", "#b8402f"}};
    return values;
}

// The roles of the palette as the catalog names them, longest first where one
// contains another (proexpert before expert, leadgpt before lead).
inline QStringList paletteRoles()
{
    return {"proexpert", "leadgpt", "architect", "backend", "frontend", "tester", "reviewer", "integrator", "researcher",
            "security", "secretary", "coder", "designer", "expert", "lead", "qa"};
}

// A design token of `skin`: its viewColors (also under the dense dialog's
// older names), then its palette for the workspace colors, then the prototype.
inline QColor color(const Skin &skin, bool dark, const QString &name)
{
    static const QHash<QString, QString> older{{"fg", "foreground"}, {"selAgent", "selectedAgent"}, {"segOn", "segmentOn"},
                                               {"cardSel", "cardSelected"}, {"cardSelBorder", "cardSelectedBorder"}};
    static const QHash<QString, QString> workspace{{"bg", "background"}, {"surface", "surface"}, {"fg", "foreground"},
                                                   {"muted", "muted"}, {"border", "border"}, {"accent", "accent"},
                                                   {"accentText", "accentText"}, {"accentHover", "accentHover"}, {"hover", "hover"}};
    QString value = skin.named(dark, name);
    if (value.isEmpty() && older.contains(name)) value = skin.named(dark, older.value(name));
    if (value.isEmpty() && workspace.contains(name)) {
        Palette copy = skin.palette(dark);
        if (const QString *found = token(copy, workspace.value(name))) value = *found;
    }
    if (value.isEmpty())
        for (const auto &[token, d, l] : designTokens())
            if (name == QLatin1String(token)) { value = QString::fromLatin1(dark ? d : l); break; }
    return QColor(value);
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

// The selected skin for painting: current(), read again only when the
// selection or the user skin's file changes, so a delegate may ask every frame.
inline const Skin &active()
{
    static Skin cached;
    static QString cachedId;
    static QDateTime cachedStamp;
    static bool valid = false;
    const QString id = currentId();
    const auto stamp = [] (const Skin &skin) { return skin.path.isEmpty() ? QDateTime() : QFileInfo(skin.path).lastModified(); };
    if (!valid || id != cachedId || stamp(cached) != cachedStamp) {
        cached = current(); cachedId = id; cachedStamp = stamp(cached); valid = true;
    }
    return cached;
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
    const auto colors = [](const QHash<QString, QString> &values) {
        QJsonObject result;
        for (auto it = values.begin(); it != values.end(); ++it) result[it.key()] = it.value();
        return result;
    };
    QJsonObject data{{"schema", 1}, {"name", trimmed.isEmpty() ? skin.name : trimmed}, {"dark", toJson(skin.dark)},
                     {"light", toJson(skin.light)}, {"views", skin.views}, {"window", QJsonObject{{"dialogs", skin.dialogFrame}, {"main", skin.mainFrame}}},
                     {"viewColors", QJsonObject{{"dark", colors(skin.darkViewColors)}, {"light", colors(skin.lightViewColors)}}},
                     {"stylesheet", skin.stylesheet}};
    QFile output(QDir(directory()).filePath(id + QStringLiteral(".json")));
    if (!output.open(QIODevice::NewOnly | QIODevice::WriteOnly) || output.write(QJsonDocument(data).toJson()) < 0) {
        if (error) *error = QObject::tr("Could not write %1.").arg(QDir::toNativeSeparators(output.fileName()));
        return {};
    }
    return id;
}
}
