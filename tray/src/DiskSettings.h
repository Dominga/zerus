#pragma once
#include <QJsonArray>
#include <QJsonObject>
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
};
