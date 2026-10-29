// Compiled into Windows test programs that use Qt GUI (tests/CMakeLists.txt). The
// offscreen platform reads the font files in QT_QPA_FONTDIR (set by CTest) but has
// no Windows font configuration: its default "Sans Serif" falls back to an
// arbitrary family. Tests use the Windows UI font, as the windows platform does.
#include <QCoreApplication>
#include <QFont>
#include <QGuiApplication>

namespace {
// #542: a test program run directly (a team's qt-test runner, a developer's
// shell) has no QT_QPA_FONTDIR unless the caller sets one, and offscreen then
// draws every glyph as a box of one width, so layout checks fail. Without one,
// use the Windows fonts folder, as CTest does. This runs before main(), before
// the platform reads its fonts; a folder the caller sets is kept.
[[maybe_unused]] const bool windowsFontDirectory = [] {
    if (qEnvironmentVariableIsEmpty("QT_QPA_FONTDIR") && !qEnvironmentVariableIsEmpty("SystemRoot"))
        qputenv("QT_QPA_FONTDIR", (qEnvironmentVariable("SystemRoot") + QStringLiteral("\\Fonts")).toLocal8Bit());
    return true;
}();

void useWindowsUiFont()
{
    if (qobject_cast<QGuiApplication *>(QCoreApplication::instance()))
        QGuiApplication::setFont(QFont(QStringLiteral("Segoe UI"), 9));
}
} // namespace

Q_COREAPP_STARTUP_FUNCTION(useWindowsUiFont)
