#include "ModelServersPage.h"
#include "DirectoryDialog.h"
#include "HgsClient.h"
#include "TeamWidgets.h"
#include <QApplication>
#include <QButtonGroup>
#include <QClipboard>
#include <QComboBox>
#include <QDateTime>
#include <QDialog>
#include <QDir>
#include <QGridLayout>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QProgressBar>
#include <QRadioButton>
#include <QRegularExpression>
#include <QScrollArea>
#include <QScrollBar>
#include <QTimer>
#include <QJsonArray>
#include <QHash>
#include <QStandardItemModel>

using namespace TeamUi;
using ModelServers::Server;

namespace {
QPushButton *small(const QString &text, const char *name, const QString &icon, const QColor &color)
{
    auto *b = button(text, name);
    // Without a color the page paints the icon in its theme (setDark).
    if (!icon.isEmpty()) { b->setProperty("pageIcon", icon); if (color.isValid()) b->setIcon(workspaceIcon(icon, color)); b->setIconSize(QSize(13, 13)); }
    return b;
}

QColor stateColor(const Colors &c, const QString &state)
{
    if (state == "running") return c.accent;
    if (state == "loading" || state == "starting" || state == "stopping") return c.warn;
    if (state == "listed") return c.folder;
    if (state == "failed" || state == "error") return c.danger;
    return c.muted;
}

// What a failed read is called on the server's pill.
QString errorLabel(const QString &error)
{
    if (error == "unreachable") return QObject::tr("Unreachable");
    if (error == "timed_out") return QObject::tr("Timed out");
    if (error == "unauthorized") return QObject::tr("Key refused");
    if (error == "refused") return QObject::tr("Refused");
    return QObject::tr("Not read");
}

QString ago(qint64 at)
{
    const qint64 seconds = qMax<qint64>(0, (QDateTime::currentMSecsSinceEpoch() - at) / 1000);
    if (seconds < 60) return QObject::tr("read %1 s ago").arg(seconds);
    if (seconds < 3600) return QObject::tr("read %1 min ago").arg(seconds / 60);
    return QObject::tr("read %1 h ago").arg(seconds / 3600);
}

// "since 09:12" today, "since 28 Oct 09:12" before.
QString sinceText(qint64 seconds)
{
    const QDateTime at = QDateTime::fromSecsSinceEpoch(seconds).toLocalTime();
    return at.date() == QDate::currentDate() ? QObject::tr("since %1").arg(at.toString(QStringLiteral("HH:mm")))
                                             : QObject::tr("since %1").arg(at.toString(QStringLiteral("d MMM HH:mm")));
}

// Which of your sessions use a model, by name: "2 of your sessions use it: a, b".
QString usedBy(const QStringList &names)
{
    if (names.isEmpty()) return QObject::tr("none of your sessions");
    QStringList shown = names.mid(0, 3);
    if (names.size() > 3) shown << QObject::tr("%1 more").arg(names.size() - 3);
    return names.size() == 1 ? QObject::tr("1 of your sessions uses it: %1").arg(shown.join(QStringLiteral(", ")))
                             : QObject::tr("%1 of your sessions use it: %2").arg(names.size()).arg(shown.join(QStringLiteral(", ")));
}

// The last line a run printed, for a failure's reason.
QString lastLine(const QString &tail)
{
    const auto lines = tail.split(QLatin1Char('\n'), Qt::SkipEmptyParts);
    for (auto it = lines.crbegin(); it != lines.crend(); ++it)
        if (const auto line = it->trimmed(); !line.isEmpty()) return line.left(200);
    return {};
}

// The session's project from its folder, as hgs names it.
QString projectOf(const QString &folder)
{
    QString base = QDir::fromNativeSeparators(folder.trimmed());
    while (base.size() > 1 && base.endsWith(QLatin1Char('/'))) base.chop(1);
    base = base.section(QLatin1Char('/'), -1);
    base.replace(QLatin1Char(':'), QLatin1Char('_')).replace(QLatin1Char('.'), QLatin1Char('_'));
    return base.isEmpty() ? QStringLiteral("root") : base;
}

QString tagFor(const QString &model)
{
    QString tag = model.section(QLatin1Char('/'), -1).toLower();
    tag.replace(QRegularExpression(QStringLiteral("[^a-z0-9-]+")), QStringLiteral("-"));
    return tag.left(40).remove(QRegularExpression(QStringLiteral("^-+|-+$")));
}
}

ModelServersPage::ModelServersPage(ModelServers::Controller *servers, QWidget *parent) : QWidget(parent), m_servers(servers)
{
    setObjectName("modelServersPage");
    auto *outer = new QVBoxLayout(this); outer->setContentsMargins(0, 0, 0, 0); outer->setSpacing(0);
    auto *header = new QFrame; header->setObjectName("modelServersHeader");
    auto *h = new QHBoxLayout(header); h->setContentsMargins(24, 14, 24, 14); h->setSpacing(10);
    m_tile = new QLabel; m_tile->setObjectName("modelServersTile"); m_tile->setFixedSize(30, 30); m_tile->setAlignment(Qt::AlignCenter);
    auto *title = new QLabel(tr("Model servers")); title->setObjectName("modelServersTitle");
    m_summary = new QLabel; m_summary->setObjectName("modelServersSummary");
    m_refreshAll = small(tr("Refresh all"), "modelServersRefreshAll", QStringLiteral("refresh"), QColor());
    m_refreshAll->setProperty("ghost", true); m_refreshAll->setToolTip(tr("Read every server once"));
    QObject::connect(m_refreshAll, &QPushButton::clicked, this, [this] { m_servers->refreshAll(); });
    m_editServers = small(tr("Edit servers"), "modelServersEdit", QStringLiteral("settings"), QColor());
    QObject::connect(m_editServers, &QPushButton::clicked, this, [this] { if (editServers) editServers({}); });
    h->addWidget(m_tile); h->addWidget(title); h->addWidget(m_summary, 1); h->addWidget(m_refreshAll); h->addWidget(m_editServers);
    outer->addWidget(header);
    m_notice = new QLabel; m_notice->setObjectName("modelServersNotice"); m_notice->setWordWrap(true); m_notice->hide();
    m_notice->setContentsMargins(24, 6, 24, 6);
    outer->addWidget(m_notice);
    m_scroll = new QScrollArea; m_scroll->setObjectName("modelServersScroll"); m_scroll->setWidgetResizable(true);
    m_scroll->setFrameShape(QFrame::NoFrame); m_scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    outer->addWidget(m_scroll, 1);
    QObject::connect(m_servers, &ModelServers::Controller::changed, this, [this] { render(); });
    setDark(true);
}

