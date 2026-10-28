#pragma once
#include "TaskDrag.h"
#include "WorkspaceList.h"
#include <QTimer>
#include <QElapsedTimer>
#include <QPersistentModelIndex>
#include <QRegion>

namespace SessionRoles {
enum Role { Key = Qt::UserRole, Title, Meta, Status, Detail, Agent, Host, Children,
            Group, Header, Identity, Collapsed, Total, Attention, Working, Model, Effort, MachineColor, MachineName, Unread,
            ChildId, ParentKey, HasChildren, Expanded, ProjectColor, ProjectVivid, LaunchId, WorkingSince, ReviewLater, Draft, Failure,
            GitStatus, GitHost, GitPath, MetaIcon, Scheduled, OptionalQuestions, ModifiedAt, CoordinationWait, Waiting,
            Subordinate,
            // WhiteWh fork: agent teams. Members are Subordinate rows with
            // TeamMember (2 for the last); the lead's row carries its team
            // (id, name, status, label, tone, meta, members, answers, progress)
            // for the strip in its card.
            TeamMember,
            TeamLead,
            // G7: the services the session runs (name, command, since); shown as
            // an icon on the row, never as Background work.
            Services,
            // U25 part 10 (spec 2.5): a member's level under its lead (1 for the
            // lead's direct reports, 2 below a sub-lead) and, per level above
            // its own, whether that level's line runs past the row (a list of
            // bools, TeamTree::through).
            TeamDepth,
            TeamLines,
            // U25 (spec 2.2): current background commands (count and the
            // first command) for the card's chip; 0 when none are confirmed.
            BackgroundJobs,
            BackgroundCommand,
            // U25 (spec 2.3): muted by its team's default, not by its own choice.
            TeamMuted,
            // U25 (spec 2.1): a project's first folder, for its '+' tooltip.
            ProjectFolder,
            Muted = Qt::UserRole + 50, RowCollapsed = Qt::UserRole + 51,
            // U30 (#508): the facts of the session's state (SessionStates::facts).
            SessionState = Qt::UserRole + 52,
            // {host, name} of a live session a dragged task can be pinned to; empty for other rows.
            // The fork's own roles fill UserRole + 1 to 49, so it follows them explicitly.
            TaskTarget = Qt::UserRole + 53,
            // Of the prepared row last written to the item (SessionsWindow::applyListRows);
            // empty after a role is written elsewhere, so the next applied rows rewrite it.
            Digest = Qt::UserRole + 54 };
}
class QToolButton;

