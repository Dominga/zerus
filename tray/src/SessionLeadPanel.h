#pragma once
#include <QComboBox>
#include <QHBoxLayout>
#include <QLabel>
#include <QList>
#include <QPair>
#include <QPushButton>
#include <QVBoxLayout>
#include <functional>

// WhiteWh fork: the selected session's place among the sessions of its
// project, in Details. A subordinate shows its lead with Open, Send report and
// Detach; a lead lists its subordinates with Release; any other session can be
// set to report to one of the project's sessions.
class SessionLeadPanel : public QWidget {
public:
    using Session = QPair<QString, QString>;   // identity, name
    explicit SessionLeadPanel(QWidget *parent = nullptr) : QWidget(parent)
    {
        setObjectName("sessionLeadPanel");
        setAttribute(Qt::WA_StyledBackground, true);
        m_layout = new QVBoxLayout(this);
        m_layout->setContentsMargins(8, 6, 8, 8);
        m_layout->setSpacing(5);
        auto *title = new QLabel(tr("Hierarchy"));
        title->setObjectName("detailsIdentityTitle");
        m_layout->addWidget(title);
        m_body = new QVBoxLayout;
        m_body->setSpacing(5);
        m_layout->addLayout(m_body);
    }
    std::function<void(const QString &identity, const QString &lead)> setLead;
    std::function<void(const QString &identity)> open;
    std::function<void()> sendReport;
    // lead: this session's lead, if any; subordinates: sessions reporting to it;
    // candidates: the project's sessions it may report to.
    void setContext(const QString &identity, const Session &lead, const QList<Session> &subordinates, const QList<Session> &candidates)
    {
        m_identity = identity;
        while (auto *item = m_body->takeAt(0)) {
            if (auto *widget = item->widget()) widget->deleteLater();
            else if (auto *layout = item->layout()) clear(layout);
            delete item;
        }
        setVisible(!identity.isEmpty());
        if (identity.isEmpty()) return;
        const auto button = [](const QString &text, const char *name) {
            auto *b = new QPushButton(text); b->setObjectName(name); b->setAutoDefault(false); return b;
        };
        if (!lead.first.isEmpty()) {
            auto *row = new QHBoxLayout; row->setSpacing(6);
            auto *label = new QLabel(tr("Reports to <b>%1</b>").arg(lead.second.toHtmlEscaped()));
            label->setObjectName("sessionLeadName"); label->setTextFormat(Qt::RichText);
            row->addWidget(label, 1);
            m_body->addLayout(row);
            auto *actions = new QHBoxLayout; actions->setSpacing(6);
            auto *openLead = button(tr("Open"), "openSessionLead");
            connect(openLead, &QPushButton::clicked, this, [this, id = lead.first] { if (open) open(id); });
            auto *report = button(tr("Send report"), "sendSessionReport");
            report->setToolTip(tr("Open the lead with this session's last reply as a draft"));
            connect(report, &QPushButton::clicked, this, [this] { if (sendReport) sendReport(); });
            auto *detach = button(tr("Detach"), "detachSessionLead");
            connect(detach, &QPushButton::clicked, this, [this] { if (setLead) setLead(m_identity, {}); });
            actions->addWidget(openLead); actions->addWidget(report); actions->addWidget(detach); actions->addStretch();
            m_body->addLayout(actions);
            return;
        }
        if (!subordinates.isEmpty()) {
            auto *label = new QLabel(tr("Leads"));
            label->setObjectName("sessionSubordinatesTitle");
            m_body->addWidget(label);
            for (const auto &session : subordinates) {
                auto *row = new QHBoxLayout; row->setSpacing(6);
                auto *name = button(session.second, "openSessionSubordinate");
                name->setFlat(true);
                connect(name, &QPushButton::clicked, this, [this, id = session.first] { if (open) open(id); });
                auto *release = button(tr("Release"), "releaseSessionSubordinate");
                connect(release, &QPushButton::clicked, this, [this, id = session.first] { if (setLead) setLead(id, {}); });
                row->addWidget(name, 1); row->addWidget(release);
                m_body->addLayout(row);
            }
            return;
        }
        if (candidates.isEmpty()) {
            auto *hint = new QLabel(tr("No other session in this project can lead this one."));
            hint->setObjectName("sessionLeadHint"); hint->setWordWrap(true);
            m_body->addWidget(hint);
            return;
        }
        auto *row = new QHBoxLayout; row->setSpacing(6);
        auto *choice = new QComboBox; choice->setObjectName("sessionLeadChoice"); choice->setAccessibleName(tr("Lead session"));
        for (const auto &session : candidates) choice->addItem(session.second, session.first);
        auto *link = button(tr("Report to"), "linkSessionLead");
        connect(link, &QPushButton::clicked, this, [this, choice] { if (setLead) setLead(m_identity, choice->currentData().toString()); });
        row->addWidget(link); row->addWidget(choice, 1);
        m_body->addLayout(row);
    }

private:
    static void clear(QLayout *layout)
    {
        while (auto *item = layout->takeAt(0)) {
            if (auto *widget = item->widget()) widget->deleteLater();
            else if (auto *inner = item->layout()) clear(inner);
            delete item;
        }
    }
    QString m_identity;
    QVBoxLayout *m_layout, *m_body;
};