void ModelServersPage::showEvent(QShowEvent *event)
{
    QWidget::showEvent(event);
    m_servers->setActive(true);
    render();
}

void ModelServersPage::hideEvent(QHideEvent *event)
{
    m_servers->setActive(false);
    QWidget::hideEvent(event);
}

void ModelServersPage::setDark(bool dark)
{
    m_dark = dark;
    const Colors c = colorsFor(dark);
    const auto a = [](const QColor &color, double value) { return alpha(color, value).name(QColor::HexArgb); };
    // #737: its dialogs take this sheet (a dialog takes the system's palette).
    setStyleSheet(teamSheet(c) + dialogSheet(c) + QStringLiteral(
        "QWidget#modelServersPage{background:%1;}"
        "QFrame#modelServersHeader{border-bottom:1px solid %2;}"
        "QLabel#modelServersTitle{color:%3;font-size:20px;font-weight:600;}"
        "QLabel#modelServersSummary{color:%4;font-size:13px;}"
        "QLabel#modelServersTile{background:%5;border:1px solid %2;border-radius:8px;}"
        "QScrollArea#modelServersScroll,QWidget#modelServersList{background:transparent;}"
        "QFrame#modelServerCard{background:%6;border:1px solid %2;border-radius:12px;}"
        "QFrame#modelServerCard[unreachable=\"true\"]{border-color:%7;}"
        "QFrame#modelServerCard QLabel{background:transparent;}"
        "QFrame#modelServerModel{border-top:1px solid %2;}"
        "QFrame#modelServerFoot{border-top:1px solid %2;}"
        "QFrame#modelServerApi{background:%5;border:1px solid %2;border-radius:7px;}"
        "QFrame#modelServerHost{background:%8;border-radius:7px;}"
        "QFrame#modelServerProblem{background:%9;border-radius:8px;}"
        "QPlainTextEdit#modelServerRaw{font-family:\"Cascadia Mono\",Consolas,monospace;font-size:12px;}"
        "QPushButton[down=\"true\"]{color:%10;border:1px solid %11;}"
        "QPushButton[down=\"true\"]:hover{background:%12;}"
        "QLabel#modelServersNotice{color:%4;}"
        "QLabel#modelServersNotice[error=\"true\"]{color:%10;}"
        "QProgressBar#modelServerMemory{background:%2;border:0;border-radius:3px;}"
        "QProgressBar#modelServerMemory::chunk{background:%13;border-radius:3px;}"
        "QProgressBar#modelServerMemory[low=\"true\"]::chunk{background:%14;}")
        .arg(c.bg.name(), c.cardBorder.name(), c.fg.name(), c.muted.name(), c.tile.name(), c.cardFill.name(), a(c.danger, .6),
             a(c.fg, .05), a(c.danger, .10))
        .arg(c.danger.name(), a(c.danger, .45), a(c.danger, .10), c.accentMuted.name(), c.warn.name()));
    m_tile->setPixmap(workspaceIcon(QStringLiteral("server"), c.text2).pixmap(16, 16));
    for (auto *b : findChild<QFrame *>("modelServersHeader")->findChildren<QPushButton *>())
        b->setIcon(workspaceIcon(b->property("pageIcon").toString(), c.fg));
    render();
}

QString ModelServersPage::hostText(const Server &server) const
{
    if (server.ssh.isEmpty()) return tr("This computer");
    const QStringList known = machines ? machines() : QStringList();
    return known.contains(server.ssh) ? tr("%1 in Machines").arg(server.ssh) : tr("ssh alias %1").arg(server.ssh);
}

void ModelServersPage::notice(const QString &text, bool error)
{
    m_noticeText = text; m_noticeError = error;
    m_notice->setText(text); m_notice->setProperty("error", error);
    m_notice->style()->unpolish(m_notice); m_notice->style()->polish(m_notice);
    m_notice->setVisible(!text.isEmpty());
}

