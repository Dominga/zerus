#include "PhonePairing.h"
#include "HgsClient.h"
#include "PageCards.h"
#include "qrcodegen.hpp"
#include <QApplication>
#include <QClipboard>
#include <QComboBox>
#include <QDateTime>
#include <QFontDatabase>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QJsonArray>
#include <QLabel>
#include <QLocale>
#include <QMessageBox>
#include <QPainter>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QResizeEvent>
#include <QScreen>
#include <QScrollArea>
#include <QScrollBar>
#include <QStackedWidget>
#include <QTextOption>
#include <QTimer>
#include <QToolButton>
#include <QTreeWidget>
#include <QTreeWidgetItemIterator>
#include <QVBoxLayout>
#include <algorithm>
#include <cmath>
#include <utility>

namespace PhonePairing {
namespace {
constexpr int QuietZone = 4;
enum Role { KindRole = Qt::UserRole, IdRole, HostRole, WorkspaceRole, TargetRole, WorkspaceIdRole, ServerRole };
QString hostName(const QString &host) { return host.isEmpty() ? QCoreApplication::translate("PhonePairing", "this computer") : host; }
QString full(const QJsonValue &seconds) {
    if (!seconds.isDouble() || seconds.toDouble() <= 0) return {};
    return QLocale(QLocale::English).toString(QDateTime::fromMSecsSinceEpoch(qint64(seconds.toDouble() * 1000)), QStringLiteral("d MMMM yyyy, HH:mm"));
}
// #647 (prototype, Phones): a flat table on the card, only as tall as its
// rows, its columns in the prototype's shares.
constexpr int PhoneRowHeight = 29;
class PhoneTable final : public QTreeWidget {
public:
    using QTreeWidget::QTreeWidget;
    void fit() {
        int rows = 0;
        for (QTreeWidgetItemIterator it(this); *it; ++it) ++rows;
        setFixedHeight(header()->sizeHint().height() + rows * PhoneRowHeight + 2 * frameWidth() + 2);
    }
protected:
    void resizeEvent(QResizeEvent *event) override {
        QTreeWidget::resizeEvent(event);
        static constexpr double share[] = {.31, .16, .14, .21};
        const int width = viewport()->width();
        for (int i = 0; i < 4; ++i) setColumnWidth(i, int(width * share[i]));
    }
};
// A problem in one short line; the whole text is its tooltip.
QString shortProblem(const QString &text) {
    QString line = text.section('\n', 0, 0).simplified();
    if (line.size() > 160) line = line.left(159) + QStringLiteral("…");
    return line;
}
QString origin(QString url) { while (url.endsWith('/')) url.chop(1); return url; }
// Pins the relay and workspace the user saw, so a gateway whose connection
// changed meanwhile neither issues nor revokes for another workspace.
QStringList pinned(const QString &action, const QString &id, const QString &workspace, const QString &server) {
    QStringList args{action};
    if (!id.isEmpty()) args << id;
    if (!workspace.isEmpty() && !server.isEmpty()) args << QStringLiteral("--workspace") << workspace << QStringLiteral("--server") << server;
    return args << QStringLiteral("--json");
}
QString platform(const QString &value) {
    if (value == QLatin1String("ios")) return QStringLiteral("iOS");
    if (value == QLatin1String("android")) return QStringLiteral("Android");
    return QStringLiteral("—");
}
} // namespace

QString shortTime(const QJsonValue &seconds, bool withTime, const QDateTime &now) {
    if (!seconds.isDouble() || seconds.toDouble() <= 0) return QStringLiteral("—");
    const auto at = QDateTime::fromMSecsSinceEpoch(qint64(seconds.toDouble() * 1000)).toLocalTime();
    const QString time = at.toString(QStringLiteral("HH:mm"));
    const qint64 days = at.date().daysTo(now.toLocalTime().date());
    if (days == 0) return withTime ? QCoreApplication::translate("PhonePairing", "Today %1").arg(time) : QCoreApplication::translate("PhonePairing", "Today");
    if (days == 1) return withTime ? QCoreApplication::translate("PhonePairing", "Yesterday %1").arg(time) : QCoreApplication::translate("PhonePairing", "Yesterday");
    const QString date = QLocale(QLocale::English).toString(at.date(), at.date().year() == now.toLocalTime().date().year() ? QStringLiteral("d MMM") : QStringLiteral("d MMM yyyy"));
    return withTime ? date + QLatin1Char(' ') + time : date;
}

std::vector<bool> encode(const QString &text, int *size) {
    *size = 0;
    const auto utf8 = text.toUtf8();
    if (utf8.isEmpty() || utf8.contains('\0')) return {};
    try {
        const auto qr = qrcodegen::QrCode::encodeText(utf8.constData(), qrcodegen::QrCode::Ecc::MEDIUM);
        const int n = qr.getSize();
        std::vector<bool> modules(size_t(n) * size_t(n));
        for (int y = 0; y < n; ++y)
            for (int x = 0; x < n; ++x) modules[size_t(y) * size_t(n) + size_t(x)] = qr.getModule(x, y);
        *size = n;
        return modules;
    } catch (const std::exception &) {
        return {};
    }
}

QrCodeView::QrCodeView(QWidget *parent) : QWidget(parent) {
    setObjectName("phonePairingCode"); setMinimumSize(200, 200);
    setAccessibleName(tr("Pairing QR code"));
}
QSize QrCodeView::sizeHint() const {
    const int side = qMax(264, (m_size + 2 * QuietZone) * 3);
    return {side, side};
}
void QrCodeView::setText(const QString &text) { m_text = text; m_modules = encode(text, &m_size); updateGeometry(); update(); }
void QrCodeView::setFaded(bool faded) { m_faded = faded; update(); }
void QrCodeView::paintEvent(QPaintEvent *) {
    QPainter p(this);
    const int side = qMin(width(), height());
    const QRect square((width() - side) / 2, (height() - side) / 2, side, side);
    // Dark modules on white in every theme: phone cameras need the contrast.
    p.fillRect(square, Qt::white);
    if (!m_size) return;
    const int cell = side / (m_size + 2 * QuietZone);
    if (cell < 1) return;
    const int origin = (side - cell * m_size) / 2;
    if (m_faded) p.setOpacity(0.15);
    for (int y = 0; y < m_size; ++y)
        for (int x = 0; x < m_size; ++x)
            if (m_modules[size_t(y) * size_t(m_size) + size_t(x)])
                p.fillRect(square.x() + origin + x * cell, square.y() + origin + y * cell, cell, cell, Qt::black);
}

Dialog::Dialog(const QString &executable, const QList<Target> &targets, int selected, QWidget *parent, std::function<double()> now)
    : QDialog(parent), m_client(new HgsClient(executable, this)), m_targets(targets), m_now(std::move(now)) {
    if (!m_now) m_now = [] { return QDateTime::currentMSecsSinceEpoch() / 1000.0; };
    setObjectName("phonePairingDialog"); setWindowTitle(tr("Pair a phone"));
    auto *layout = new QVBoxLayout(this); layout->setSpacing(12);
    m_scroll = new QScrollArea; m_scroll->setObjectName("phonePairingScroll");
    m_scroll->setAutoFillBackground(false); m_scroll->viewport()->setAutoFillBackground(false);
    m_scroll->setWidgetResizable(true); m_scroll->setFrameShape(QFrame::NoFrame);
    m_scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    auto *body = new QWidget;
    body->setAutoFillBackground(false);
    auto *content = new QVBoxLayout(body); content->setContentsMargins(0, 0, 0, 0); content->setSpacing(12);
    m_scroll->setWidget(body); layout->addWidget(m_scroll, 1);
    if (m_targets.size() > 1) {
        m_workspace = new QComboBox; m_workspace->setObjectName("phonePairingWorkspace"); m_workspace->setAccessibleName(tr("Workspace"));
        m_workspace->setSizeAdjustPolicy(QComboBox::AdjustToMinimumContentsLengthWithIcon); m_workspace->setMinimumContentsLength(20);
        for (const auto &target : m_targets) m_workspace->addItem(tr("%1 (via %2)").arg(target.workspaceName, hostName(target.host)));
        m_workspace->setCurrentIndex(qBound(0, selected, int(m_targets.size()) - 1));
        auto *row = new QFormLayout; row->addRow(tr("Workspace"), m_workspace); content->addLayout(row);
        connect(m_workspace, &QComboBox::currentIndexChanged, this, [this] { request(); });
    }
    m_qr = new QStackedWidget; m_qr->setObjectName("phonePairingQr");
    m_code = new QrCodeView; m_qr->addWidget(m_code);
    m_qrStatus = new QLabel; m_qrStatus->setObjectName("phonePairingQrStatus");
    m_qrStatus->setAlignment(Qt::AlignCenter); m_qrStatus->setWordWrap(true); m_qrStatus->setTextFormat(Qt::PlainText);
    m_qrStatus->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
    m_qr->addWidget(m_qrStatus); content->addWidget(m_qr);
    auto *hint = new QLabel(tr("In Zerus on your phone, open Machines → Pair another workspace and scan this code. "
                               "It works once, for ten minutes; treat it like a password."));
    hint->setWordWrap(true); content->addWidget(hint);
    m_server = new QLabel; m_server->setObjectName("phonePairingServer");
    m_server->setTextFormat(Qt::PlainText); m_server->setTextInteractionFlags(Qt::TextSelectableByMouse);
    m_server->setWordWrap(true); m_server->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
    auto *serverRow = new QHBoxLayout; serverRow->addWidget(new QLabel(tr("Server"))); serverRow->addWidget(m_server, 1);
    auto *manual = new QToolButton; manual->setObjectName("showPairingCode");
    manual->setText(tr("Enter the code manually")); manual->setCheckable(true);
    manual->setToolButtonStyle(Qt::ToolButtonTextBesideIcon); manual->setArrowType(Qt::RightArrow);
    serverRow->addWidget(manual); content->addLayout(serverRow);
    m_pairCode = new QPlainTextEdit; m_pairCode->setObjectName("phonePairingCodeText");
    m_pairCode->setAccessibleName(tr("Pairing code")); m_pairCode->setReadOnly(true); m_pairCode->setTabChangesFocus(true);
    m_pairCode->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
    m_pairCode->setWordWrapMode(QTextOption::WrapAnywhere);
    m_pairCode->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    m_pairCode->setFixedHeight(m_pairCode->fontMetrics().lineSpacing() * 3 + 16);
    m_pairCode->hide(); content->addWidget(m_pairCode);
    connect(manual, &QToolButton::toggled, this, [this, manual](bool visible) {
        m_pairCode->setVisible(visible); manual->setArrowType(visible ? Qt::DownArrow : Qt::RightArrow);
        QTimer::singleShot(0, this, [this, manual] {
            if (manual->isChecked()) m_scroll->ensureWidgetVisible(m_pairCode);
            else m_scroll->verticalScrollBar()->setValue(0);
        });
    });
    content->addStretch();
    m_expiry = new QLabel; m_expiry->setObjectName("phonePairingExpiry"); layout->addWidget(m_expiry);
    m_status = new QLabel; m_status->setObjectName("phonePairingStatus"); m_status->setWordWrap(true); m_status->setTextFormat(Qt::PlainText);
    m_status->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
    m_status->setTextInteractionFlags(Qt::TextSelectableByMouse); layout->addWidget(m_status);
    auto *buttons = new QHBoxLayout;
    m_copyCode = new QPushButton(tr("Copy code")); m_copyCode->setObjectName("copyPairingCode");
    m_copyLink = new QPushButton(tr("Copy link")); m_copyLink->setObjectName("copyPairingLink");
    m_again = new QPushButton(tr("New code")); m_again->setObjectName("newPairingCode");
    auto *close = new QPushButton(tr("Close")); close->setObjectName("closePhonePairing");
    buttons->addWidget(m_copyCode); buttons->addWidget(m_copyLink); buttons->addWidget(m_again); buttons->addStretch(); buttons->addWidget(close);
    layout->addLayout(buttons);
    connect(m_copyCode, &QPushButton::clicked, this, [this] { QApplication::clipboard()->setText(m_codeText); });
    connect(m_copyLink, &QPushButton::clicked, this, [this] { QApplication::clipboard()->setText(m_link); });
    connect(m_again, &QPushButton::clicked, this, [this] { request(); });
    connect(close, &QPushButton::clicked, this, &QDialog::accept);
    m_timer = new QTimer(this); m_timer->setInterval(1000);
    connect(m_timer, &QTimer::timeout, this, [this] { tick(); });
    connect(m_client, &HgsClient::mobileReady, this, [this](quint64 request, const QString &, const QJsonObject &result) {
        if (request != m_request) return;
        m_request = 0; showInvitation(result);
    });
    connect(m_client, &HgsClient::mobileFailed, this, [this](quint64 request, const QString &, const QString &error) {
        if (request != m_request) return;
        m_request = 0; m_status->setText(error); m_qrStatus->setText(tr("Pairing code unavailable")); m_again->setEnabled(true);
    });
    // The invitation stays a credential until it is used or expires.
    connect(this, &QDialog::finished, this, [this] { m_link.clear(); m_codeText.clear(); m_pairCode->clear(); m_code->setText({}); m_timer->stop(); });
    m_scroll->viewport()->installEventFilter(this);
    const auto available = screen()->availableGeometry().size() - QSize(48, 48);
    resize(qMin(640, available.width()), qMin(860, available.height()));
    request();
}

void Dialog::resizeEvent(QResizeEvent *event) { QDialog::resizeEvent(event); sizeQrCode(); }

bool Dialog::eventFilter(QObject *object, QEvent *event) {
    if (object == m_scroll->viewport() && event->type() == QEvent::Resize) sizeQrCode();
    return QDialog::eventFilter(object, event);
}

void Dialog::sizeQrCode() {
    // Keep the entire square visible before the scrollable manual details.
    // Dense encrypted invitations need more pixels than legacy short codes.
    const int side = qMax(200, std::min({m_code->sizeHint().height(), m_scroll->viewport()->width(), m_scroll->viewport()->height() - m_qr->y()}));
    m_qr->setFixedHeight(side);
}

void Dialog::request() {
    m_qr->setCurrentWidget(m_qrStatus); m_qrStatus->setText(tr("Requesting a pairing code…"));
    if (m_targets.isEmpty()) {
        m_qrStatus->setText(tr("No workspace is available. Close this dialog and refresh Phones."));
        m_copyCode->setEnabled(false); m_copyLink->setEnabled(false); m_again->setEnabled(false);
        return;
    }
    const auto target = m_current = m_targets.value(m_workspace ? m_workspace->currentIndex() : 0);
    m_link.clear(); m_codeText.clear(); m_expires = 0; m_timer->stop();
    m_code->setText({}); m_code->setFaded(false);
    m_server->setText(target.serverUrl); m_pairCode->clear(); m_expiry->clear();
    m_status->setText(tr("Requesting a pairing code from %1…").arg(hostName(target.host)));
    m_copyCode->setEnabled(false); m_copyLink->setEnabled(false); m_again->setEnabled(false);
    m_request = m_client->requestMobile(target.host, pinned(QStringLiteral("invite"), {}, target.workspaceId, target.serverUrl));
}

void Dialog::showInvitation(const QJsonObject &invitation) {
    m_link = invitation.value("pair_url").toString(); m_codeText = invitation.value("pair_code").toString();
    m_expires = invitation.value("expires_at").toDouble();
    m_again->setEnabled(true);
    if (!m_link.startsWith(QLatin1String("zerus://pair?")) || m_codeText.isEmpty()) {
        m_link.clear(); m_codeText.clear(); m_qrStatus->setText(tr("Pairing code unavailable")); m_status->setText(tr("The relay returned an invalid pairing code.")); return;
    }
    if (invitation.value("workspace_id").toString() != m_current.workspaceId
        || origin(invitation.value("server_url").toString()) != origin(m_current.serverUrl)) {
        m_link.clear(); m_codeText.clear();
        m_qrStatus->setText(tr("Pairing code unavailable"));
        m_status->setText(tr("This pairing code belongs to another workspace or relay than the one listed. Refresh the phone list and try again."));
        return;
    }
    m_code->setText(m_link);
    sizeQrCode();
    if (m_code->modules()) m_qr->setCurrentWidget(m_code);
    else m_qrStatus->setText(tr("This invitation is too large for a QR code. Use Copy code to enter it on your phone."));
    m_server->setText(invitation.value("server_url").toString());
    m_pairCode->setPlainText(m_codeText);
    m_status->setText(invitation.value("encrypted").toBool()
        ? tr("Workspace: %1\nEnd-to-end encrypted. Keep this computer online until the phone confirms pairing.").arg(invitation.value("workspace_name").toString())
        : tr("Workspace: %1").arg(invitation.value("workspace_name").toString()));
    m_copyCode->setEnabled(true); m_copyLink->setEnabled(true);
    tick(); m_timer->start();
}

void Dialog::tick() {
    const double left = m_expires - m_now();
    if (left <= 0) {
        m_timer->stop(); m_code->setFaded(true); m_copyCode->setEnabled(false); m_copyLink->setEnabled(false);
        m_expiry->setText(tr("This code expired. Choose New code to pair."));
        return;
    }
    const int seconds = int(std::ceil(left));
    m_expiry->setText(tr("Expires in %1:%2").arg(seconds / 60).arg(seconds % 60, 2, 10, QLatin1Char('0')));
}

Panel::Panel(const QString &executable, QWidget *parent, std::function<bool(const QString &, const QString &)> confirm)
    : QWidget(parent), m_executable(executable), m_client(new HgsClient(executable, this)), m_confirm(std::move(confirm)) {
    setObjectName("phonesPanel");
    auto *layout = new QVBoxLayout(this); layout->setContentsMargins(0, 0, 0, 0); layout->setSpacing(8);
    // U25 (spec 8), #647 (prototype): "Phones  N phones" with Pair a phone… as
    // the primary action on the right; the intro, a line per notice and per
    // unreachable computer, the flat table, then Revoke… and Refresh.
    auto *head = new QHBoxLayout; head->setSpacing(8);
    auto *heading = new QLabel(tr("Phones")); heading->setObjectName("phonesHeading"); heading->setStyleSheet(QStringLiteral("font-weight:600;font-size:13px;"));
    head->addWidget(heading);
    m_count = new QLabel; m_count->setObjectName("phonesCount"); m_count->setTextFormat(Qt::PlainText); head->addWidget(m_count); head->addStretch(1);
    m_pair = new QPushButton(tr("Pair a phone…")); m_pair->setObjectName("pairPhone"); m_pair->setProperty("pagePrimary", true);
    head->addWidget(m_pair);
    layout->addLayout(head);
    auto *intro = new QLabel(tr("Phones paired with your computers. A phone can control every computer of its workspace; "
                                "revoke one you no longer use."));
    intro->setObjectName("phonesIntro"); intro->setWordWrap(true); layout->addWidget(intro);
    m_status = new QLabel; m_status->setObjectName("phonesStatus"); m_status->setWordWrap(true); m_status->setTextFormat(Qt::PlainText);
    m_status->setTextInteractionFlags(Qt::TextSelectableByMouse); m_status->hide(); layout->addWidget(m_status);
    auto *problems = new QWidget; problems->setObjectName("phonesProblems");
    m_problemList = new QVBoxLayout(problems); m_problemList->setContentsMargins(0, 0, 0, 0); m_problemList->setSpacing(2);
    layout->addWidget(problems);
    auto *table = new PhoneTable; m_tree = table; m_tree->setObjectName("linkedPhones"); m_tree->setAccessibleName(tr("Phones"));
    m_tree->setHeaderLabels({tr("Phone"), tr("Platform"), tr("Paired"), tr("Last active"), tr("Added from")});
    m_tree->setSelectionMode(QAbstractItemView::SingleSelection); m_tree->setUniformRowHeights(true);
    m_tree->setRootIsDecorated(false); m_tree->setIndentation(14); m_tree->setFrameShape(QFrame::NoFrame);
    m_tree->setVerticalScrollBarPolicy(Qt::ScrollBarAlwaysOff); m_tree->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    m_tree->header()->setSectionResizeMode(QHeaderView::Interactive); m_tree->header()->setStretchLastSection(true);
    m_tree->header()->setDefaultAlignment(Qt::AlignLeft | Qt::AlignVCenter); m_tree->header()->setSectionsClickable(false);
    m_tree->setAutoFillBackground(false); m_tree->viewport()->setAutoFillBackground(false);
    table->fit(); layout->addWidget(m_tree);
    auto *buttons = new QHBoxLayout; buttons->setSpacing(8);
    m_revoke = new QPushButton(tr("Revoke…")); m_revoke->setObjectName("revokePhone");
    m_refresh = new QPushButton(tr("Refresh")); m_refresh->setObjectName("refreshPhones");
    buttons->addWidget(m_revoke); buttons->addWidget(m_refresh); buttons->addStretch(); layout->addLayout(buttons);
    setTheme(false);
    m_pair->setEnabled(false); m_revoke->setEnabled(false);
    connect(m_pair, &QPushButton::clicked, this, [this] { pair(); });
    connect(m_revoke, &QPushButton::clicked, this, [this] { revoke(); });
    connect(m_refresh, &QPushButton::clicked, this, [this] { m_notice.clear(); refresh(); });
    connect(m_tree, &QTreeWidget::itemSelectionChanged, this, [this] {
        const auto *item = m_tree->currentItem();
        m_revoke->setEnabled(!m_revoking && !m_pairRequested && !m_machines && m_pending.isEmpty()
                             && item && item->data(0, KindRole).toString() == QLatin1String("phone"));
    });
    connect(m_client, &HgsClient::machinesReady, this, [this](quint64 request, const QJsonObject &result) {
        if (request != m_machines) return;
        m_machines = 0;
        QStringList hosts{QString()};
        for (const auto &value : result.value("machines").toArray()) {
            const auto row = value.toObject();
            if (row.value("enabled").toBool() && !row.value("alias").toString().isEmpty()) hosts << row.value("alias").toString();
        }
        m_hostOrder = hosts;
        for (const auto &host : hosts) m_pending.insert(m_client->requestMobile(host, {QStringLiteral("phones"), QStringLiteral("--json")}));
    });
    connect(m_client, &HgsClient::machinesFailed, this, [this](quint64 request, const QString &error) {
        if (request != m_machines) return;
        m_machines = 0; m_problems << tr("Could not read the machine list: %1").arg(error);
        m_hostOrder = {QString()};
        m_pending.insert(m_client->requestMobile(QString(), {QStringLiteral("phones"), QStringLiteral("--json")}));
    });
    connect(m_client, &HgsClient::mobileReady, this, [this](quint64 request, const QString &host, const QJsonObject &result) {
        if (request && request == m_revoking) { m_revoking = 0; m_notice = tr("%1 revoked.").arg(m_revokingName); refresh(); return; }
        if (!m_pending.remove(request)) return;
        if (result.value("configured").toBool()) { auto list = result; list.insert("host", host); m_lists << list; }
        if (m_pending.isEmpty()) finish();
    });
    connect(m_client, &HgsClient::mobileFailed, this, [this](quint64 request, const QString &host, const QString &error) {
        if (request && request == m_revoking) {
            m_revoking = 0;
            m_notice = tr("Could not confirm revoking %1: %2. Refresh to check whether it is still listed before trying again.")
                           .arg(m_revokingName, error);
            render(); return;
        }
        if (!m_pending.remove(request)) return;
        m_problems << tr("%1: %2").arg(hostName(host), error);
        if (m_pending.isEmpty()) finish();
    });
}

void Panel::showEvent(QShowEvent *event) { QWidget::showEvent(event); refresh(); }

void Panel::refresh() {
    if (m_machines || !m_pending.isEmpty()) { m_refreshQueued = true; return; }
    m_lists.clear(); m_problems.clear();
    m_pair->setEnabled(false); m_revoke->setEnabled(false);
    m_count->setText(tr("Checking phones…")); m_refresh->setEnabled(false);
    m_machines = m_client->requestMachines({QStringLiteral("ls")});
}

void Panel::finish() {
    if (m_refreshQueued) { m_refreshQueued = false; refresh(); return; }
    // This computer first, then peers in Machines order; one route per workspace.
    std::stable_sort(m_lists.begin(), m_lists.end(), [this](const QJsonObject &a, const QJsonObject &b) {
        return m_hostOrder.indexOf(a.value("host").toString()) < m_hostOrder.indexOf(b.value("host").toString());
    });
    QList<QJsonObject> unique; QSet<QString> seen;
    for (const auto &list : std::as_const(m_lists)) {
        const auto id = list.value("workspace_id").toString();
        if (id.isEmpty() || seen.contains(id)) continue;
        seen.insert(id); unique << list;
    }
    m_lists = unique; m_targets.clear();
    for (const auto &list : std::as_const(m_lists))
        m_targets << Target{list.value("host").toString(), list.value("workspace_id").toString(),
                            list.value("workspace_name").toString(), list.value("server_url").toString()};
    render();
    if (m_pairRequested) QTimer::singleShot(0, this, [this] { openPairing(); });
}

void Panel::render() {
    const auto selected = m_tree->currentItem() ? m_tree->currentItem()->data(0, IdRole).toString() : QString();
    m_tree->clear(); int phones = 0;
    const auto now = QDateTime::currentDateTime();
    QFont bold = m_tree->font(); bold.setWeight(QFont::DemiBold);
    for (int index = 0; index < m_lists.size(); ++index) {
        const auto &list = m_lists[index]; const auto host = list.value("host").toString();
        const auto workspace = list.value("workspace_name").toString();
        auto *top = new QTreeWidgetItem(m_tree, {host.isEmpty() ? tr("%1 · this computer").arg(workspace) : tr("%1 · via %2").arg(workspace, host)});
        top->setData(0, KindRole, "workspace"); top->setData(0, TargetRole, index); top->setFirstColumnSpanned(true);
        // #647: the workspace is a bold group row on a tint; only phones are chosen.
        top->setFont(0, bold); top->setBackground(0, m_groupFill); top->setSizeHint(0, QSize(0, PhoneRowHeight));
        top->setFlags(Qt::ItemIsEnabled);
        for (const auto &value : list.value("devices").toArray()) {
            const auto device = value.toObject(); const auto added = device.value("added_by").toObject();
            auto *item = new QTreeWidgetItem(top, {device.value("name").toString(), platform(device.value("platform").toString()),
                shortTime(device.value("paired_at"), false, now), shortTime(device.value("last_seen_at"), true, now),
                added.isEmpty() ? QStringLiteral("—") : added.value("name").toString()});
            item->setSizeHint(0, QSize(0, PhoneRowHeight));
            item->setToolTip(2, full(device.value("paired_at"))); item->setToolTip(3, full(device.value("last_seen_at")));
            if (added.isEmpty()) item->setToolTip(4, list.value("encrypted").toBool()
                ? tr("Verified device membership. Private device names are not stored by the relay.")
                : tr("Paired with an administrator invitation, or before Zerus recorded it"));
            item->setData(0, KindRole, "phone"); item->setData(0, IdRole, device.value("id").toString());
            item->setData(0, HostRole, host); item->setData(0, WorkspaceRole, workspace); item->setData(0, TargetRole, index);
            item->setData(0, WorkspaceIdRole, list.value("workspace_id").toString()); item->setData(0, ServerRole, list.value("server_url").toString());
            if (item->data(0, IdRole).toString() == selected) m_tree->setCurrentItem(item);
            ++phones;
        }
    }
    m_tree->expandAll();
    static_cast<PhoneTable *>(m_tree)->fit();
    m_tree->setVisible(!m_lists.isEmpty());
    QStringList lines;
    if (!m_notice.isEmpty()) lines << m_notice;
    if (m_targets.isEmpty()) {
#ifdef Q_OS_WIN
        lines << tr("Open Settings → Mobile connection to create a workspace or join one by code, then start the connector and refresh Phones.");
#else
        lines << tr("No computer here has a mobile connection. Set one up in Settings → Mobile connection on a computer with local sessions enabled, then refresh Phones.");
#endif
    }
    m_count->setText(m_targets.isEmpty() ? QString() : phones == 1 ? tr("1 phone") : tr("%1 phones").arg(phones));
    if (!m_revoking) showStatus(lines.join('\n'));
    // #647: each computer that did not answer has a short line of its own.
    while (auto *item = m_problemList->takeAt(0)) { if (auto *w = item->widget()) w->deleteLater(); delete item; }
    for (const auto &problem : std::as_const(m_problems)) {
        auto *line = new QLabel(shortProblem(problem)); line->setObjectName("phonesProblem"); line->setTextFormat(Qt::PlainText);
        line->setToolTip(problem); line->setStyleSheet(QStringLiteral("color:%1;font-size:11.5px;").arg(m_warnColor.name()));
        line->setTextInteractionFlags(Qt::TextSelectableByMouse);
        m_problemList->addWidget(line);
    }
    m_pair->setEnabled(!m_pairRequested && !m_targets.isEmpty()); m_refresh->setEnabled(true);
    const auto *item = m_tree->currentItem();
    m_revoke->setEnabled(!m_revoking && item && item->data(0, KindRole).toString() == QLatin1String("phone"));
}

void Panel::showStatus(const QString &text) {
    m_status->setText(text); m_status->setVisible(!text.isEmpty());
}

void Panel::setTheme(bool dark) {
    const auto token = [dark](const char *name) { return PageCards::token(dark, name); };
    const auto hex = [&token](const char *name) { return token(name).name(QColor::HexArgb); };
    m_groupFill = token("hover"); m_warnColor = token("warn");
    QColor selected = token("accent"); selected.setAlphaF(dark ? .16 : .12);
    findChild<QLabel *>(QStringLiteral("phonesIntro"))->setStyleSheet(QStringLiteral("color:%1;font-size:11.5px;").arg(hex("muted")));
    m_count->setStyleSheet(QStringLiteral("color:%1;font-size:12px;").arg(hex("muted")));
    m_status->setStyleSheet(QStringLiteral("color:%1;font-size:11.5px;").arg(hex("text2")));
    m_tree->setStyleSheet(QStringLiteral(
        "QTreeWidget#linkedPhones{background:transparent;border:0;outline:0;color:%1;font-size:12px;}"
        "QTreeWidget#linkedPhones::item{border:0;border-bottom:1px solid %2;padding-left:6px;}"
        "QTreeWidget#linkedPhones::item:hover{background:%3;}"
        "QTreeWidget#linkedPhones::item:selected{background:%4;color:%1;}"
        "QTreeWidget#linkedPhones::branch{background:transparent;border-bottom:1px solid %2;}"
        "QTreeWidget#linkedPhones QHeaderView{background:transparent;border:0;}"
        "QTreeWidget#linkedPhones QHeaderView::section{background:transparent;color:%5;border:0;border-bottom:1px solid %2;"
        "padding:5px 6px 5px 8px;font-size:11.5px;font-weight:400;}")
        .arg(hex("fg"), hex("border"), hex("hover"), selected.name(QColor::HexArgb), hex("muted")));
    for (int i = 0; i < m_tree->topLevelItemCount(); ++i) m_tree->topLevelItem(i)->setBackground(0, m_groupFill);
    for (auto *line : findChildren<QLabel *>(QStringLiteral("phonesProblem")))
        line->setStyleSheet(QStringLiteral("color:%1;font-size:11.5px;").arg(m_warnColor.name()));
}

void Panel::revoke() {
    const auto *item = m_tree->currentItem();
    if (m_revoking || m_pairRequested || m_machines || !m_pending.isEmpty()
        || !item || item->data(0, KindRole).toString() != QLatin1String("phone")) return;
    const auto phone = item->text(0), workspace = item->data(0, WorkspaceRole).toString();
    const auto id = item->data(0, IdRole).toString(), host = item->data(0, HostRole).toString();
    const auto workspaceId = item->data(0, WorkspaceIdRole).toString(), server = item->data(0, ServerRole).toString();
    const bool confirmed = m_confirm ? m_confirm(phone, workspace) : [&] {
        QMessageBox box(QMessageBox::Warning, tr("Revoke %1?").arg(phone),
            tr("%1 loses access to every computer of %2 at once, and its queued requests are cancelled. "
               "Pair it again to restore access.").arg(phone, workspace), QMessageBox::Cancel, this);
        auto *revoke = box.addButton(tr("Revoke"), QMessageBox::DestructiveRole);
        box.setDefaultButton(QMessageBox::Cancel); box.exec();
        return box.clickedButton() == revoke;
    }();
    if (!confirmed) return;
    m_notice.clear(); m_revokingName = phone;
    showStatus(tr("Revoking %1…").arg(phone)); m_revoke->setEnabled(false);
    m_revoking = m_client->requestMobile(host, pinned(QStringLiteral("revoke-phone"), id, workspaceId, server));
}

void Panel::pair() {
    if (m_pairRequested || m_targets.isEmpty()) return;
    const auto *item = m_tree->currentItem();
    m_pairTarget = m_targets.value(item ? item->data(0, TargetRole).toInt() : 0);
    m_pairRequested = true;
    refresh();
}

void Panel::openPairing() {
    if (!m_pairRequested) return;
    if (m_machines || !m_pending.isEmpty()) return;
    m_pairRequested = false;
    if (m_targets.isEmpty()) { render(); return; }
    int selected = 0;
    for (int i = 0; i < m_targets.size(); ++i)
        if (m_targets[i].workspaceId == m_pairTarget.workspaceId && origin(m_targets[i].serverUrl) == origin(m_pairTarget.serverUrl)) { selected = i; break; }
    Dialog dialog(m_executable, m_targets, selected, this);
    dialog.exec();
    m_notice.clear(); refresh();
}

} // namespace PhonePairing
