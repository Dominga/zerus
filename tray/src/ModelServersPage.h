#pragma once
#include "ModelServers.h"
#include <QPointer>
#include <QWidget>
#include <functional>

class QLabel;
class QPushButton;
class QScrollArea;
class QVBoxLayout;

// WhiteWh fork, U60 (#656): the Model servers tab beside Team designer
// (design/model-servers, the servers view). Every server of this computer
// with its API address (copied in one click), SSH host, free memory and
// models: their state, New session…, Logs, Up and Down… when the server has
// control commands. A server may be shared, so every Down and Down all asks
// first and says so. Unreachable, read-only and not-read servers say why;
// nothing retries on its own: Try again and Refresh read once.
class ModelServersPage : public QWidget {
public:
    explicit ModelServersPage(ModelServers::Controller *servers, QWidget *parent = nullptr);
    void setDark(bool dark);
    // Settings > Model servers, at that server (empty: the list).
    std::function<void(const QString &server)> editServers;
    // This computer's Machines, to name an SSH host as one of them.
    std::function<QStringList()> machines;
    // The folders New session offers (this computer's project folders).
    std::function<QStringList()> folders;
    // Starts the session: hgs AGENT FOLDER --new -n TAG --model-server SERVER --model MODEL.
    std::function<void(const QString &agent, const QString &folder, const QString &tag, const QString &server, const QString &model)> startSession;
    // The dialogs (tests answer them).
    void askDown(const QString &server, const QStringList &models);
    void showLogs(const QString &server, const QString &model);
    void newSession(const QString &server, const QString &model);
protected:
    void showEvent(QShowEvent *event) override;
    void hideEvent(QHideEvent *event) override;
private:
    void render();
    QWidget *serverCard(const ModelServers::Server &server, const ModelServers::Status &status);
    QString hostText(const ModelServers::Server &server) const;
    void notice(const QString &text, bool error = false);
    void up(const QString &server, const QString &model);
    ModelServers::Controller *m_servers;
    QScrollArea *m_scroll;
    QWidget *m_list = nullptr;
    QLabel *m_summary, *m_notice, *m_tile;
    QPushButton *m_refreshAll, *m_editServers;
    QString m_copied, m_noticeText;
    bool m_noticeError = false, m_dark = true;
};