void ModelServersPage::render()
{
    const Colors c = colorsFor(m_dark);
    const int scrolled = m_scroll->verticalScrollBar()->value();
    // The summary: servers, models, running (no dots between them).
    int models = 0, running = 0;
    for (const auto &server : m_servers->servers()) {
        const auto status = m_servers->status(server.name);
        models += int(status.models.size());
        for (const auto &model : status.models) running += model.state == "running";
    }
    const int count = int(m_servers->servers().size());
    // #703 (#659 QA): with no server the page offers only Add server.
    m_refreshAll->setVisible(count > 0); m_editServers->setVisible(count > 0);
    m_summary->setText(count == 0 ? QString()
                       : tr("%1, %2, %3 running").arg(count == 1 ? tr("1 server") : tr("%1 servers").arg(count),
                                                       models == 1 ? tr("1 model") : tr("%1 models").arg(models)).arg(running));
    auto *list = new QWidget; list->setObjectName("modelServersList");
    auto *v = new QVBoxLayout(list); v->setContentsMargins(24, 18, 24, 24); v->setSpacing(14);
    if (count == 0) {
        auto *empty = new QWidget; empty->setObjectName("modelServersEmpty");
        auto *ev = new QVBoxLayout(empty); ev->setContentsMargins(0, 60, 0, 0); ev->setSpacing(8);
        auto *icon = new QLabel; icon->setPixmap(workspaceIcon(QStringLiteral("server"), c.text2).pixmap(28, 28)); icon->setAlignment(Qt::AlignCenter);
        auto *text = label(m_servers->listed() ? tr("No model servers on this computer") : tr("Reading the model servers"), c.fg, 14, true);
        text->setAlignment(Qt::AlignCenter);
        auto *hint = label(tr("Add a computer on your network, or this one, that serves models through an OpenAI- or Anthropic-compatible API."), c.muted, 12);
        hint->setAlignment(Qt::AlignCenter); hint->setWordWrap(true);
        auto *add = small(tr("Add server"), "modelServersAdd", QStringLiteral("add"), c.fg);
        QObject::connect(add, &QPushButton::clicked, this, [this] { if (editServers) editServers({}); });
        ev->addWidget(icon); ev->addWidget(text); ev->addWidget(hint); ev->addWidget(add, 0, Qt::AlignHCenter);
        v->addWidget(empty);
    }
    for (const auto &server : m_servers->servers()) v->addWidget(serverCard(server, m_servers->status(server.name)));
    v->addStretch(1);
    if (m_list) { m_list->hide(); m_list->deleteLater(); }
    m_scroll->takeWidget();
    m_scroll->setWidget(list); m_list = list;
    m_scroll->verticalScrollBar()->setValue(scrolled);
}

