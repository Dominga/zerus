#pragma once
#include "TasksView.h"
#include <QJsonObject>
#include <QWidget>

class QLabel;
class QPushButton;
class ActivityView;
class QSplitter;
class QStackedWidget;
class QScrollArea;
class QVBoxLayout;
class TaskTile;
class QTabBar;
class QFrame;
class QLineEdit;
class QPlainTextEdit;
class QHBoxLayout;
class QToolButton;
class MessageComposer;
class ToolbarChip;
namespace ActivityWidth { class ColumnLayout; }

// Session tasks beside Activity. Shares the board's listing, but fetches only
// the selected task's history. It never infers task execution from native work.
class TaskInspector : public QWidget {
    Q_OBJECT
public:
    TaskInspector(HgsClient *client, TasksView *board, QWidget *parent = nullptr);
    void setContext(const QString &project, const QString &host, const QString &session, const QList<TaskSession> &sessions);
    void setTheme(bool dark);
    void setContentScale(double scale);
    void setColumnWidth(int width);
    void selectTask(const QString &uuid);
    void openTask(const QString &uuid) { selectTask(uuid); if (m_selected == uuid) emit taskViewRequested(); }
    QString selectedTask() const { return m_selected; }
    QString selectedStage() const { return m_stage; }
    int displayedCount() const { return m_rows.size(); }
    // One central Task View owns these widgets; the right pane remains a board.
    QWidget *takeDetailsPane();
    MessageComposer *composer() const { return m_commentComposer; }
    bool canDictate() const { return !selectionPending() && !m_detail.isEmpty() && m_detail.value("writable").toBool(true) && !m_noteBusy; }
signals:
    void taskViewRequested();
    void executorRequested(const QString &host, const QString &name, const QString &id);
    void subagentRequested(const QString &host, const QString &parent, const QString &parentId, const QString &conversation, const QString &child);
protected:
    void showEvent(QShowEvent *event) override;
    void hideEvent(QHideEvent *event) override;
    void resizeEvent(QResizeEvent *event) override;
    bool eventFilter(QObject *watched, QEvent *event) override;
private:
    void refresh();
    bool descriptionMode(const QJsonObject &task) const;
    QString inputKey(const QJsonObject &task) const;
    void seedDescriptionDraft();
    void createCard();
    void beginRename();
    void finishRename();
    void cancelRename();
    void restoreNewCard();
    void fetch();
    void renderDetail();
    void updateFilters();
    void openInlineDetails();
    void closeInlineDetails();
    bool canLeaveDetails();
    void renderCommands(const TaskItem &task);
    void showAssignment();
    void publishDetail(QJsonObject task);
    void publishJournal(const QString &key, const QJsonArray &entries, bool followLatest);
    bool selectionPending() const;
    QList<TaskRow *> shownRows(const QString &stage = {}) const;
    int dropGap(const QString &uuid, const QPoint &point, const QString &stage = {}) const;
    void followDrag(const TaskDrag &drag, const QPoint &point);
    void endListDrag();
    QJsonObject labels(QJsonObject task) const;
    const TaskSession *executor() const;
    HgsClient *m_client;
    TasksView *m_board;
    QString m_project, m_host, m_session, m_sessionId, m_selected, m_stage;
    QList<TaskSession> m_sessions;
    QJsonObject m_detail;
    quint64 m_request = 0, m_generation = 0;
    QLabel *m_scope, *m_state;
    QLabel *m_title, *m_identity;
    QPlainTextEdit *m_titleEditor;
    QLabel *m_titleError;
    QString m_renameRequested;
    bool m_titleSaving = false;
    ActivityWidth::ColumnLayout *m_headerColumn, *m_inputColumn;
    ToolbarChip *m_messageMode;
    QPushButton *m_cancelDescription;
    QPushButton *m_checkMessage;
    bool m_noteOnly = false;
    QToolButton *m_add;
    QLineEdit *m_newTitle;
    QStackedWidget *m_creationHeader;
    QLabel *m_createError;
    QJsonObject m_creation;
    bool m_creating = false, m_detached = false, m_descriptionEditing = false, m_descriptionSeed = false;
    QWidget *m_filters = nullptr, *m_listPage = nullptr, *m_details = nullptr;
    QScrollArea *m_scroll = nullptr;
    QVBoxLayout *m_cards = nullptr;
    QHash<QString, TaskTile *> m_tiles;
    QHash<QString, TaskRow *> m_rows;
    QPushButton *m_older;
    ActivityView *m_activity;
    QSplitter *m_splitter;
    QStackedWidget *m_content;
    QLabel *m_emptyDetail;
    QPushButton *m_moreDetail, *m_latestHistory;
    QJsonObject m_historyBefore;
    QPushButton *m_open, *m_edit;
    QPushButton *m_assignment;
    QWidget *m_assignmentRow;
    MessageComposer *m_commentComposer;
    QJsonObject m_pendingDetail;
    QJsonArray m_journalSource;
    QString m_journalKey;
    quint64 m_journalGeneration = 0;
    QLabel *m_blocker;
    QPushButton *m_reviewLaunch;
    QLabel *m_noteError;
    QWidget *m_commands;
    QHBoxLayout *m_commandLayout;
    QHash<int, QPushButton *> m_commandButtons;
    QString m_commandsKey, m_noteKey, m_inlineRequested;
    bool m_noteBusy = false;
    QTabBar *m_detailTabs;
    bool m_dark = true;
    bool m_showOlder = false;
    int m_historyVisible = 8;
    QFrame *m_dropLine;
    QTimer m_autoScroll;
    TaskDrag m_drag;
    QPoint m_dragAt;
    int m_scrollStep = 0;
    bool m_overList = false;
};
