#pragma once
#include <QDialog>
#include <QPointer>
#include "FleetState.h"
#include "SessionOrganization.h"
#include <functional>
class CollapsibleBlock;
class IronRulesPanel;
class ProjectPreview;
class WorktreePanel;
class WorktreesDialog;
class QAction;
class QCheckBox;
class QLabel;
class QPushButton;
class QListWidget;
class QTableWidget;
class QLineEdit;
class QSplitter;

// App-owned logical projects; folders are addresses on fleet machines.
class ProjectsDialog : public QDialog {
    Q_OBJECT
public:
    ProjectsDialog(HgsClient *client, QWidget *parent = nullptr, bool embedded = false, SessionOrganization *projects = nullptr);
    void setTheme(bool dark);
    void setFleet(const FleetState &fleet);
    void selectHost(const QString &host);
    void selectProject(const QString &id);
    void refresh();
    void setSwarmStatus(const QString &text, bool error);
    void setSwarmSnapshot(const QJsonObject &snapshot);
    void deleteProject(const QString &id);
    static bool nameProblem(const QString &name, QString *why);
    static QString nameWarning(const QString &) { return {}; }
    static QString suggestName(const QString &dir, MachinePlatform::Style style = MachinePlatform::Style::Posix);
    // U25 part 7 (spec 7): the project's sessions, one row each - its agent,
    // name, team role, machine, branch and status - from the window.
    struct ProjectSession { QString host, name, title, agent, role, machine, branch, status, tone, team; };
    void setSessionsProvider(std::function<QList<ProjectSession>(const QString &project)> provider) { m_sessionsProvider = std::move(provider); showProject(); }
    // The kind of a folder: "Main checkout · main", "Worktree · team/x/y",
    // "Clone · release/2.4"; `team` is set for a team member's worktree.
    QString folderKind(const SessionOrganization::Folder &folder, bool *team = nullptr) const;
public slots:
    void setProjects(const QList<ProjectInfo> &projects);
signals:
    void writeReported(const QString &op, bool ok);
    void projectsChanged(const QString &host);
    void organizationChanged();
    void swarmRequested();
    void connectSwarmRequested(const QString &computer);
    void catalogDraftEditing(bool active);
    void folderSessionsRequested(const QString &host, const QString &path, bool checkout);
    void relatedSessionRequested(const QString &host, const QString &name, const QString &archive);
    void newPathSessionRequested(const QString &project, const QString &host, const QString &path);
    void newSessionRequested(const QString &project, const QString &host, const QString &folder);
    void newTeamRequested(const QString &project);
private:
    void showProject();
    void updateSwarmConnections();
    void moveProject(int offset);
    void chooseColor();
    void changed();
    void addProject();
    void removeProject();
    void addFolder();
    void cloneRepository();
    void editFolder(const QString &id, const QString &projectId = {});
    void removeFolder(const QString &projectId, const QString &id);
    void transferFolder(const QString &projectId, const SessionOrganization::Folder &folder,
                        const QString &destination, bool copy);
    SessionOrganization::Folder selectedFolder() const;
    // Folders of the local machine follow its platform (MachinePlatform::of); peers are POSIX.
    MachinePlatform::Style folderStyle(const QString &machine) const;
    QString folderOpenProblem(const SessionOrganization::Folder &folder) const;
    void openFolder(const SessionOrganization::Folder &folder);
    QString folderLaunchProblem(const SessionOrganization::Folder &folder) const;
    void newFolderSession(const QString &project, const SessionOrganization::Folder &folder);
    void updateFolderActions();
    void showFolderMenu(const QPoint &position);
    void importLegacy(const QString &host, const QList<ProjectInfo> &projects);
    QString currentId() const;
    HgsClient *m_client;
    SessionOrganization m_ownedProjects;
    SessionOrganization *m_projects;
    FleetState m_fleet;
    QString m_host;
    QSet<QString> m_importing;
    // Machines whose import left aliases out this run: not marked imported, so the
    // next start tries them again, but not asked again before then.
    QSet<QString> m_importDeferred;
    QListWidget *m_list;
    QAction *m_moveUp,*m_moveDown;
    QTableWidget *m_folders;
    QSplitter *m_folderSplitter;
    bool m_folderSplitInitialized = false;
    WorktreePanel *m_worktrees;
    QPointer<WorktreesDialog> m_worktreeOverview;
    QLineEdit *m_name;
    QLabel *m_id, *m_status;
    QCheckBox *m_default;
    QCheckBox *m_allProjects;
    QLabel *m_swarmStatus;
    QWidget *m_swarmConnectionPanel;
    QLabel *m_swarmConnectionHint;
    QPushButton *m_connectSwarm;
    QJsonObject m_swarmSnapshot;
    QStringList m_unconnectedComputers;
    bool m_dark = false;
    ProjectPreview *m_preview;
    QPushButton *m_color, *m_remove, *m_removeFolder, *m_editFolder, *m_openFolder, *m_newSession;
    // U25 part 7: the header, the three blocks and the sessions table.
    QLabel *m_headerIcon = nullptr, *m_headerName = nullptr, *m_summary = nullptr;
    QPushButton *m_newTeam = nullptr;
    CollapsibleBlock *m_foldersBlock = nullptr, *m_sessionsBlock = nullptr, *m_rulesBlock = nullptr;
    QTableWidget *m_sessions = nullptr;
    IronRulesPanel *m_rules = nullptr;
    std::function<QList<ProjectSession>(const QString &)> m_sessionsProvider;
};
