#include "Settings.h"
#include "TeamAttentionWatch.h"
#include "TeamAttentionNotices.h"
#include "TeamWaitingNotices.h"
#include "TeamController.h"
#include <QSettings>

TeamAttentionWatch::TeamAttentionWatch(HgsClient *client, QObject *parent, int intervalMs)
    : QObject(parent), m_teams(new TeamController(client, this))
{
    m_teams->setInterval(intervalMs);
    connect(m_teams, &TeamController::teamsChanged, this, &TeamAttentionWatch::observe);
}

TeamAttentionWatch::~TeamAttentionWatch() = default;

void TeamAttentionWatch::setActive(bool active)
{
    if (active == isActive()) return;
    // The window may have changed the known items since this watch last ran.
    if (active) m_notices = std::make_unique<TeamAttentionNotices>(Settings::value("teams/attentionNotified").toStringList());
    else m_notices.reset();
    m_teams->setActive(active);
    // An unchanged list is not announced again: compare the teams read before.
    if (active && !m_teams->teams().isEmpty()) observe();
}

void TeamAttentionWatch::observe()
{
    // An answer that arrives after the window took over is not ours.
    if (!m_notices) return;
    auto changes = m_notices->observe(m_teams->teams());
    if (changes.raised.isEmpty() && changes.cleared.isEmpty()) return;
    Settings::set("teams/attentionNotified", m_notices->known());
    // U55: a thing the waiting banner announced already is not announced twice.
    TeamWaitingNotices::dropAnnounced(changes, m_teams->teams());
    for (const auto &token : changes.cleared) emit cleared(token);
    for (const auto &notice : changes.raised) emit raised(notice.token, notice.title, notice.body);
}