QWidget *ModelServersPage::serverCard(const Server &server, const ModelServers::Status &status)
{
    const Colors c = colorsFor(m_dark);
    const bool failed = status.known && !status.error.isEmpty();
    auto *card = new QFrame; card->setObjectName("modelServerCard");
    card->setProperty("server", server.name); card->setProperty("unreachable", failed);
    card->setProperty("readOnly", server.readOnly());
    auto *v = new QVBoxLayout(card); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(0);
    // The head: name, its state, SSH host, API address, memory and actions.
    auto *head = new QWidget; auto *h = new QHBoxLayout(head); h->setContentsMargins(16, 12, 16, 12); h->setSpacing(10);
    auto *tile = new QLabel; tile->setFixedSize(26, 26); tile->setAlignment(Qt::AlignCenter);
    tile->setStyleSheet(QStringLiteral("background:%1;border:1px solid %2;border-radius:7px;").arg(c.tile.name(), c.cardBorder.name()));
    tile->setPixmap(workspaceIcon(QStringLiteral("server"), failed ? c.danger : c.text2).pixmap(14, 14));
    h->addWidget(tile);
    h->addWidget(label(server.name, c.fg, 15, true, "modelServerName"));
    QString stateText; QColor stateTone;
    if (failed) { stateText = errorLabel(status.error); stateTone = c.danger; }
    else if (!status.known) { stateText = status.reading ? tr("Reading") : tr("Not read yet"); stateTone = c.muted; }
    else if (server.readOnly()) { stateText = tr("Read only"); stateTone = c.folder; }
    else { stateText = tr("Online"); stateTone = c.accent; }
    auto *pill = new Pill(stateText, stateTone); pill->setObjectName("modelServerState"); pill->setProperty("state", stateText);
    h->addWidget(pill);
    auto *host = new QFrame; host->setObjectName("modelServerHost");
    auto *hh = new QHBoxLayout(host); hh->setContentsMargins(8, 3, 8, 3); hh->setSpacing(5);
    auto *hostIcon = new QLabel; hostIcon->setPixmap(workspaceIcon(server.ssh.isEmpty() ? QStringLiteral("monitor") : QStringLiteral("terminal"), c.muted).pixmap(12, 12));
    hh->addWidget(hostIcon); hh->addWidget(label(hostText(server), c.text2, 12, false, "modelServerHostText"));
    h->addWidget(host);
    // The API address and its kind; Copy puts the address on the clipboard.
    auto *api = new QFrame; api->setObjectName("modelServerApi");
    auto *ah = new QHBoxLayout(api); ah->setContentsMargins(9, 3, 4, 3); ah->setSpacing(7);
    auto *kind = label(server.kind.toUpper(), c.muted, 10, true, "modelServerKind"); kind->setFont(NewSessionDense::mono(10));
    // #703 (#658 N2): the address an agent of this kind is given, from the root.
    const QString address = ModelServers::agentUrl(server.baseUrl, server.kind);
    auto *url = label(address, c.fg, 12, false, "modelServerUrl"); url->setFont(NewSessionDense::mono(12));
    url->setTextInteractionFlags(Qt::TextSelectableByMouse);
    const bool copied = m_copied == server.name;
    auto *copy = small(copied ? tr("Copied") : QString(), "modelServerCopy", copied ? QStringLiteral("check-circle") : QStringLiteral("copy"), copied ? c.accent : c.muted);
    copy->setProperty("ghost", true); copy->setToolTip(tr("Copy the API address")); copy->setAccessibleName(tr("Copy the API address of %1").arg(server.name));
    QObject::connect(copy, &QPushButton::clicked, this, [this, name = server.name, address] {
        QGuiApplication::clipboard()->setText(address);
        m_copied = name; render();
        QTimer::singleShot(2000, this, [this, name] { if (m_copied == name) { m_copied.clear(); render(); } });
    });
    ah->addWidget(kind); ah->addWidget(url); ah->addWidget(copy);
    h->addWidget(api);
    if (!failed && status.known) {
        if (status.hasMemory()) {
            auto *bar = new QProgressBar; bar->setObjectName("modelServerMemory"); bar->setTextVisible(false); bar->setFixedSize(130, 6);
            bar->setRange(0, 1000); bar->setValue(int(1000 * (1 - status.free / status.total)));
            bar->setProperty("low", status.free / status.total < .15);
            h->addWidget(bar);
            auto *memory = label(ModelServers::memoryText(status.free, status.total), status.free / status.total < .15 ? c.warn : c.text2, 12, false, "modelServerMemoryText");
            memory->setFont(NewSessionDense::mono(12));
            h->addWidget(memory);
        } else {
            h->addWidget(label(tr("memory not reported"), c.muted, 12, false, "modelServerMemoryText"));
        }
    }
    h->addStretch(1);
    QStringList active;
    for (const auto &model : status.models) if (model.state == "running" || model.state == "loading") active << model.name;
    if (failed) {
        auto *again = small(tr("Try again"), "modelServerTryAgain", QStringLiteral("refresh"), c.fg);
        QObject::connect(again, &QPushButton::clicked, this, [this, name = server.name] { m_servers->refreshStatus(name); });
        auto *edit = small(tr("Edit…"), "modelServerEdit", QStringLiteral("settings"), c.fg); edit->setProperty("ghost", true);
        QObject::connect(edit, &QPushButton::clicked, this, [this, name = server.name] { if (editServers) editServers(name); });
        h->addWidget(again); h->addWidget(edit);
    } else {
        if (!server.logs.isEmpty()) {
            auto *logs = small(tr("Logs"), "modelServerLogs", QStringLiteral("logs"), c.fg); logs->setProperty("ghost", true);
            QObject::connect(logs, &QPushButton::clicked, this, [this, name = server.name] { showLogs(name, {}); });
            h->addWidget(logs);
        }
        if (!server.down.isEmpty() && !active.isEmpty()) {
            auto *all = small(tr("Down all…"), "modelServerDownAll", QStringLiteral("power"), c.danger); all->setProperty("down", true);
            QObject::connect(all, &QPushButton::clicked, this, [this, name = server.name, active] { QTimer::singleShot(0, this, [this, name, active] { askDown(name, active); }); });
            h->addWidget(all);
        }
        if (server.readOnly()) {
            auto *refresh = small(tr("Refresh"), "modelServerRefresh", QStringLiteral("refresh"), c.fg); refresh->setProperty("ghost", true);
            QObject::connect(refresh, &QPushButton::clicked, this, [this, name = server.name] { m_servers->refreshStatus(name); });
            h->addWidget(refresh);
        }
    }
    v->addWidget(head);
    if (failed) {
        // Why it was not read; nothing tries again until you ask.
        auto *problem = new QFrame; problem->setObjectName("modelServerProblem");
        auto *ph = new QHBoxLayout(problem); ph->setContentsMargins(12, 9, 12, 9); ph->setSpacing(8);
        auto *icon = new QLabel; icon->setPixmap(workspaceIcon(QStringLiteral("warning"), c.danger).pixmap(14, 14)); ph->addWidget(icon, 0, Qt::AlignTop);
        QString why = status.error == "unauthorized" ? tr("The server refused the API key: %1").arg(status.detail)
                    : status.detail.isEmpty() ? errorLabel(status.error) : status.detail;
        // #703 (#658 N2): a read-only server is read at root/v1/models; say which URL.
        if (server.readOnly() && status.error != "failed") why = tr("GET %1: %2").arg(ModelServers::modelsUrl(server.baseUrl), why);
        auto *text = label(tr("%1. Zerus tries again only when you ask.").arg(why.endsWith(QLatin1Char('.')) ? why.chopped(1) : why), c.fg, 12, false, "modelServerProblemText");
        text->setWordWrap(true); text->setTextInteractionFlags(Qt::TextSelectableByMouse);
        ph->addWidget(text, 1);
        auto *wrap = new QWidget; auto *wv = new QVBoxLayout(wrap); wv->setContentsMargins(16, 0, 16, 14); wv->addWidget(problem);
        v->addWidget(wrap);
        return card;
    }
    // One row per model: its state, and what you can do with it.
    for (const auto &model : status.models) {
        auto *line = new QFrame; line->setObjectName("modelServerModel"); line->setProperty("model", model.name); line->setProperty("state", model.state);
        auto *lh = new QHBoxLayout(line); lh->setContentsMargins(16, 7, 16, 7); lh->setSpacing(10);
        auto *icon = new QLabel; icon->setPixmap(workspaceIcon(QStringLiteral("asterisk"), c.muted).pixmap(12, 12)); lh->addWidget(icon);
        auto *name = label(model.name, c.fg, 13, true, "modelServerModelName"); name->setFont(NewSessionDense::mono(13)); name->setMinimumWidth(220);
        lh->addWidget(name);
        const QString state = model.state.isEmpty() && status.source == "api" ? QStringLiteral("listed") : model.state;
        const QColor tone = stateColor(c, state);
        auto *dot = new QLabel(QStringLiteral("●")); QPalette dp = dot->palette(); dp.setColor(QPalette::WindowText, tone); dot->setPalette(dp);
        lh->addWidget(dot);
        lh->addWidget(label(ModelServers::stateLabel(state, status.source), tone, 13, true, "modelServerModelState"));
        if (status.source == "api") lh->addWidget(label(tr("from GET /v1/models"), c.muted, 12));
        else if (model.since > 0) lh->addWidget(label(sinceText(model.since), c.muted, 12, false, "modelServerModelSince"));
        lh->addStretch(1);
        const bool usable = state == "running" || state == "listed";
        if (usable) {
            auto *session = small(tr("New session…"), "modelServerNewSession", QStringLiteral("add"), c.fg);
            QObject::connect(session, &QPushButton::clicked, this, [this, server = server.name, name = model.name] { QTimer::singleShot(0, this, [this, server, name] { newSession(server, name); }); });
            lh->addWidget(session);
        }
        if (!server.logs.isEmpty()) {
            auto *logs = small(tr("Logs"), "modelServerModelLogs", QStringLiteral("logs"), c.fg); logs->setProperty("ghost", true);
            QObject::connect(logs, &QPushButton::clicked, this, [this, server = server.name, name = model.name] { showLogs(server, name); });
            lh->addWidget(logs);
        }
        if (!server.down.isEmpty() && (state == "running" || state == "loading")) {
            auto *down = small(tr("Down…"), "modelServerDown", QStringLiteral("power"), c.danger); down->setProperty("down", true);
            QObject::connect(down, &QPushButton::clicked, this, [this, server = server.name, name = model.name] { QTimer::singleShot(0, this, [this, server, name] { askDown(server, {name}); }); });
            lh->addWidget(down);
        }
        if (!server.up.isEmpty() && (state == "stopped" || state == "unloaded" || state == "idle")) {
            const bool running = m_servers->upRunning(server.name, model.name);
            auto *up = small(running ? tr("Up running") : tr("Up"), "modelServerUp", QStringLiteral("power"), c.fg);
            up->setEnabled(!running);
            QObject::connect(up, &QPushButton::clicked, this, [this, server = server.name, name = model.name] { this->up(server, name); });
            lh->addWidget(up);
        }
        v->addWidget(line);
    }
    if (!status.raw.isEmpty()) {
        // Neither the JSON contract nor NAME: STATE lines: kept as it was printed.
        auto *raw = new QPlainTextEdit(status.raw.join(QLatin1Char('\n'))); raw->setObjectName("modelServerRaw"); raw->setReadOnly(true);
        raw->setFixedHeight(qMin(160, 20 + 18 * int(status.raw.size())));
        auto *wrap = new QWidget; auto *wv = new QVBoxLayout(wrap); wv->setContentsMargins(16, 6, 16, 6); wv->addWidget(raw);
        v->addWidget(wrap);
    }
    // The footer: how this server is read, and when it last was.
    auto *foot = new QFrame; foot->setObjectName("modelServerFoot");
    auto *fh = new QHBoxLayout(foot); fh->setContentsMargins(16, 8, 16, 8); fh->setSpacing(8);
    auto *footIcon = new QLabel; footIcon->setPixmap(workspaceIcon(server.readOnly() ? QStringLiteral("eye") : QStringLiteral("terminal"), c.muted).pixmap(12, 12));
    fh->addWidget(footIcon);
    const QString how = server.readOnly()
        ? tr("No control commands: the models come from the API's model list. No Up, Down or Logs here.")
        : server.ssh.isEmpty() ? tr("Status, Up, Down and Logs run your commands on this computer, once each; nothing retries on its own.")
                               : tr("Status, Up, Down and Logs run your commands over hgs's SSH to %1, once each; nothing retries on its own.").arg(server.ssh);
    auto *howLabel = label(how, c.muted, 12, false, "modelServerFootText"); howLabel->setWordWrap(true);
    fh->addWidget(howLabel, 1);
    if (status.known) fh->addWidget(label(status.reading ? tr("reading") : ago(status.readAt), c.muted, 12, false, "modelServerReadAt"));
    if (!server.readOnly()) {
        auto *refresh = small(tr("Refresh"), "modelServerRefresh", QStringLiteral("refresh"), c.fg); refresh->setProperty("ghost", true);
        refresh->setEnabled(!status.reading);
        QObject::connect(refresh, &QPushButton::clicked, this, [this, name = server.name] { m_servers->refreshStatus(name); });
        fh->addWidget(refresh);
    }
    v->addWidget(foot);
    return card;
}

