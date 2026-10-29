// Records shortcuts-native.json (client/src/store): run on Linux and on macOS
// with Qt 6 and merge the two outputs under "linux" and "macos".
// Records how Qt writes key sequences on this platform: for each portable
// chord, its NativeText and PortableText, and whether Qt parsed it.
#include <QGuiApplication>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QKeySequence>
#include <cstdio>

int main(int argc, char **argv) {
    qputenv("QT_QPA_PLATFORM", "offscreen");
    QGuiApplication app(argc, argv);
    const QStringList chords{
        "Ctrl+1", "Ctrl+2", "Ctrl+3", "Ctrl+4", "Ctrl+5", "Ctrl+,", "Ctrl+K", "F1", "Ctrl+/",
        "Alt+Shift+Down", "Alt+Shift+Up", "Ctrl+N", "F2", "Ctrl+F", "Ctrl+R", "F5", "Ctrl+I", "F6", "Shift+F6",
        "Ctrl+Return", "Ctrl+Shift+Return", "Ctrl+=", "Ctrl++", "Ctrl+-", "Ctrl+0",
        "Alt+Down", "Ctrl+Alt+Down", "Ctrl+PgDown", "Alt+Up", "Ctrl+Alt+Up", "Ctrl+PgUp",
        "Meta+Tab", "Ctrl+Tab", "Meta+Shift+Tab", "Ctrl+Shift+Tab", "Meta+`", "Ctrl+`",
        "Meta+X, Meta+S", "Ctrl+X, Ctrl+S", "Ctrl+Enter", "Ctrl+Del", "Ctrl+Backspace", "Ctrl+Esc",
        "Ctrl+Space", "Ctrl+Home", "Ctrl+End", "Ctrl+Ins", "Ctrl+Left", "Ctrl+Right", "Meta+Alt+Shift+Ctrl+K",
        "Ctrl+[", "Ctrl+]", "Ctrl+;", "Ctrl+'", "Ctrl+.", "Ctrl+\\", "Menu", "Shift+F10", "Ctrl+F12",
    };
    QJsonArray out;
    for (const QString &chord : chords) {
        const QKeySequence seq = QKeySequence::fromString(chord, QKeySequence::PortableText);
        QJsonObject row;
        row["portable"] = chord;
        row["count"] = seq.count();
        row["native"] = seq.toString(QKeySequence::NativeText);
        row["roundtrip"] = seq.toString(QKeySequence::PortableText);
        out.append(row);
    }
    std::fputs(QJsonDocument(out).toJson(QJsonDocument::Indented).constData(), stdout);
    return 0;
}
