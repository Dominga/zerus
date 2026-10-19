#pragma once
#include "HgsClient.h"
#include "SessionTag.h"
#include <QCheckBox>
#include <QDateTime>
#include <QDialog>
#include <QDialogButtonBox>
#include <QDir>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QJsonArray>
#include <QJsonObject>
#include <QLabel>
#include <QLineEdit>
#include <QMessageBox>
#include <QPushButton>
#include <QTreeWidget>
#include <QVBoxLayout>

// WhiteWh fork: sessions of the Claude desktop app (its Code tab) on one
// computer (hgs claude-app ls), to continue one as a new Zerus session. The
// list is the app's newest sessions; "This folder only" narrows it to the
// folder New session shows. A session the app may still run asks first: two
// processes writing one conversation corrupt it.
class ClaudeAppSessionsDialog : public QDialog {
public:
    ClaudeAppSessionsDialog(HgsClient *client, const QString &host, const QString &folder, QWidget *parent = nullptr)
        : QDialog(parent), m_client(client), m_host(host), m_folder(normalized(folder))
    {
        setObjectName("claudeAppSessionsDialog");
        setWindowTitle(tr("Claude app sessions"));
        resize(760, 460);
        auto *layout = new QVBoxLayout(this);
        auto *intro = new QLabel(tr("Continue a session of the Claude desktop app in Zerus. Zerus starts Claude Code with that conversation in its folder; "
                                    "the app's own copy stays as it is. Stop the session in the app first."));
        intro->setWordWrap(true);
        layout->addWidget(intro);
        auto *row = new QHBoxLayout;
        m_onlyFolder = new QCheckBox(tr("This folder only"));
        m_onlyFolder->setObjectName("claudeAppOnlyFolder");
        m_onlyFolder->setChecked(!m_folder.isEmpty());
        m_onlyFolder->setEnabled(!m_folder.isEmpty());
        m_onlyFolder->setToolTip(QDir::toNativeSeparators(folder));
        row->addWidget(m_onlyFolder);
        row->addStretch();
        m_refresh = new QPushButton(tr("Refresh"));
        m_refresh->setObjectName("claudeAppRefresh");
        m_refresh->setAutoDefault(false);
        row->addWidget(m_refresh);
        layout->addLayout(row);
        m_list = new QTreeWidget;
        m_list->setObjectName("claudeAppSessions");
        m_list->setRootIsDecorated(false);
        m_list->setHeaderLabels({tr("Session"), tr("Folder"), tr("Last active"), tr("State")});
        m_list->header()->setSectionResizeMode(0, QHeaderView::Stretch);
        m_list->header()->setSectionResizeMode(1, QHeaderView::Interactive);
        m_list->setColumnWidth(1, 240);
        m_list->setTextElideMode(Qt::ElideMiddle);
        layout->addWidget(m_list, 1);
        m_status = new QLabel;
        m_status->setObjectName("claudeAppStatus");
        m_status->setWordWrap(true);
        layout->addWidget(m_status);
        auto *nameRow = new QHBoxLayout;
        nameRow->addWidget(new QLabel(tr("Session name")));
        m_name = new QLineEdit;
        m_name->setObjectName("claudeAppName");
        nameRow->addWidget(m_name, 1);
        layout->addLayout(nameRow);
        auto *buttons = new QDialogButtonBox(QDialogButtonBox::Cancel);
        m_continue = buttons->addButton(tr("Continue in Zerus"), QDialogButtonBox::AcceptRole);
        m_continue->setObjectName("primary");
        layout->addWidget(buttons);
        connect(buttons, &QDialogButtonBox::rejected, this, &QDialog::reject);
        connect(m_continue, &QPushButton::clicked, this, &ClaudeAppSessionsDialog::confirm);
        connect(m_refresh, &QPushButton::clicked, this, &ClaudeAppSessionsDialog::request);
        connect(m_onlyFolder, &QCheckBox::toggled, this, &ClaudeAppSessionsDialog::render);
        connect(m_list, &QTreeWidget::currentItemChanged, this, [this](QTreeWidgetItem *item) {
            if (item) m_name->setText(item->data(0, Qt::UserRole).toJsonObject()["suggested_name"].toString());
            update();
        });
        connect(m_list, &QTreeWidget::itemDoubleClicked, this, [this] { if (m_continue->isEnabled()) confirm(); });
        connect(m_name, &QLineEdit::textChanged, this, &ClaudeAppSessionsDialog::update);
        connect(m_client, &HgsClient::claudeAppSessionsReady, this, [this](quint64 id, const QString &host, const QJsonObject &data) {
            if (id != m_request || host != m_host) return;
            m_request = 0; m_data = data; m_error.clear(); render();
        });
        connect(m_client, &HgsClient::claudeAppSessionsFailed, this, [this](quint64 id, const QString &host, const QString &error) {
            if (id != m_request || host != m_host) return;
            m_request = 0; m_error = error; render();
        });
        request();
    }
    // The chosen session, after Continue in Zerus.
    QString sessionId() const { return m_chosen["id"].toString(); }
    QString folder() const { return m_chosen["cwd"].toString(); }
    QString name() const { return m_name->text(); }
    bool force() const { return m_force; }

private:
    static QString normalized(QString path)
    {
        path = QDir::fromNativeSeparators(path);
        while (path.size() > 1 && path.endsWith('/') && !path.endsWith(QLatin1String(":/"))) path.chop(1);
        return path;
    }
    bool sameFolder(const QString &path) const
    {
        // Drive letters and Windows folders compare without case.
        return normalized(path).compare(m_folder, path.contains('\\') || path.contains(':') ? Qt::CaseInsensitive : Qt::CaseSensitive) == 0;
    }
    void request()
    {
        if (m_request) return;
        m_request = m_client->requestClaudeAppSessions(m_host);
        m_status->setText(tr("Reading the Claude app's sessions…"));
        update();
    }
    static QString ago(qint64 seconds)
    {
        if (seconds < 90) return tr("just now");
        if (seconds < 5400) return tr("%1 min ago").arg(seconds / 60);
        if (seconds < 129600) return tr("%1 h ago").arg(seconds / 3600);
        return tr("%1 days ago").arg(seconds / 86400);
    }
    void render()
    {
        const QString chosen = m_list->currentItem() ? m_list->currentItem()->data(0, Qt::UserRole).toJsonObject()["id"].toString() : QString();
        m_list->clear();
        const auto sessions = m_data["sessions"].toArray();
        const qint64 now = QDateTime::currentSecsSinceEpoch();
        int shown = 0;
        for (const auto &value : sessions) {
            const auto session = value.toObject();
            if (m_onlyFolder->isChecked() && !sameFolder(session["cwd"].toString())) continue;
            const auto title = session["title"].toString();
            QString state = session["running"].toBool() ? tr("Open in the app") : session["archived"].toBool() ? tr("Archived") : tr("Stopped");
            if (!session["history"].toBool()) state = tr("No history");
            else if (!session["folder_exists"].toBool()) state = tr("Folder missing");
            auto *item = new QTreeWidgetItem({title.isEmpty() ? tr("Untitled (%1)").arg(session["conversation"].toString().left(8)) : title,
                                              QDir::toNativeSeparators(session["cwd"].toString()),
                                              ago(now - qint64(session["last_activity"].toDouble())), state});
            item->setData(0, Qt::UserRole, session);
            item->setToolTip(0, session["conversation"].toString());
            item->setToolTip(1, QDir::toNativeSeparators(session["cwd"].toString()));
            m_list->addTopLevelItem(item);
            if (session["id"].toString() == chosen) m_list->setCurrentItem(item);
            ++shown;
        }
        if (!m_error.isEmpty()) m_status->setText(m_error);
        else if (m_data["available"] == false) m_status->setText(tr("The Claude app keeps no sessions on this computer."));
        else if (!shown) m_status->setText(m_onlyFolder->isChecked() ? tr("No Claude app session works in this folder.") : tr("No Claude app sessions."));
        else m_status->setText(m_data["truncated"].toBool() ? tr("The newest %1 sessions.").arg(sessions.size()) : QString());
        if (!m_list->currentItem() && m_list->topLevelItemCount()) m_list->setCurrentItem(m_list->topLevelItem(0));
        update();
    }
    void update()
    {
        m_refresh->setEnabled(!m_request);
        const auto *item = m_list->currentItem();
        const auto session = item ? item->data(0, Qt::UserRole).toJsonObject() : QJsonObject();
        const auto problem = SessionTag::problem(m_name->text());
        const bool usable = item && session["history"].toBool() && session["folder_exists"].toBool();
        m_continue->setEnabled(usable && problem.isEmpty() && !m_request);
        m_continue->setToolTip(!item ? QString() : !session["history"].toBool() ? tr("Claude Code has no history of this conversation here.")
                               : !session["folder_exists"].toBool() ? tr("Its folder is not on this computer.") : problem);
    }
    void confirm()
    {
        const auto *item = m_list->currentItem();
        if (!item) return;
        const auto session = item->data(0, Qt::UserRole).toJsonObject();
        m_force = false;
        if (session["running"].toBool()) {
            const auto answer = QMessageBox::warning(this, tr("Still open in the Claude app?"),
                session["running_process"].toBool()
                    ? tr("A Claude process of the app still continues this conversation. If both write to it, its history breaks. Stop the session in the Claude app first. Continue anyway?")
                    : tr("This conversation changed in the last two minutes, so the Claude app may still run it. If both write to it, its history breaks. Continue anyway?"),
                QMessageBox::Yes | QMessageBox::Cancel, QMessageBox::Cancel);
            if (answer != QMessageBox::Yes) return;
            m_force = true;
        }
        m_chosen = session;
        accept();
    }
    HgsClient *m_client;
    QString m_host, m_folder, m_error;
    QJsonObject m_data, m_chosen;
    quint64 m_request = 0;
    bool m_force = false;
    QCheckBox *m_onlyFolder;
    QPushButton *m_refresh, *m_continue;
    QTreeWidget *m_list;
    QLabel *m_status;
    QLineEdit *m_name;
};
