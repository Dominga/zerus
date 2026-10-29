#pragma once
#include <QObject>
#include <memory>

class HgsClient;
class TeamController;
class TeamAttentionNotices;

// WhiteWh fork, N32 of #499 (N7, U18): the teams' attention items reach the
// desktop notifications before the Sessions window exists, which is how Zerus
// starts in the tray. It reads `hgs team list` at a background pace and keeps
// the known items where the window keeps them (teams/attentionNotified), so an
// item is raised once whichever of the two sees it first. TrayAgent stops it
// when it creates the window, whose own team poll takes over.
class TeamAttentionWatch : public QObject {
    Q_OBJECT
public:
    static constexpr int BackgroundMs = 30000;
    explicit TeamAttentionWatch(HgsClient *client, QObject *parent = nullptr, int intervalMs = BackgroundMs);
    ~TeamAttentionWatch() override;
    // Active: poll now and then every interval, starting from the stored known items.
    void setActive(bool active);
    bool isActive() const { return bool(m_notices); }
signals:
    void raised(const QString &token, const QString &title, const QString &body);
    void cleared(const QString &token);
private:
    void observe();
    TeamController *m_teams;
    std::unique_ptr<TeamAttentionNotices> m_notices;
};
