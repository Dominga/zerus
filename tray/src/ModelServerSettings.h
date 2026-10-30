#pragma once
#include "ModelServers.h"
#include <QWidget>
#include <functional>

class QLabel;
class QLineEdit;
class QListWidget;
class QPlainTextEdit;
class QPushButton;
class QComboBox;

// WhiteWh fork, U60 (#656): Settings > Model servers (design/model-servers,
// the settings view). Adds, edits, tests and removes this computer's model
// servers through `hgs model-server save|test --json`. A saved key is never
// shown again: the form says it is in the private store and offers Replace
// and Remove. Test reads the status (or the API's model list) once and saves
// nothing; Logs reads the saved server's logs once. Up and Down are never run
// from here: on a shared server they belong to the tab and its confirmations.
class ModelServerSettings : public QWidget {
public:
    explicit ModelServerSettings(ModelServers::Controller *servers, QWidget *parent = nullptr);
    void setDark(bool dark);
    // Opens a server's form (empty: the first, or a new one when none).
    void showServer(const QString &name);
    void addServer();
    std::function<QStringList()> machines;
    // Asks before Delete server… (tests answer it); empty: a message box.
    std::function<bool(const QString &title, const QString &text, const QString &action)> confirm;
private:
    void renderList();
    void load(const ModelServers::Server *server);
    QJsonObject formServer(bool withControl) const;
    void save();
    void remove();
    void test(const QString &what);
    void showResult(QLabel *target, const QJsonObject &result, const QString &error, const QString &what);
    void setKeyMode();
    ModelServers::Controller *m_servers;
    QListWidget *m_list;
    QWidget *m_form;
    QLineEdit *m_name, *m_url, *m_key, *m_alias, *m_status, *m_up, *m_down, *m_logs;
    QPushButton *m_openai, *m_anthropic, *m_sshNone, *m_sshMachine, *m_sshAlias, *m_replace, *m_removeKey, *m_delete, *m_save, *m_testLogs;
    QComboBox *m_machine;
    QLabel *m_stored, *m_apiResult, *m_statusResult, *m_logsResult, *m_saveError, *m_title;
    QPlainTextEdit *m_raw;
    QString m_was;          // the server being edited; empty for a new one
    bool m_hasKey = false;  // a key is stored for m_was
    QString m_keyChange;    // "", "replace" or "remove"
    bool m_dark = true;
};
