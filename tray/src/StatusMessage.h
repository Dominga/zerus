#pragma once

#include <QLabel>
#include <QPainter>

// Keep feedback on one line; retain the full text for tooltips and accessibility.
class StatusMessage : public QLabel {
public:
    explicit StatusMessage(QWidget *parent = nullptr) : QLabel(parent) {
        setObjectName("notice"); setTextFormat(Qt::PlainText);
        setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
    }
protected:
    void paintEvent(QPaintEvent *event) override {
        // A status with emphasis is drawn as rich text; a plain one elides.
        if (textFormat() == Qt::RichText) return QLabel::paintEvent(event);
        QPainter painter(this); painter.setPen(palette().color(QPalette::WindowText));
        painter.drawText(contentsRect(), Qt::AlignVCenter | Qt::AlignLeft,
            fontMetrics().elidedText(text().simplified(), Qt::ElideRight, contentsRect().width()));
    }
};
