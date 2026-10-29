#pragma once
#include <QColor>
#include <QFontMetrics>
#include <QFrame>
#include <QHBoxLayout>
#include <QJsonArray>
#include <QJsonObject>
#include <QLabel>
#include <QPushButton>
#include <QSet>
#include <QVBoxLayout>
#include <functional>

// WhiteWh fork, U35: the messages hgs holds for a session until its agent takes
// the previous one (the inspection's held_inputs), one row each with Cancel,
// above a composer. hgs sends them in order; a row goes once it is sent.
class HeldInputBar : public QFrame {
public:
    explicit HeldInputBar(QWidget *parent = nullptr) : QFrame(parent)
    {
        setObjectName("heldInputBar");
        auto *layout = new QVBoxLayout(this);
        layout->setContentsMargins(10, 6, 10, 6);
        layout->setSpacing(4);
        hide();
    }
    // Cancels one held message; its row stays until the answer.
    std::function<void(const QString &requestId, const QString &text)> cancel;
    // `agent` names who takes the message: "the lead", "Claude".
    void setHeld(const QJsonArray &held, const QString &agent)
    {
        QStringList ids;
        for (const auto &value : held) ids << value.toObject().value("request_id").toString();
        if (ids == m_ids && agent == m_agent) { setVisible(!ids.isEmpty()); return; }
        m_ids = ids; m_agent = agent;
        m_cancelling.intersect(QSet<QString>(ids.cbegin(), ids.cend()));
        while (auto *item = layout()->takeAt(0)) { if (auto *w = item->widget()) w->deleteLater(); delete item; }
        for (const auto &value : held) {
            const auto item = value.toObject();
            const QString id = item.value("request_id").toString(), text = item.value("text").toString();
            auto *row = new QWidget; row->setObjectName("heldInput"); row->setProperty("requestId", id);
            auto *h = new QHBoxLayout(row); h->setContentsMargins(0, 0, 0, 0); h->setSpacing(8);
            auto *label = new QLabel(tr("Queued until %1 takes your previous message").arg(agent));
            label->setObjectName("heldInputText"); label->setToolTip(text);
            label->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
            h->addWidget(label, 1);
            auto *preview = new QLabel; preview->setObjectName("heldInputPreview"); preview->setToolTip(text);
            preview->setText(QFontMetrics(preview->font()).elidedText(text.simplified(), Qt::ElideRight, 220));
            h->addWidget(preview);
            auto *button = new QPushButton(tr("Cancel")); button->setObjectName("heldInputCancel"); button->setAutoDefault(false);
            button->setToolTip(tr("Cancel this message; hgs will not send it"));
            button->setEnabled(!m_cancelling.contains(id));
            QObject::connect(button, &QPushButton::clicked, this, [this, button, id, text] {
                m_cancelling.insert(id); button->setEnabled(false);
                if (cancel) cancel(id, text);
            });
            h->addWidget(button);
            layout()->addWidget(row);
        }
        setVisible(!ids.isEmpty());
        applyColors();
    }
    // A failed cancel enables its button again.
    void cancelFinished(const QString &requestId)
    {
        m_cancelling.remove(requestId);
        for (auto *row : findChildren<QWidget *>("heldInput"))
            if (row->property("requestId").toString() == requestId)
                if (auto *button = row->findChild<QPushButton *>("heldInputCancel")) button->setEnabled(true);
    }
    int count() const { return int(m_ids.size()); }
    void setColors(const QColor &text, const QColor &muted, const QColor &border, const QColor &background)
    {
        m_text = text; m_muted = muted; m_border = border; m_background = background;
        applyColors();
    }

private:
    void applyColors()
    {
        if (!m_text.isValid()) return;
        setStyleSheet(QStringLiteral(
            "QFrame#heldInputBar{background:%4;border-top:1px solid %3;}"
            "QLabel#heldInputText{color:%1;font-size:12px;}"
            "QLabel#heldInputPreview{color:%2;font-size:12px;}"
            "QPushButton#heldInputCancel{color:%1;background:transparent;border:1px solid %3;border-radius:6px;padding:2px 10px;font-size:12px;}"
            "QPushButton#heldInputCancel:disabled{color:%2;}")
            .arg(m_text.name(), m_muted.name(), m_border.name(), m_background.name()));
    }
    QStringList m_ids;
    QSet<QString> m_cancelling;
    QString m_agent;
    QColor m_text, m_muted, m_border, m_background;
};

// #511 N5: the held messages a composer queued, until hgs either delivers them
// or lists them as not delivered (the inspection's held_settled: failed, or
// uncertain). A message that was not delivered comes back to its composer
// once, with why; a delivered one simply leaves. Bounded, oldest first.
class HeldSent {
public:
    struct Lost { QString text, error; bool uncertain = false; };
    void add(const QString &requestId, const QString &text)
    {
        if (requestId.isEmpty()) return;
        m_sent.append({requestId, text});
        while (m_sent.size() > 32) m_sent.removeFirst();
    }
    void clear() { m_sent.clear(); }
    // The messages of this composer that `settled` lists, each taken once.
    QList<Lost> take(const QJsonArray &settled)
    {
        QList<Lost> lost;
        for (const auto &value : settled) {
            const auto item = value.toObject();
            const QString id = item.value("request_id").toString();
            for (int i = 0; i < m_sent.size(); ++i) {
                if (m_sent.at(i).first != id) continue;
                const QString text = m_sent.at(i).second.isEmpty() ? item.value("text").toString() : m_sent.at(i).second;
                lost.append({text, item.value("error").toString(), item.value("status").toString() == QLatin1String("uncertain")});
                m_sent.removeAt(i);
                break;
            }
        }
        return lost;
    }
    // The text an editor shows after `lost` comes back: an empty editor takes
    // it, one with a draft keeps the draft and gets it below.
    static QString restored(const QString &current, const QString &lost)
    {
        if (current.contains(lost)) return current;
        return current.trimmed().isEmpty() ? lost : current + QStringLiteral("\n\n") + lost;
    }
    // What the composer says: an uncertain message may have arrived.
    static QString notice(const Lost &lost)
    {
        return lost.uncertain
            ? QObject::tr("Delivery unconfirmed — check the conversation before sending again. %1").arg(lost.error).trimmed()
            : QObject::tr("Not delivered: %1 The text is back in the composer.").arg(lost.error.trimmed().endsWith('.') ? lost.error.trimmed() : lost.error.trimmed() + '.');
    }
    int size() const { return int(m_sent.size()); }
private:
    QList<QPair<QString, QString>> m_sent;
};
