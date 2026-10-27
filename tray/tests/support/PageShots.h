#pragma once
#include <QApplication>
#include <QDir>
#include <QPalette>
#include <QTest>
#include <QWidget>

#include "WorkspaceSkin.h"

// U25 part 7: the Projects, Machines and Agent accounts pages as the window
// shows them - the skin's tokens for the page, its fields, buttons and lists
// on top of the page's own sheet - saved to ZERUS_TEAM_SHOTS as <name>.png.
namespace PageShots {

inline QString windowSheet(bool dark)
{
    const auto c = [dark](const char *name) { return WorkspaceSkin::color(WorkspaceSkin::active(), dark, QString::fromLatin1(name)).name(QColor::HexArgb); };
    return QStringLiteral(
        "QWidget{background:%1;color:%2;font-size:13px;}"
        "QLabel{background:transparent;} QLabel#muted{color:%3;} QLabel#heading{font-size:18px;font-weight:600;}"
        "QLineEdit,QSpinBox,QComboBox,QPlainTextEdit{background:%4;color:%2;border:1px solid %5;border-radius:7px;padding:5px 8px;}"
        "QLineEdit:disabled,QSpinBox:disabled{color:%6;}"
        "QPushButton{background:%7;color:%8;border:1px solid %5;border-radius:7px;padding:4px 11px;min-height:18px;}"
        "QPushButton:disabled{color:%6;}"
        "QPushButton:flat{background:transparent;border:0;}"
        "QListWidget,QTreeWidget,QTableWidget{background:transparent;border:0;outline:0;}"
        "QHeaderView::section{background:transparent;color:%3;border:0;border-bottom:1px solid %9;padding:5px 7px;}"
        "QTreeWidget::item,QTableWidget::item{padding:6px;border-bottom:1px solid %9;}"
        "QTreeWidget::item:selected,QTableWidget::item:selected{background:%10;color:%2;}"
        "QCheckBox{background:transparent;}"
        "QSpinBox::up-button,QSpinBox::down-button{width:0;border:0;}")
        .arg(c("bg"), c("fg"), c("muted"), c("field"), c("border"), c("dim"), c("surface"), c("text2"), c("cardBorder"))
        .arg(c("cardSel"));
}

// Styles `page` for the theme (its own setTheme first), waits for it to lay
// out and saves the shot; nothing without ZERUS_TEAM_SHOTS.
template <typename Page>
inline void shot(Page &page, bool dark, const QString &name)
{
    const auto dir = qEnvironmentVariable("ZERUS_TEAM_SHOTS");
    if (dir.isEmpty()) return;
    page.setTheme(dark);
    const QString own = page.styleSheet();
    page.setStyleSheet(windowSheet(dark) + own);
    QTest::qWait(150);
    page.grab().save(QDir(dir).filePath(name + QStringLiteral(".png")));
    page.setStyleSheet(own);
}

}   // namespace PageShots
