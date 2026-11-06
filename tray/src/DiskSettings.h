#pragma once
#include <QJsonArray>
#include <QJsonObject>
#include <QTimer>
#include <QWidget>
#include <functional>

class QCheckBox;
class QComboBox;
class QLabel;
class QPushButton;
class QSpinBox;
class QVBoxLayout;
class TeamController;

// WhiteWh fork, task 108 (owner, 2026-11-06): Settings > Build folders. What
// Zerus holds on this computer per team and folder (`hgs team disk`), the
// cleanup policy (on by default: a finished team's folders, folders unused for
// N days, oldest first while a drive is low), a dry run and Clean now. Clean
// now asks first (Cancel focused). Zerus removes only build folders it created
// or was told to use, never a folder of the person's, never sources, never a
// folder in use; a removal that may have half happened waits for review.
class DiskSettings : public QWidget {
public:
    explicit DiskSettings(TeamController *teams, QWidget *parent = nullptr);
    // Reads the view again (and measures with `refresh`).
    void refresh(bool measure = false);
    // Asks before Clean now (tests answer it); empty: a message box with Cancel focused.
    std::function<bool(const QString &name, const QString &title, const QString &text, const QString &action)> confirm;
    // The last view and the last dry run or clean, for tests.
    QJsonObject view() const { return m_view; }
    QJsonArray decisions() const { return m_decisions; }
    static QString size(qint64 bytes, bool known = true);
    // Task 109: when a build last used a folder, and what its row says about it
    // (busy, waits for review, not Zerus's, gone, due or removable).
    static QString lastUse(double at, double now);
    static QString folderState(const QJsonObject &folder);
    static QString folderKind(const QJsonObject &folder);
    // Task 109: while shown, the view reads again this often (measuring a few
    // folders each time); tests shorten it.
    void setLiveInterval(int ms) { m_live.setInterval(ms); }
    // The live read, as the timer runs it.
    void poll();
protected:
    void showEvent(QShowEvent *event) override;
    void hideEvent(QHideEvent *event) override;
private:
    void render();
    void clean(bool dry);
    void savePolicy();
    void forget(const QString &path);
    void send(const QJsonObject &input, const std::function<void(const QJsonObject &)> &then);
    TeamController *m_teams;
    QCheckBox *m_enabled;
    QSpinBox *m_days, *m_freeGb, *m_freePercent;
    QComboBox *m_scope;
    QPushButton *m_dry, *m_clean, *m_measure, *m_save;
    QLabel *m_notice, *m_summary;
    QVBoxLayout *m_rows;
    QJsonObject m_view;
    QJsonArray m_decisions;
    bool m_dryShown = true, m_busy = false;
    // Task 109: the live read; its answer is dropped when an action overtook it.
    QTimer m_live;
    quint64 m_generation = 0;
    bool m_polling = false;
    // The stored policy the form last showed: a read repeating it keeps edits.
    QJsonObject m_policyShown;
    void showView(const QJsonObject &result);
};