void ModelServersPage::up(const QString &server, const QString &model)
{
    notice(tr("Up %1 on %2…").arg(model, server));
    QPointer<ModelServersPage> guard(this);
    m_servers->up(server, model, [this, guard, server, model](const QJsonObject &result, const QString &error) {
        if (!guard || !error.isEmpty() || result.value("status").toString() != "started") return;
        notice(tr("Up %1 on %2 is running once; its result shows here when it ends.").arg(model, server));
        render();
    }, [this, guard, server, model](const QJsonObject &result, const QString &error) {
        if (!guard) return;
        // #703 (#658 N5): the run's own outcome, not only that it started.
        const QString status = result.value("status").toString();
        const int exit = result.value("exit").toInt(-1);
        QString why;
        if (!error.isEmpty()) why = error;
        else if (status == "error") why = result.value("detail").toString(result.value("error").toString());
        else if (result.value("timed_out").toBool())
            why = tr("stopped at its %1 min limit").arg(result.value("timeout_secs").toInt() / 60);
        else if (status == "refused") why = result.value("error").toString(tr("it did not run"));
        else if (status == "ran" && exit != 0) {
            const QString line = lastLine(result.value("tail").toString());
            why = line.isEmpty() ? tr("exit %1").arg(exit) : tr("exit %1: %2").arg(exit).arg(line);
        }
        notice(why.isEmpty() ? tr("Up %1 on %2 finished: exit 0. Nothing starts it again on its own.").arg(model, server)
                             : tr("Up %1 on %2 failed: %3").arg(model, server, why), !why.isEmpty());
        render();
    });
    render();
}

