#pragma once
#include "Settings.h"
#include <QSettings>
#include <QVBoxLayout>
#include <QtGlobal>

// In a wide window Activity reads like a chat: its transcript, questions and
// message field share one centered column. Widths are pixels at 100% content
// scale, so enlarged content keeps the same line length.
namespace ActivityWidth {
constexpr int Minimum = 480;
constexpr int Maximum = 2400;
constexpr int Default = 900;
inline int clamp(int width) { return qBound(Minimum, width, Maximum); }
inline int width()
{
    bool ok = false; const int width = Settings::value("workspace/activityWidth", Default).toInt(&ok);
    return ok ? clamp(width) : Default;
}
inline bool fullWidth() { return Settings::value("workspace/activityFullWidth", false).toBool(); }
inline void setWidth(int width) { Settings::set("workspace/activityWidth", clamp(width)); }
inline void setFullWidth(bool full) { Settings::set("workspace/activityFullWidth", full); }
// The column in pixels at a content scale, or 0 when Activity fills its pane.
inline int column(double scale) { return fullWidth() ? 0 : qRound(width() * scale); }
// The space on each side of a column centered in the available width.
inline int inset(int available, int column) { return column > 0 && available > column ? (available - column) / 2 : 0; }

// Stacks its items in the centered column. Its parent keeps the full width
// for siblings that span the pane, such as the transcript's scroll bar.
class ColumnLayout final : public QVBoxLayout {
public:
    ColumnLayout() { setContentsMargins(0, 0, 0, 0); setSpacing(0); }
    void setColumnWidth(int width) { if (width == m_width) return; m_width = width; invalidate(); }
    int columnWidth() const { return m_width; }
    void setGeometry(const QRect &rect) override
    {
        const int side = inset(rect.width(), m_width);
        QVBoxLayout::setGeometry(rect.adjusted(side, 0, -side, 0));
    }
    int heightForWidth(int width) const override { return QVBoxLayout::heightForWidth(width - 2 * inset(width, m_width)); }
    int minimumHeightForWidth(int width) const override { return QVBoxLayout::minimumHeightForWidth(width - 2 * inset(width, m_width)); }
private:
    int m_width = 0;
};
}
