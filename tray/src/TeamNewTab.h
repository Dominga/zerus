#pragma once
#include <QDateTime>
#include <QJsonObject>
#include <QMap>
#include <QSet>
#include <QTimer>
#include <QWidget>
#include <functional>
#include "TeamModel.h"

class HgsClient;
class TeamController;
class TeamConversation;
class TeamTemplateList;
class QCheckBox;
class QComboBox;
class QFrame;
class QGridLayout;
class QLabel;
class QLineEdit;
class QPlainTextEdit;
class QProgressBar;
class QPushButton;
class QRadioButton;
class QScrollArea;
class QSpinBox;
class QSplitter;
class QStackedWidget;
class QTabWidget;
class QToolButton;
class QVBoxLayout;

// WhiteWh fork: agent teams. The New team tab next to Activity (F5, spec 3 and
// 11.1). Planning a team is a talk with its future lead: the settings column
// (compact template list, name, folder and its policy, members, approval, the
// protocol addendum folded) and the lead column with Chat, Goal and Plan. A
// narrow tab shows the two columns as tabs. Opening the tab creates nothing;
// Start planning creates the draft (`hgs team draft` start) with this session or
// a new one as its lead. Then the owner's edits go out as draft updates with
// the revision they read, the lead's edits come back with each read and stay
// marked until the owner has seen them, Create team runs the preflight and
// Discard draft asks first and whether to stop the lead. A draft survives the
// tab: its lead's session reopens it.
class TeamNewTab : public QWidget {
public:
    struct Computer { QString alias; QString label; bool online = true; };
    struct Coordinator { QString name, agent, model, account, folder, computer, status; bool busy = false; };
    struct Member { QString id, role, agent, account, model, effort, permissions, computer, prompt; bool worktree = true; };
    TeamNewTab(TeamController *teams, HgsClient *client, const QList<Computer> &computers, const Coordinator &coordinator,
               const QString &project, QWidget *parent = nullptr);
    // The session the tab belongs to; it is offered as the lead.
    QString leadSession() const { return m_coordinator.name; }
    // The open draft's team id, empty before Start planning.
    QString draftId() const { return m_draft; }
    void setDark(bool dark);
    // The coordinator's live state changes while the tab is open.
    void setCoordinatorBusy(bool busy);
    // Shows an existing draft: the tab was closed, or its lead was selected.
    void openDraft(const QString &team);
    // F6: chooses a template as a click in the list does.
    void selectTemplate(const QString &id);
    std::function<void(const QString &team)> created;
    // The draft was discarded; the tab starts over.
    std::function<void(const QString &team)> discarded;
    // Opens the Team designer for templates and the catalog (F4: it gets no
    // other work; the lead plans the team).
    std::function<void()> askDesigner;
    // Discard draft asks first; false keeps the draft. *stopLead says whether
    // the lead's session is stopped too. Unset, a message box asks.
    std::function<bool(bool *stopLead)> confirmDiscard;
    // How often an open draft is read again while the tab shows.
    static void setDraftPollInterval(int milliseconds);
    // The draft fields the form has changed and not yet sent.
    QStringList pendingFields() const { return QStringList(m_dirty.cbegin(), m_dirty.cend()); }
protected:
    void resizeEvent(QResizeEvent *event) override;
    void showEvent(QShowEvent *event) override;
    void hideEvent(QHideEvent *event) override;
private:
    QWidget *buildSettings();
    QWidget *buildLead();
    QToolButton *unseenMark(const QString &field);
    void arrange();
    void showLeadPage(const QString &page);
    void pickTemplate(const QString &id);
    void addMember(const Teams::Role *role);
    void renderTemplateCard();
    void renderMembers();
    void renderPlan();
    void renderLead();
    void renderUnseen();
    void validate();
    // F5: the draft's life.
    void startPlanning();
    void touch(const QString &field);
    void flush();
    void readDraft();
    void applyDraft(const Teams::Team &team);
    void markSeen(const QString &field);
    void createTeam();
    void discardDraft();
    void resetDraft();
    QJsonValue fieldValue(const QString &field) const;
    QJsonObject settingsJson() const;
    QJsonArray membersJson() const;
    QJsonArray templateTasks() const;
    QStringList rowErrors(const Member &member) const;
    QJsonObject memberJson(const Member &member, const QString &id) const;
    struct Account { QString id, provider, label; bool signedIn = true; };
    TeamController *m_teams;
    HgsClient *m_client;
    QList<Computer> m_computers;
    Coordinator m_coordinator;
    QString m_project, m_template = QStringLiteral("-");
    QList<Member> m_members;
    QList<Teams::Task> m_tasks;
    QMap<QString, QList<Account>> m_accounts;   // by computer alias
    bool m_dark = true, m_wide = true, m_tableMembers = true;
    // The columns' minimum widths: side by side when both fit, tabs below.
    static constexpr int settingsMinimum = 600, leadMinimum = 400;
    QSplitter *m_columns = nullptr;
    QTabWidget *m_narrow = nullptr;
    QWidget *m_settingsPane = nullptr, *m_leadPane = nullptr;
    QScrollArea *m_scroll = nullptr;
    TeamTemplateList *m_templateList = nullptr;
    QWidget *m_templateCard = nullptr, *m_membersBox = nullptr;
    QLabel *m_gitNote = nullptr;
    QVBoxLayout *m_templateCardLayout = nullptr, *m_membersLayout = nullptr;
    QLabel *m_busy = nullptr, *m_membersCount = nullptr, *m_summary = nullptr, *m_reason = nullptr, *m_branch = nullptr, *m_preflight = nullptr;
    QLineEdit *m_name = nullptr, *m_folder = nullptr;
    QCheckBox *m_worktrees = nullptr, *m_review = nullptr;
    // B6: auto (the lead picks), shared or a worktree per member.
    QComboBox *m_folderPolicy = nullptr, *m_phaseApproval = nullptr;
    // N10: the path is finished (focus out or Enter); typing keeps auto.
    bool m_folderCommitted = true;
    // N10: the shared choice came from the non-Git folder rule.
    bool m_forcedShared = false;
    QWidget *m_planning = nullptr;
    QString m_planningValue = QStringLiteral("lead");
    QSpinBox *m_max = nullptr;
    QPushButton *m_addMember = nullptr, *m_create = nullptr, *m_discard = nullptr, *m_start = nullptr;
    QProgressBar *m_progress = nullptr;
    // The protocol addendum, folded: how the lead runs the team.
    QPlainTextEdit *m_protocol = nullptr;
    QToolButton *m_protocolToggle = nullptr;
    bool m_protocolEdited = false;
    // The lead column.
    QWidget *m_leadTabs = nullptr, *m_leadSetup = nullptr;
    QLabel *m_leadTitle = nullptr, *m_leadStarting = nullptr, *m_unseenLegend = nullptr, *m_startError = nullptr;
    QStackedWidget *m_leadStack = nullptr;
    QRadioButton *m_leadThis = nullptr, *m_leadNew = nullptr;
    QComboBox *m_leadAgent = nullptr, *m_leadModel = nullptr, *m_leadEffort = nullptr, *m_leadAccount = nullptr;
    TeamConversation *m_chat = nullptr;
    QPlainTextEdit *m_goal = nullptr;
    QLabel *m_planEmpty = nullptr;
    QVBoxLayout *m_planLayout = nullptr;
    QWidget *m_planBox = nullptr;
    QMap<QString, QList<QToolButton *>> m_marks;   // by draft field
    // The draft: its team id, the revision last read or written, the lead's
    // session once it exists, the fields the lead changed and the owner has
    // not seen, the owner's fields not yet sent, and the preflight.
    QString m_draft, m_leadSessionName, m_leadPage = QStringLiteral("chat");
    qint64 m_revision = 0;
    QStringList m_unseen, m_preflightErrors, m_preflightWarnings;
    QSet<QString> m_dirty;
    QJsonObject m_settings;    // the draft's settings as read; the form changes its own fields
    QDateTime m_savedAt;
    QString m_saveError;       // the last refused draft call, until one succeeds
    bool m_starting = false, m_sending = false, m_applying = false, m_creating = false, m_retried = false;
    bool m_createAfterSave = false; // Create team pressed while edits were unsaved
    QTimer m_flushTimer, m_pollTimer;
    static int s_pollInterval;
};
