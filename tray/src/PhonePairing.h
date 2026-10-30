#pragma once
#include <QColor>
#include <QDateTime>
#include <QDialog>
#include <QJsonObject>
#include <QList>
#include <QPointer>
#include <QSet>
#include <QStringList>
#include <QWidget>
#include <functional>
#include <vector>

class HgsClient; class QVBoxLayout; class QComboBox; class QLabel; class QPlainTextEdit; class QPushButton; class QScrollArea; class QStackedWidget; class QTimer; class QTreeWidget;

// Phones paired with the workspaces this computer reaches: through its own
// mobile connection, or through one enabled peer that has one (hgs @peer
// mobile, one hop). Pairing shows a single-use QR code; revoking needs
// confirmation. See docs/superpowers/specs/2026-10-17-desktop-phone-management-design.md.
namespace PhonePairing {

// A workspace reached through a host ("" is this computer).
struct Target {
    QString host, workspaceId, workspaceName, serverUrl;
};

// The QR modules of `text` (row-major, true = dark), with the side length.
// Empty for text that does not fit a QR code.
std::vector<bool> encode(const QString &text, int *size);

// #647 (prototype, Phones): a time as the table shows it: "Today 23:02",
// "Yesterday 09:10", "24 Oct 18:40" this year, "3 Sep 2025" before; without
// the time "Today", "12 Oct". "—" for none.
QString shortTime(const QJsonValue &seconds, bool withTime, const QDateTime &now);

class QrCodeView : public QWidget {
public:
    explicit QrCodeView(QWidget *parent = nullptr);
    void setText(const QString &text);
    void setFaded(bool faded);
    QString text() const { return m_text; }
    int modules() const { return m_size; }
    QSize sizeHint() const override;
protected:
    void paintEvent(QPaintEvent *) override;
private:
    QString m_text;
    std::vector<bool> m_modules;
    int m_size = 0;
    bool m_faded = false;
};

class Dialog : public QDialog {
public:
    // `now` (seconds since the epoch) is injectable for tests.
    Dialog(const QString &executable, const QList<Target> &targets, int selected, QWidget *parent = nullptr,
           std::function<double()> now = {});
    void request();
protected:
    void resizeEvent(QResizeEvent *event) override;
    bool eventFilter(QObject *object, QEvent *event) override;
private:
    void showInvitation(const QJsonObject &invitation);
    void sizeQrCode();
    void tick();
    HgsClient *m_client;
    QList<Target> m_targets;
    std::function<double()> m_now;
    QComboBox *m_workspace = nullptr;
    QrCodeView *m_code;
    QScrollArea *m_scroll;
    QStackedWidget *m_qr;
    QLabel *m_qrStatus, *m_server, *m_expiry, *m_status;
    QPlainTextEdit *m_pairCode;
    QPushButton *m_copyCode, *m_copyLink, *m_again;
    QTimer *m_timer;
    QString m_link, m_codeText;
    Target m_current;
    double m_expires = 0;
    quint64 m_request = 0;
};

class Panel : public QWidget {
public:
    // `confirm` replaces the revoke confirmation in tests.
    explicit Panel(const QString &executable, QWidget *parent = nullptr,
                   std::function<bool(const QString &phone, const QString &workspace)> confirm = {});
    void refresh();
    QList<Target> targets() const { return m_targets; }
    // #647: the table, its notes and problem lines in the page's theme (#650
    // N1: the window's until a page sets it).
    void setTheme(bool dark);
    bool dark() const { return m_dark; }
    // #650 L6: the workspace Pair a phone… opens on (a target index).
    int chosenWorkspace() const { return m_chosen; }
protected:
    void showEvent(QShowEvent *) override;
private:
    void finish();
    void render();
    void revoke();
    void pair();
    void openPairing();
    void showStatus(const QString &text);
    void chooseWorkspace(int index);
    void paintGroups();
    QString m_executable;
    HgsClient *m_client;
    std::function<bool(const QString &, const QString &)> m_confirm;
    QTreeWidget *m_tree;
    // The count beside the title, a notice or setup line, and a line per
    // computer that did not answer (#647).
    QLabel *m_status, *m_count;
    QVBoxLayout *m_problemList;
    QColor m_groupFill, m_warnColor, m_chosenFill;
    int m_chosen = 0;
    bool m_dark = false;
    QPushButton *m_pair, *m_revoke, *m_refresh;
    QList<Target> m_targets;
    QList<QJsonObject> m_lists;
    QStringList m_hostOrder;
    QStringList m_problems;
    // Kept until an explicit refresh: a failed or unconfirmed revocation is never retried.
    QString m_notice, m_revokingName;
    QSet<quint64> m_pending;
    quint64 m_machines = 0, m_revoking = 0;
    Target m_pairTarget;
    bool m_refreshQueued = false, m_pairRequested = false;
};

} // namespace PhonePairing
