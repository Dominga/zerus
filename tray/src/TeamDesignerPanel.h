#pragma once
#include <QWidget>
#include <functional>
#include "TeamModel.h"

class TeamController;
class QScrollArea;

// The Team designer's draft: what the
// designer session proposed for the catalog — new, changed and removed roles,
// templates and routing rules with their old and new values — kept or rejected
// one by one, then applied together as the next catalog version. The designer
// itself can only propose; nothing changes before Apply.
class TeamDesignerPanel : public QWidget {
public:
    explicit TeamDesignerPanel(TeamController *teams, QWidget *parent = nullptr);
    void setDark(bool dark) { m_dark = dark; render(); }
    void render();
    std::function<bool(const QString &name, const QString &title, const QString &text, const QString &action)> confirm;
private:
    QWidget *proposalCard(const Teams::Proposal &proposal);
    TeamController *m_teams;
    QScrollArea *m_scroll;
    QString m_kind = QStringLiteral("role"), m_notice;
    bool m_dark = true;
};