void ModelServersPage::askDown(const QString &serverName, const QStringList &models)
{
    const auto *server = m_servers->server(serverName);
    if (!server || models.isEmpty()) return;
    const Colors c = colorsFor(m_dark);
    QDialog dialog(this); dialog.setObjectName("modelServerDownDialog"); dialog.setStyleSheet(styleSheet());
    dialog.setWindowTitle(models.size() == 1 ? tr("Down %1").arg(models.first()) : tr("Down all on %1").arg(serverName));
    dialog.setMinimumWidth(520);
    auto *v = new QVBoxLayout(&dialog); v->setContentsMargins(20, 18, 20, 16); v->setSpacing(12);
    v->addWidget(label(models.size() == 1 ? tr("Down %1 on %2?").arg(models.first(), serverName) : tr("Down all on %1?").arg(serverName), c.fg, 16, true, "modelServerDownTitle"));
    // The server may be shared: say so before anything runs.
    auto *shared = label(tr("%1 may be shared. Other people's sessions on %2 stop getting answers too, and Zerus cannot see them.")
                             .arg(serverName, models.size() == 1 ? tr("this model") : tr("these models")),
                         c.warn, 13, false, "modelServerDownShared");
    shared->setWordWrap(true);
    shared->setStyleSheet(QStringLiteral("background:%1;border:1px solid %2;border-radius:8px;padding:10px 12px;")
                              .arg(alpha(c.warn, .10).name(QColor::HexArgb), alpha(c.warn, .45).name(QColor::HexArgb)));
    v->addWidget(shared);
    const auto status = m_servers->status(serverName);
    // #703 (#659 QA): which of your sessions use each model, read once.
    QHash<QString, QLabel *> uses;
    QHash<QString, QString> states;
    for (const auto &name : models) {
        QString state;
        for (const auto &model : status.models) if (model.name == name) state = model.state;
        states.insert(name, state);
        auto *r = row(10);
        auto *n = label(name, c.fg, 13, true, "modelServerDownModel"); n->setFont(NewSessionDense::mono(13)); n->setMinimumWidth(200);
        r->addWidget(n);
        r->addWidget(label(ModelServers::stateLabel(state, status.source), stateColor(c, state), 13, true));
        auto *use = label(tr("reading your sessions…"), c.muted, 12, false, "modelServerDownUse"); use->setProperty("model", name);
        use->setWordWrap(true);
        r->addWidget(use, 1);
        uses.insert(name, use);
        v->addLayout(r);
    }
    m_servers->sessions(serverName, [uses, states, guard = QPointer<QDialog>(&dialog)](const QJsonObject &result, const QString &error) {
        if (!guard) return;
        const QString why = !error.isEmpty() ? error : result.value("status").toString() == "error" ? result.value("detail").toString() : QString();
        QHash<QString, QStringList> names;
        for (const auto &value : result.value("sessions").toArray()) {
            const auto session = value.toObject();
            names[session.value("model").toString()] << session.value("name").toString();
        }
        for (auto it = uses.cbegin(); it != uses.cend(); ++it) {
            QString text = why.isEmpty() ? usedBy(names.value(it.key())) : QObject::tr("your sessions not read: %1").arg(why);
            // A model still loading stops loading; it serves no session yet.
            if (states.value(it.key()) == "loading" && names.value(it.key()).isEmpty() && why.isEmpty()) text = QObject::tr("stops loading");
            it.value()->setText(text); it.value()->setToolTip(names.value(it.key()).join(QLatin1Char('\n')));
        }
    });
    auto *how = label(server->ssh.isEmpty()
                          ? tr("Runs your down command on this computer once for each model. Nothing starts again on its own: Up them here when you need them.")
                          : tr("Runs your down command over SSH to %1 once for each model. Nothing starts again on its own: Up them here when you need them.").arg(server->ssh),
                      c.text2, 12);
    how->setWordWrap(true); v->addWidget(how);
    auto *buttons = row(8); buttons->addStretch(1);
    auto *cancel = button(tr("Cancel"), "modelServerDownCancel");
    auto *confirm = button(models.size() == 1 ? tr("Down") : tr("Down %1 models").arg(models.size()), "modelServerDownConfirm");
    confirm->setProperty("down", true); confirm->setIcon(workspaceIcon(QStringLiteral("power"), c.danger));
    cancel->setDefault(true); cancel->setAutoDefault(true);
    QObject::connect(cancel, &QPushButton::clicked, &dialog, &QDialog::reject);
    QObject::connect(confirm, &QPushButton::clicked, &dialog, &QDialog::accept);
    buttons->addWidget(cancel); buttons->addWidget(confirm);
    v->addLayout(buttons);
    if (dialog.exec() != QDialog::Accepted) return;
    // One down per model, one after the other; each runs once.
    auto queue = std::make_shared<QStringList>(models);
    auto failures = std::make_shared<QStringList>();
    auto next = std::make_shared<std::function<void()>>();
    QPointer<ModelServersPage> guard(this);
    *next = [this, guard, queue, failures, next, serverName, total = int(models.size())] {
        if (!guard) return;
        if (queue->isEmpty()) {
            notice(failures->isEmpty() ? (total == 1 ? tr("Down ran once on %1.").arg(serverName) : tr("Down ran once for %1 models on %2.").arg(total).arg(serverName))
                                       : tr("Down on %1 failed: %2").arg(serverName, failures->join(QStringLiteral("; "))), !failures->isEmpty());
            *next = nullptr;
            return;
        }
        const QString model = queue->takeFirst();
        notice(tr("Down %1 on %2…").arg(model, serverName));
        m_servers->down(serverName, model, [failures, next, model](const QJsonObject &result, const QString &error) {
            const QString why = !error.isEmpty() ? error : result.value("status").toString() == "error" ? result.value("detail").toString()
                              : result.value("exit").toInt() != 0 ? QObject::tr("exit %1").arg(result.value("exit").toInt()) : QString();
            if (!why.isEmpty()) *failures << QStringLiteral("%1: %2").arg(model, why);
            if (*next) (*next)();
        });
    };
    (*next)();
}

