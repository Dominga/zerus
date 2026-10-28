#pragma once
#include "SessionAccounts.h"
#include <QWidget>
class QPushButton;
class QMenu;
class QLayout;

class AccountFilter : public QWidget {
    Q_OBJECT
public:
    explicit AccountFilter(QWidget *parent = nullptr);
    QPushButton *button() const { return m_button; }
    QSet<QString> selection() const { return m_selection; }
    void setSelection(const QSet<QString> &bindings);
    void setAccounts(const QList<SessionAccounts::Account> &accounts);
    void setSessionCounts(const QMap<QString, int> &counts, const QString &caption);
    void setTheme(bool dark);
    void setConcealed(bool concealed);
signals:
    void selectionChanged(const QSet<QString> &bindings);
    void opened();
private:
    void rebuildChips();
    void rebuildMenu();
    void appendChoices();
    void updateMenu();
    void toggle(const SessionAccounts::Account &account, bool checked);
    QString caption(const SessionAccounts::Account &account) const;
    QList<SessionAccounts::Account> choices() const;
    QList<SessionAccounts::Account> m_accounts;
    QSet<QString> m_selection;
    QMap<QString, int> m_counts;
    QHash<QString, SessionAccounts::Account> m_selectedAccounts;
    QString m_caption, m_signature;
    QPushButton *m_button;
    QMenu *m_menu;
    QLayout *m_flow;
    bool m_dark = false, m_concealed = false;
};
