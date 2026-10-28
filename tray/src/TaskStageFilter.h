#pragma once
#include "TaskBoard.h"
#include "TaskRow.h"
#include "WorkspaceIcons.h"
#include <QAbstractButton>
#include <QPainter>

// The same stage filter on the project board and the Activity inspector.
class TaskTile : public QAbstractButton {
public:
    TaskTile(const QString &stage, QWidget *parent) : QAbstractButton(parent), m_stage(stage), m_colors(TaskColors::of(true)) {
        setObjectName(QStringLiteral("taskTile:") + stage); setCheckable(true); setFixedHeight(34); setMinimumWidth(48);
        setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Fixed); setCursor(Qt::PointingHandCursor); setAttribute(Qt::WA_Hover);
        setToolTip(TaskBoard::stageLabel(stage)); setCount(0);
    }
    void setCount(int count) {
        setProperty("count", count); setAccessibleName(QStringLiteral("%1, %2").arg(TaskBoard::stageLabel(m_stage)).arg(count)); update();
    }
    void setColors(const TaskColors &colors) { m_colors = colors; update(); }
    void setCompact(bool compact) { setProperty("compact", compact); update(); }
    void setDropTarget(bool on) { if (property("dropTarget").toBool() != on) { setProperty("dropTarget", on); update(); } }
    int expandedWidth() const {
        QFont label = font(); label.setPixelSize(12); label.setWeight(QFont::DemiBold);
        QFont count = font(); count.setPixelSize(15); count.setBold(true);
        return 38 + QFontMetrics(label).horizontalAdvance(TaskBoard::stageLabel(m_stage))
            + QFontMetrics(count).horizontalAdvance(QString::number(property("count").toInt())) + 20;
    }
    QSize sizeHint() const override { return {expandedWidth(), 34}; }
protected:
    void paintEvent(QPaintEvent *) override {
        QPainter p(this); p.setRenderHint(QPainter::Antialiasing);
        const QColor color = m_colors.stage(m_stage);
        const auto tint = [](QColor color, double share) { color.setAlphaF(share); return color; };
        p.setPen(Qt::NoPen); p.setBrush(m_colors.surface); p.drawRoundedRect(QRectF(rect()), 9, 9);
        if (isChecked()) { p.setBrush(tint(color, .17)); p.drawRoundedRect(QRectF(rect()), 9, 9); }
        const QColor edge = isChecked() ? tint(color, .45) : underMouse() ? m_colors.border : Qt::transparent;
        p.setPen(edge.alpha() ? QPen(edge, 1) : Qt::NoPen); p.setBrush(Qt::NoBrush);
        p.drawRoundedRect(QRectF(rect()).adjusted(.5, .5, -.5, -.5), 9, 9);
        if (property("dropTarget").toBool()) {
            p.setPen(QPen(color, 2, Qt::DashLine)); p.drawRoundedRect(QRectF(rect()).adjusted(3, 3, -3, -3), 7, 7);
        }
        const bool compact = property("compact").toBool();
        p.setPen(Qt::NoPen); p.setBrush(tint(color, .18)); p.drawEllipse(QRectF(compact ? 3 : 8, 4, 26, 26));
        workspaceIcon(TaskBoard::stageIcon(m_stage), color).paint(&p, QRect(compact ? 6 : 11, 7, 20, 20));
        QFont count = font(); count.setPixelSize(15); count.setBold(true); p.setFont(count); p.setPen(m_colors.fg);
        const QString number = QString::number(property("count").toInt());
        const int numberWidth = QFontMetrics(count).horizontalAdvance(number);
        p.drawText(QRect(width() - (compact ? 5 : 10) - numberWidth, 0, numberWidth, height()), Qt::AlignVCenter, number);
        if (compact) return;
        QFont label = font(); label.setPixelSize(12); label.setWeight(QFont::DemiBold); p.setFont(label);
        p.setPen(isChecked() ? m_colors.fg : m_colors.muted);
        const QRect text(38, 0, qMax(0, width() - 38 - numberWidth - 16), height());
        p.drawText(text, Qt::AlignVCenter, QFontMetrics(label).elidedText(TaskBoard::stageLabel(m_stage), Qt::ElideRight, text.width()));
    }
private:
    QString m_stage;
    TaskColors m_colors;
};