void ModelServersPage::showLogs(const QString &server, const QString &model)
{
    const Colors c = colorsFor(m_dark);
    auto *dialog = new QDialog(this); dialog->setObjectName("modelServerLogsDialog"); dialog->setAttribute(Qt::WA_DeleteOnClose);
    dialog->setStyleSheet(styleSheet());
    dialog->setWindowTitle(model.isEmpty() ? tr("Logs of %1").arg(server) : tr("Logs of %1 on %2").arg(model, server));
    dialog->resize(820, 520);
    auto *v = new QVBoxLayout(dialog); v->setContentsMargins(16, 14, 16, 12); v->setSpacing(8);
    v->addWidget(label(dialog->windowTitle(), c.fg, 15, true));
    auto *text = new QPlainTextEdit; text->setObjectName("modelServerLogsText"); text->setReadOnly(true);
    text->setFont(NewSessionDense::mono(12)); text->setPlaceholderText(tr("Reading the logs once…"));
    v->addWidget(text, 1);
    auto *state = label(QString(), c.muted, 12, false, "modelServerLogsState"); v->addWidget(state);
    auto *buttons = row(8); buttons->addStretch(1);
    auto *again = button(tr("Read again"), "modelServerLogsAgain"); auto *close = button(tr("Close"), "modelServerLogsClose");
    buttons->addWidget(again); buttons->addWidget(close); v->addLayout(buttons);
    QObject::connect(close, &QPushButton::clicked, dialog, &QDialog::close);
    const auto read = [this, server, model, text = QPointer<QPlainTextEdit>(text), state = QPointer<QLabel>(state), again = QPointer<QPushButton>(again)] {
        if (again) again->setEnabled(false);
        m_servers->logs(server, model, [text, state, again](const QJsonObject &result, const QString &error) {
            if (!text) return;
            if (again) again->setEnabled(true);
            const QString why = !error.isEmpty() ? error : result.value("status").toString() == "error" ? result.value("detail").toString() : QString();
            if (!why.isEmpty()) { state->setText(tr("Not read: %1").arg(why)); return; }
            QStringList lines;
            for (const auto &line : result.value("lines").toArray()) lines << line.toString();
            text->setPlainText(lines.join(QLatin1Char('\n')));
            text->verticalScrollBar()->setValue(text->verticalScrollBar()->maximum());
            state->setText(result.value("truncated").toBool() ? tr("The last %1 lines").arg(lines.size()) : tr("%1 lines").arg(lines.size()));
        });
    };
    QObject::connect(again, &QPushButton::clicked, dialog, read);
    read();
    dialog->show();
}