// Drops express placement relative to an identity, never a filtered row number.
// A task dragged from the Tasks tab drops on a live session of its project, or on
// the project's header for Any session.
class SessionList : public WorkspaceList {
    Q_OBJECT
public:
    explicit SessionList(QWidget *parent = nullptr);
    ~SessionList() override;
    void setActivityAnimationEnabled(bool enabled);
    void syncActivityAnimation();
    bool activityAnimationRunning() const { return m_activityTimer.isActive(); }
    bool elapsedTimerRunning() const { return m_elapsedTimer.isActive(); }
    const QRegion &currentPaintRegion() const { return m_paintRegion; }
    bool dragging() const { return m_dragging; }
    bool bulkSelecting() const { return m_bulkSelecting; }
    void clearBulkSelection();
    void setSelectionBar(QWidget *bar, bool visible);
    void syncRowExpandButtons();
    static QString childrenLabel(const QModelIndex &index);
    static QRect childrenControlRect(const QRect &row, const QModelIndex &index, QFont font);
signals:
    void bulkSelectionChanged();
    void groupToggled(const QString &group);
    void childrenToggled(const QString &parentKey);
    void rowExpanded(const QString &key);
    void rowCompacted(const QString &key);
    void sessionMoved(const QString &identity, const QString &group, const QString &before);
    void groupMoved(const QString &group, const QString &before);
    // WhiteWh fork: a lead's team strip was clicked.
    void teamActivated(const QString &team);
    // U25: a project's '+' (spec 2.1) and a card's chip (spec 2.2) were clicked.
    void projectNewSession(const QString &group);
    void chipActivated(const QString &key, int kind);
    // U25 part 6: a card's git chip was clicked (the session's key).
    void gitActivated(const QString &key);
    // U30 (#508): a cold card's + New ("new") or Resume ("resume").
    void coldActionRequested(const QString &key, const QString &action);
    void dragFinished();
    // A task was dropped on a session (host "" for this computer) or, with an empty
    // session, on its project's header.
    void taskDropped(const TaskDrag &task, const QString &host, const QString &session);
protected:
    QStringList mimeTypes() const override { return {QStringLiteral("application/x-hgs-session-placement")}; }
    QMimeData *mimeData(const QList<QListWidgetItem *> &) const override;
    void startDrag(Qt::DropActions) override;
    void dragEnterEvent(QDragEnterEvent *) override;
    void dragMoveEvent(QDragMoveEvent *) override;
    void dragLeaveEvent(QDragLeaveEvent *) override;
    void dropEvent(QDropEvent *) override;
    void mousePressEvent(QMouseEvent *) override;
    void mouseDoubleClickEvent(QMouseEvent *) override;
    void mouseMoveEvent(QMouseEvent *) override;
    void mouseReleaseEvent(QMouseEvent *) override;
    void keyPressEvent(QKeyEvent *) override;
public:
    // The team strip at the bottom of a lead's card; a click opens the team.
    static constexpr int TeamStripHeight = 32;
    static QRect teamStripRect(const QRect &row) { return {row.left() + 8, row.bottom() - 6 - 26, row.width() - 16, 26}; }
    // U25 (spec 2.1): the 28 px '+' at the right end of a project's band.
    static QRect projectAddRect(const QRect &row) { const QRect band = row.adjusted(2, 5, -2, -3); return {band.right() - 27, band.y(), 28, band.height()}; }
protected:
    void paintEvent(QPaintEvent *) override;
    void showEvent(QShowEvent *) override;
    void hideEvent(QHideEvent *) override;
    void resizeEvent(QResizeEvent *) override;
    bool viewportEvent(QEvent *) override;
    bool event(QEvent *) override;
private:
    bool rangeSelectable(const QListWidgetItem *item) const;
    int rangeRow(const QString &key) const;
    void selectRange(int targetRow, bool additive);
    QString m_rangeAnchor, m_rangeCursor;
    void scheduleRowExpandButtons();
    QHash<QPersistentModelIndex, QToolButton *> m_rowExpandButtons;
    bool m_rowExpandButtonsPending = false;
    bool m_rowFullLayout = true;
    QElapsedTimer m_expandClickClock;
    QString m_expandClickKey;
    QRect m_expandClickRect;
    QRect childrenControlRect(const QListWidgetItem *item) const;
    QStyleOptionViewItem animationOption(const QModelIndex &index) const;
    QList<QPersistentModelIndex> m_visibleWorkingRows;
    QHash<QPersistentModelIndex, QRegion> m_animationRegions;
    QRegion m_paintRegion;
    QTimer m_activityTimer, m_elapsedTimer;
    QElapsedTimer m_activityClock;
    bool m_animateActivity = true;
    // A session or group this list dragged: a QDataStream, which stays in this process.
    struct Dragged { QString token; bool group = false; QString id; };
    static std::optional<Dragged> dragged(const QMimeData *mime);
    bool validDrop(const QMimeData *) const;
    // The row a dragged task would change: a live session of its project other than
    // its own, or the project's header for a pinned task; null elsewhere.
    const QListWidgetItem *taskDropItem(const TaskDrag &task, const QPoint &point) const;
    struct Placement { QString group, before; QRect indicator; bool onGroup = false; };
    Placement placement(const QPoint &point, bool group) const;
    QPoint m_press;
    QString m_pressedTeam, m_pressedGroup, m_pressedAdd, m_pressedChip, m_pressedCold;
    QString hoverTarget(const QPoint &point) const;
    QString m_pressedParent;
    bool m_dragging = false;
    QRect m_indicator;
    bool m_onGroup = false, m_taskDrop = false;
    QString m_token;
    bool m_bulkSelecting = false, m_togglePress = false;
    QWidget *m_selectionBar = nullptr;
};
