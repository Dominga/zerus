#include "TeamDesignerPage.h"
#include "TeamCatalogEditor.h"
#include "TeamController.h"
#include "TeamDesignerPanel.h"
#include "TeamWidgets.h"
#include <QHBoxLayout>
#include <QSplitter>
#include <QVBoxLayout>

using namespace TeamUi;

TeamDesignerPage::TeamDesignerPage(TeamController *teams, QWidget *parent) : QWidget(parent), m_teams(teams)
{
    setObjectName("teamDesignerPage");
    auto *outer = new QVBoxLayout(this); outer->setContentsMargins(20, 16, 20, 12); outer->setSpacing(4);
    m_title = new QLabel(tr("Team designer")); m_title->setObjectName("teamDesignerPageTitle");
    m_subtitle = new QLabel(tr("Universal roles, templates and routing rules. New team, next to Activity, builds a team for a task from them."));
    m_subtitle->setObjectName("teamDesignerPageSubtitle"); m_subtitle->setWordWrap(true);
    outer->addWidget(m_title); outer->addWidget(m_subtitle); outer->addSpacing(8);
    auto *split = new QSplitter(Qt::Horizontal); split->setObjectName("teamDesignerSplit"); split->setChildrenCollapsible(false);
    m_editor = new TeamCatalogEditor(teams); m_editor->hideHeading();
    split->addWidget(m_editor);
    auto *right = new QWidget; right->setObjectName("teamDesignerSide"); right->setMinimumWidth(320);
    auto *rv = new QVBoxLayout(right); rv->setContentsMargins(12, 0, 0, 0); rv->setSpacing(8);
    m_agentTitle = new QLabel(tr("Designer agent")); m_agentTitle->setObjectName("teamDesignerAgentTitle");
    m_agentStatus = new QLabel; m_agentStatus->setObjectName("teamDesignerAgentStatus"); m_agentStatus->setWordWrap(true);
    auto *buttons = new QHBoxLayout; buttons->setSpacing(8);
    m_start = button(tr("Start designer…"), "teamDesignerStart", true);
    m_open = button(tr("Open chat"), "teamDesignerOpen");
    buttons->addWidget(m_start); buttons->addWidget(m_open); buttons->addStretch();
    rv->addWidget(m_agentTitle); rv->addWidget(m_agentStatus); rv->addLayout(buttons);
    m_draft = new TeamDesignerPanel(teams);
    m_draft->confirm = [this](const QString &name, const QString &title, const QString &text, const QString &action) {
        return confirm ? confirm(name, title, text, action) : true;
    };
    rv->addWidget(m_draft, 1);
    split->addWidget(right);
    split->setStretchFactor(0, 3); split->setStretchFactor(1, 2);
    outer->addWidget(split, 1);
    m_editor->askDesigner = [this] { if (startDesigner) startDesigner(); };
    QObject::connect(m_start, &QPushButton::clicked, this, [this] { if (startDesigner) startDesigner(); });
    QObject::connect(m_open, &QPushButton::clicked, this, [this] {
        const QString session = designerSession ? designerSession() : QString();
        if (!session.isEmpty() && openDesigner) openDesigner(session);
    });
    setDark(true);
}

void TeamDesignerPage::setDark(bool dark)
{
    m_dark = dark;
    const Colors c = colorsFor(dark);
    m_title->setStyleSheet(QStringLiteral("color:%1;font-size:18px;font-weight:600;").arg(c.fg.name()));
    m_subtitle->setStyleSheet(QStringLiteral("color:%1;font-size:12px;").arg(c.muted.name()));
    m_agentTitle->setStyleSheet(QStringLiteral("color:%1;font-size:13px;font-weight:600;").arg(c.fg.name()));
    m_agentStatus->setStyleSheet(QStringLiteral("color:%1;font-size:12px;").arg(c.text2.name()));
    m_editor->setDark(dark);
    m_draft->setDark(dark);
    refresh();
}

void TeamDesignerPage::refresh()
{
    const QString session = designerSession ? designerSession() : QString();
    m_agentStatus->setText(session.isEmpty()
        ? tr("Not running. The designer reads a project read-only and proposes roles, templates and rules; nothing changes before you apply its draft.")
        : tr("Running as %1. Its proposals appear below as it works.").arg(session));
    m_start->setVisible(session.isEmpty());
    m_open->setVisible(!session.isEmpty());
}

void TeamDesignerPage::showEvent(QShowEvent *event)
{
    QWidget::showEvent(event);
    m_teams->setWatchCatalogPage(true);
    m_draft->render();
    refresh();
}

void TeamDesignerPage::hideEvent(QHideEvent *event)
{
    QWidget::hideEvent(event);
    m_teams->setWatchCatalogPage(false);
}