void ModelServersPage::newSession(const QString &serverName, const QString &model)
{
    const auto *server = m_servers->server(serverName);
    if (!server) return;
    const Colors c = colorsFor(m_dark);
    QDialog dialog(this); dialog.setObjectName("modelServerSessionDialog"); dialog.setStyleSheet(styleSheet());
    dialog.setWindowTitle(tr("New session on %1").arg(serverName)); dialog.setMinimumWidth(640);
    auto *v = new QVBoxLayout(&dialog); v->setContentsMargins(20, 18, 20, 16); v->setSpacing(12);
    v->addWidget(label(dialog.windowTitle(), c.fg, 16, true));
    auto *grid = new QGridLayout; grid->setHorizontalSpacing(14); grid->setVerticalSpacing(10); grid->setColumnStretch(1, 1);
    const auto key = [&](const QString &text, int row) { auto *k = label(text, c.text2, 13); k->setAlignment(Qt::AlignRight | Qt::AlignTop); grid->addWidget(k, row, 0); };
    // The model: the server's running or listed ones.
    key(tr("Model"), 0);
    auto *models = new QComboBox; models->setObjectName("modelServerSessionModel"); models->setFont(NewSessionDense::mono(12));
    const auto status = m_servers->status(serverName);
    // #703 (#659 QA): every model with its state; only a running or listed one is chosen.
    for (const auto &item : status.models) {
        const QString state = item.state.isEmpty() && status.source == "api" ? QStringLiteral("listed") : item.state;
        models->addItem(QStringLiteral("%1   %2").arg(item.name, ModelServers::stateLabel(state, status.source)), item.name);
        models->setItemData(models->count() - 1, state, Qt::UserRole + 1);
        if (state != "running" && state != "listed")
            if (auto *items = qobject_cast<QStandardItemModel *>(models->model())) items->item(models->count() - 1)->setEnabled(false);
    }
    if (models->findData(model) < 0) models->addItem(model, model);
    models->setCurrentIndex(models->findData(model));
    grid->addWidget(models, 0, 1);
    // The agent: each through its own provider settings, for this session only.
    key(tr("Agent"), 1);
    auto *agents = new QWidget; auto *ah = new QHBoxLayout(agents); ah->setContentsMargins(0, 0, 0, 0); ah->setSpacing(8);
    auto *group = new QButtonGroup(&dialog);
    struct Agent { QString id, name, how; bool ok; QString why; };
    const bool openai = server->kind == "openai";
    const QList<Agent> choices{
        {"codex", tr("Codex"), tr("OpenAI-compatible provider for this session"), openai,
         tr("needs an OpenAI-compatible API; %1 serves Anthropic").arg(serverName)},
        {"claude", tr("Claude Code"), tr("Anthropic base URL for this session"), !openai,
         tr("needs an Anthropic-compatible API; %1 serves OpenAI").arg(serverName)}};
    QRadioButton *first = nullptr;
    for (const auto &agent : choices) {
        auto *choice = new QFrame; choice->setObjectName("modelServerAgent"); choice->setProperty("agent", agent.id);
        choice->setEnabled(agent.ok);
        choice->setStyleSheet(QStringLiteral("QFrame#modelServerAgent{border:1px solid %1;border-radius:9px;}").arg(c.cardBorder.name()));
        auto *cv = new QVBoxLayout(choice); cv->setContentsMargins(10, 8, 10, 8); cv->setSpacing(3);
        auto *radio = new QRadioButton(agent.name); radio->setObjectName("modelServerAgentChoice"); radio->setProperty("agent", agent.id);
        radio->setIcon(agentLogoIcon(agent.id, c.fg)); radio->setEnabled(agent.ok);
        group->addButton(radio); cv->addWidget(radio);
        auto *how = label(agent.ok ? agent.how : agent.why, agent.ok ? c.text2 : c.muted, 12, false, "modelServerAgentHow"); how->setWordWrap(true);
        cv->addWidget(how);
        ah->addWidget(choice, 1);
        if (agent.ok && !first) first = radio;
    }
    if (first) first->setChecked(true);
    grid->addWidget(agents, 1, 1);
    // The folder on this computer, and the session's name.
    key(tr("Folder"), 2);
    auto *folderRow = row(6);
    auto *folder = new QComboBox; folder->setObjectName("modelServerSessionFolder"); folder->setEditable(true);
    for (const auto &path : folders ? folders() : QStringList()) folder->addItem(QDir::toNativeSeparators(path));
    auto *browse = button(tr("Browse…"), "modelServerSessionBrowse");
    QObject::connect(browse, &QPushButton::clicked, &dialog, [this, folder, &dialog] {
        const auto chosen = DirectoryDialog::chooseDirectory(m_servers->client()->executable(), {}, folder->currentText(), &dialog);
        if (!chosen.isEmpty()) folder->setEditText(QDir::toNativeSeparators(chosen));
    });
    folderRow->addWidget(folder, 1); folderRow->addWidget(browse);
    grid->addLayout(folderRow, 2, 1);
    // The session's name as hgs makes it (agent/project/tag); you choose the tag.
    key(tr("Name"), 3);
    auto *nameRow = row(0);
    auto *prefix = label(QString(), c.muted, 12, false, "modelServerSessionPrefix"); prefix->setFont(NewSessionDense::mono(12));
    auto *tag = new QLineEdit(tagFor(model)); tag->setObjectName("modelServerSessionTag"); tag->setMaxLength(60); tag->setFont(NewSessionDense::mono(12));
    nameRow->addWidget(prefix); nameRow->addWidget(tag, 1);
    grid->addLayout(nameRow, 3, 1);
    // What this session gets; nothing else changes.
    key(tr("For this session"), 4);
    auto *summary = new QLabel; summary->setObjectName("modelServerSessionSummary"); summary->setFont(NewSessionDense::mono(12));
    summary->setTextFormat(Qt::PlainText);
    summary->setStyleSheet(QStringLiteral("background:%1;border:1px solid %2;border-radius:8px;padding:8px 12px;color:%3;")
                               .arg(c.tile.name(), c.cardBorder.name(), c.fg.name()));
    grid->addWidget(summary, 4, 1);
    v->addLayout(grid);
    auto *note = label(QString(), c.muted, 12, false, "modelServerSessionNote"); note->setWordWrap(true); v->addWidget(note);
    auto *buttons = row(8); buttons->addStretch(1);
    auto *cancel = button(tr("Cancel"), "modelServerSessionCancel");
    auto *start = button(tr("Start session"), "modelServerStart", true);
    QObject::connect(cancel, &QPushButton::clicked, &dialog, &QDialog::reject);
    QObject::connect(start, &QPushButton::clicked, &dialog, &QDialog::accept);
    buttons->addWidget(cancel); buttons->addWidget(start); v->addLayout(buttons);
    const auto agentOf = [group] { auto *checked = group->checkedButton(); return checked ? checked->property("agent").toString() : QString(); };
    const auto update = [=] {
        const QString agent = agentOf(), chosen = models->currentData().toString();
        const QString name = agent == "codex" ? tr("Codex") : tr("Claude Code");
        prefix->setText(QStringLiteral("%1/%2/").arg(agent, projectOf(folder->currentText())));
        if (!tag->isModified()) tag->setText(tagFor(chosen));
        summary->setText(QStringLiteral("agent     %1, on this computer\nprovider  %2  %3\nmodel     %4\nkey       %5")
                             .arg(agent, serverName, ModelServers::agentUrl(server->baseUrl, server->kind), chosen,
                                  server->hasKey ? tr("from the private store") : tr("none")));
        note->setText(tr("Your %1 configuration is not changed: the provider is set for this session only. If %2 stops, the session waits; Zerus does not start the model again on its own.")
                          .arg(name, chosen));
        const QString state = models->currentData(Qt::UserRole + 1).toString();
        start->setEnabled(!agent.isEmpty() && !chosen.isEmpty() && (state.isEmpty() || state == "running" || state == "listed")
                          && !folder->currentText().trimmed().isEmpty() && !tag->text().trimmed().isEmpty());
    };
    QObject::connect(group, &QButtonGroup::buttonToggled, &dialog, update);
    QObject::connect(models, &QComboBox::currentIndexChanged, &dialog, update);
    QObject::connect(folder, &QComboBox::editTextChanged, &dialog, update);
    QObject::connect(tag, &QLineEdit::textChanged, &dialog, update);
    update();
    if (dialog.exec() != QDialog::Accepted) return;
    const QString agent = agentOf(), chosen = models->currentData().toString();
    if (startSession) startSession(agent, QDir::fromNativeSeparators(folder->currentText().trimmed()), tag->text().trimmed(), serverName, chosen);
    notice(tr("Starting %1 on %2 with %3…").arg(chosen, serverName, agent));
}
