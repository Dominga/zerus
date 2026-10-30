#include "ModelServerSettings.h"
#include "TeamWidgets.h"
#include <QButtonGroup>
#include <QComboBox>
#include <QGridLayout>
#include <QJsonArray>
#include <QLineEdit>
#include <QListWidget>
#include <QMessageBox>
#include <QPlainTextEdit>
#include <QScrollArea>

using namespace TeamUi;
using ModelServers::Server;

namespace {
QPushButton *segment(const QString &text, const char *name, QButtonGroup *group)
{
    auto *b = button(text, name); b->setCheckable(true); group->addButton(b);
    return b;
}
}

ModelServerSettings::ModelServerSettings(ModelServers::Controller *servers, QWidget *parent) : QWidget(parent), m_servers(servers)
{
    setObjectName("modelServerSettings");
    auto *outer = new QVBoxLayout(this); outer->setContentsMargins(0, 0, 0, 0); outer->setSpacing(10);
    auto *head = row(10);
    m_title = new QLabel(tr("Model servers")); m_title->setObjectName("modelServerSettingsTitle");
    auto *add = button(tr("Add server"), "modelServerAdd", true);
    QObject::connect(add, &QPushButton::clicked, this, [this] { addServer(); });
    head->addWidget(m_title); head->addStretch(1); head->addWidget(add);
    outer->addLayout(head);
    auto *hint = new QLabel(tr("Computers on your network, or this one, that serve models through an OpenAI- or Anthropic-compatible API. Kept on this computer only."));
    hint->setObjectName("modelServerSettingsHint"); hint->setWordWrap(true); outer->addWidget(hint);
    auto *body = new QHBoxLayout; body->setSpacing(16);
    m_list = new QListWidget; m_list->setObjectName("modelServerList"); m_list->setFixedWidth(260);
    QObject::connect(m_list, &QListWidget::currentRowChanged, this, [this](int row) {
        if (row < 0) return;
        load(m_servers->server(m_list->item(row)->data(Qt::UserRole).toString()));
    });
    body->addWidget(m_list, 0, Qt::AlignTop);
    m_form = new QFrame; m_form->setObjectName("modelServerForm");
    auto *grid = new QGridLayout(m_form); grid->setContentsMargins(18, 16, 18, 16); grid->setHorizontalSpacing(14); grid->setVerticalSpacing(8);
    grid->setColumnStretch(1, 1);
    int r = 0;
    const auto key = [&](const QString &text) { auto *k = new QLabel(text); k->setObjectName("modelServerFieldName"); k->setAlignment(Qt::AlignRight | Qt::AlignVCenter); grid->addWidget(k, r, 0); };
    const auto result = [&](const char *name) { auto *l = new QLabel; l->setObjectName(name); l->setWordWrap(true); l->hide(); grid->addWidget(l, ++r, 1, 1, 2); return l; };
    key(tr("Name"));
    m_name = new QLineEdit; m_name->setObjectName("modelServerName"); m_name->setPlaceholderText(tr("lab"));
    grid->addWidget(m_name, r, 1, 1, 2);
    // #703 (#658 N2): the server's root; hgs derives each consumer's URL from it.
    ++r; key(tr("Server root"));
    auto *apiRow = row(8);
    m_url = new QLineEdit; m_url->setObjectName("modelServerUrl"); m_url->setPlaceholderText(QStringLiteral("http://host.example:8000"));
    m_url->setFont(NewSessionDense::mono(12));
    auto *kinds = new QWidget; kinds->setProperty("seg", true); auto *kh = new QHBoxLayout(kinds); kh->setContentsMargins(2, 2, 2, 2); kh->setSpacing(0);
    auto *kindGroup = new QButtonGroup(this);
    m_openai = segment(tr("OpenAI"), "modelServerKindOpenai", kindGroup); m_anthropic = segment(tr("Anthropic"), "modelServerKindAnthropic", kindGroup);
    kh->addWidget(m_openai); kh->addWidget(m_anthropic);
    auto *testApi = button(tr("Test API"), "modelServerTestApi");
    QObject::connect(testApi, &QPushButton::clicked, this, [this] { test(QStringLiteral("api")); });
    apiRow->addWidget(m_url, 1); apiRow->addWidget(kinds); apiRow->addWidget(testApi);
    grid->addLayout(apiRow, r, 1, 1, 2);
    m_urlHint = new QLabel; m_urlHint->setObjectName("modelServerSettingsNote"); m_urlHint->setWordWrap(true);
    m_urlHint->setProperty("urlHint", true); m_urlHint->setTextFormat(Qt::PlainText);
    grid->addWidget(m_urlHint, ++r, 1, 1, 2);
    const auto hintUrl = [this] {
        const QString typed = m_url->text().trimmed();
        if (typed.isEmpty()) {
            m_urlHint->setText(tr("The address the server answers at, without /v1: http(s)://HOST[:PORT][/path]. Zerus reads its model list at ROOT/v1/models, gives Codex ROOT/v1 and Claude Code ROOT."));
            return;
        }
        const QString root = ModelServers::rootOf(typed);
        QString text = tr("Model list: %1\nCodex: %2\nClaude Code: %3").arg(ModelServers::modelsUrl(typed), root + QStringLiteral("/v1"), root);
        QString bare = typed;
        while (bare.endsWith(QLatin1Char('/'))) bare.chop(1);
        if (bare.endsWith(QStringLiteral("/v1")))
            text += QLatin1Char('\n') + tr("Saved as its root %1: the /v1 you typed is added where it is needed.").arg(root);
        m_urlHint->setText(text);
    };
    QObject::connect(m_url, &QLineEdit::textChanged, this, hintUrl);
    hintUrl();
    m_apiResult = result("modelServerApiResult");
    ++r; key(tr("API key"));
    auto *keyRow = row(8);
    m_stored = new QLabel; m_stored->setObjectName("modelServerKeyStored");
    m_key = new QLineEdit; m_key->setObjectName("modelServerKey"); m_key->setEchoMode(QLineEdit::Password); m_key->setPlaceholderText(tr("none"));
    m_replace = button(tr("Replace"), "modelServerKeyReplace");
    m_removeKey = button(tr("Remove"), "modelServerKeyRemove"); m_removeKey->setProperty("danger", true);
    QObject::connect(m_replace, &QPushButton::clicked, this, [this] { m_keyChange = QStringLiteral("replace"); setKeyMode(); m_key->setFocus(); });
    QObject::connect(m_removeKey, &QPushButton::clicked, this, [this] { m_keyChange = QStringLiteral("remove"); setKeyMode(); });
    keyRow->addWidget(m_stored, 1); keyRow->addWidget(m_key, 1); keyRow->addWidget(m_replace); keyRow->addWidget(m_removeKey);
    grid->addLayout(keyRow, r, 1, 1, 2);
    ++r; key(tr("SSH host"));
    auto *sshRow = row(8);
    auto *hosts = new QWidget; hosts->setProperty("seg", true); auto *hh = new QHBoxLayout(hosts); hh->setContentsMargins(2, 2, 2, 2); hh->setSpacing(0);
    auto *sshGroup = new QButtonGroup(this);
    m_sshNone = segment(tr("None"), "modelServerSshNone", sshGroup); m_sshMachine = segment(tr("Machine"), "modelServerSshMachine", sshGroup);
    m_sshAlias = segment(tr("SSH alias"), "modelServerSshAlias", sshGroup);
    hh->addWidget(m_sshNone); hh->addWidget(m_sshMachine); hh->addWidget(m_sshAlias);
    m_machine = new QComboBox; m_machine->setObjectName("modelServerMachine");
    m_alias = new QLineEdit; m_alias->setObjectName("modelServerAlias"); m_alias->setPlaceholderText(tr("an alias from your SSH configuration"));
    sshRow->addWidget(hosts); sshRow->addWidget(m_machine, 1); sshRow->addWidget(m_alias, 1);
    grid->addLayout(sshRow, r, 1, 1, 2);
    QObject::connect(sshGroup, &QButtonGroup::buttonToggled, this, [this] {
        m_machine->setVisible(m_sshMachine->isChecked()); m_alias->setVisible(m_sshAlias->isChecked());
    });
    ++r;
    auto *transport = new QLabel(tr("Reached through hgs's own SSH transport (BatchMode); hgs is not needed on the server."));
    transport->setObjectName("modelServerSettingsNote"); transport->setWordWrap(true); grid->addWidget(transport, r, 1, 1, 2);
    ++r;
    auto *control = new QLabel(tr("<b>CONTROL COMMANDS</b> optional, argv templates; {model} is the model, {server} the server"));
    control->setObjectName("modelServerControlTitle"); control->setTextFormat(Qt::RichText); grid->addWidget(control, r, 0, 1, 3);
    const auto command = [&](const QString &name, QLineEdit *&edit, const char *object, QPushButton **testButton, const QString &what) {
        ++r; key(name);
        edit = new QLineEdit; edit->setObjectName(object); edit->setPlaceholderText(tr("not set")); edit->setFont(NewSessionDense::mono(12));
        grid->addWidget(edit, r, 1);
        if (testButton) {
            auto *b = button(tr("Test"), what == "status" ? "modelServerTestStatus" : "modelServerTestLogs");
            b->setProperty("pageIcon", QStringLiteral("play")); *testButton = b;
            QObject::connect(b, &QPushButton::clicked, this, [this, what] { test(what); });
            grid->addWidget(b, r, 2);
        }
    };
    QPushButton *testStatus = nullptr;
    command(tr("Status"), m_status, "modelServerStatusCommand", &testStatus, QStringLiteral("status"));
    m_statusResult = result("modelServerStatusResult");
    ++r;
    m_raw = new QPlainTextEdit; m_raw->setObjectName("modelServerStatusRaw"); m_raw->setReadOnly(true); m_raw->setFixedHeight(80);
    m_raw->setFont(NewSessionDense::mono(12)); m_raw->hide(); grid->addWidget(m_raw, r, 1, 1, 2);
    command(tr("Up NAME"), m_up, "modelServerUpCommand", nullptr, {});
    command(tr("Down NAME"), m_down, "modelServerDownCommand", nullptr, {});
    ++r;
    auto *tab = new QLabel(tr("Up and Down run from the Model servers tab, after its confirmations: the server may be shared."));
    tab->setObjectName("modelServerSettingsNote"); tab->setWordWrap(true); grid->addWidget(tab, r, 1, 1, 2);
    command(tr("Logs NAME"), m_logs, "modelServerLogsCommand", &m_testLogs, QStringLiteral("logs"));
    m_logsResult = result("modelServerLogsResult");
    ++r;
    auto *formats = new QLabel(QStringLiteral("{\"models\":[{\"name\":\"small\",\"state\":\"running\"}],\"memory\":{\"free\":12.5,\"total\":64}}\n")
                               + tr("or plain lines:  small: running   large: stopped   memory: 12.5/64 GB"));
    formats->setObjectName("modelServerFormats"); formats->setFont(NewSessionDense::mono(11)); formats->setTextFormat(Qt::PlainText);
    formats->setWordWrap(true); grid->addWidget(formats, r, 0, 1, 3);
    ++r;
    m_saveError = new QLabel; m_saveError->setObjectName("modelServerSaveError"); m_saveError->setWordWrap(true); m_saveError->hide();
    grid->addWidget(m_saveError, r, 0, 1, 3);
    ++r;
    auto *buttons = row(8);
    m_delete = button(tr("Delete server…"), "modelServerDelete"); m_delete->setProperty("danger", true);
    m_delete->setProperty("pageIcon", QStringLiteral("trash"));
    auto *cancel = button(tr("Cancel"), "modelServerCancel");
    m_save = button(tr("Save"), "modelServerSave", true);
    QObject::connect(m_delete, &QPushButton::clicked, this, [this] { remove(); });
    QObject::connect(cancel, &QPushButton::clicked, this, [this] { m_was.isEmpty() ? showServer({}) : load(m_servers->server(m_was)); });
    QObject::connect(m_save, &QPushButton::clicked, this, [this] { save(); });
    buttons->addWidget(m_delete); buttons->addStretch(1); buttons->addWidget(cancel); buttons->addWidget(m_save);
    grid->addLayout(buttons, r, 0, 1, 3);
    grid->setRowStretch(r + 1, 1);   // the fields keep their height; the room goes under them
    auto *scroll = new QScrollArea; scroll->setObjectName("modelServerFormScroll"); scroll->setWidgetResizable(true); scroll->setFrameShape(QFrame::NoFrame);
    scroll->setWidget(m_form);
    body->addWidget(scroll, 1);
    outer->addLayout(body, 1);
    QObject::connect(m_servers, &ModelServers::Controller::changed, this, [this] { renderList(); });
    setDark(true);
    renderList();
    showServer({});
}

void ModelServerSettings::setDark(bool dark)
{
    m_dark = dark;
    const Colors c = colorsFor(dark);
    setStyleSheet(teamSheet(c) + QStringLiteral(
        "QLabel#modelServerSettingsTitle{color:%1;font-size:18px;font-weight:600;}"
        "QLabel#modelServerSettingsHint,QLabel#modelServerSettingsNote{color:%2;font-size:12px;}"
        "QLabel#modelServerFieldName{color:%3;}"
        "QLabel#modelServerControlTitle{color:%3;font-size:12px;}"
        "QLabel#modelServerKeyStored{color:%3;}"
        "QLabel#modelServerFormats{background:%4;border:1px solid %5;border-radius:8px;padding:8px 10px;color:%3;}"
        "QFrame#modelServerForm{background:%6;border:1px solid %5;border-radius:12px;}"
        "QFrame#modelServerForm QLabel{background:transparent;}"
        "QListWidget#modelServerList{background:transparent;border:0;outline:0;}"
        "QListWidget#modelServerList::item{border:1px solid transparent;border-radius:9px;padding:8px 10px;margin-bottom:4px;color:%1;}"
        "QListWidget#modelServerList::item:selected{background:%7;border-color:%8;}"
        "QLabel[result=\"ok\"]{color:%9;} QLabel[result=\"warn\"]{color:%10;} QLabel[result=\"error\"]{color:%11;}"
        "QLabel#modelServerSaveError{color:%11;}")
        .arg(c.fg.name(), c.muted.name(), c.text2.name(), c.tile.name(), c.cardBorder.name(), c.cardFill.name(),
             WorkspaceSkin::color(WorkspaceSkin::active(), dark, QStringLiteral("cardSel")).name(),
             WorkspaceSkin::color(WorkspaceSkin::active(), dark, QStringLiteral("cardSelBorder")).name(), c.accent.name())
        .arg(c.warn.name(), c.danger.name()));
    for (auto *b : findChildren<QPushButton *>())
        if (const auto icon = b->property("pageIcon").toString(); !icon.isEmpty()) { b->setIcon(workspaceIcon(icon, b->property("danger").toBool() ? c.danger : c.fg)); b->setIconSize(QSize(13, 13)); }
    renderList();
}

void ModelServerSettings::renderList()
{
    const Colors c = colorsFor(m_dark);
    const QString current = m_was;
    const QSignalBlocker block(m_list);
    m_list->clear();
    for (const auto &server : m_servers->servers()) {
        const auto status = m_servers->status(server.name);
        const QString state = status.known && !status.error.isEmpty() ? tr("not read") : server.readOnly() ? tr("read only") : QString();
        auto *item = new QListWidgetItem(server.name + (state.isEmpty() ? QString() : QStringLiteral("   ") + state) + QLatin1Char('\n') + server.baseUrl, m_list);
        item->setData(Qt::UserRole, server.name);
        item->setIcon(workspaceIcon(QStringLiteral("server"), status.known && !status.error.isEmpty() ? c.danger : c.text2));
        if (server.name == current) m_list->setCurrentItem(item);
    }
}

void ModelServerSettings::showServer(const QString &name)
{
    const auto *server = name.isEmpty() && !m_servers->servers().isEmpty() ? &m_servers->servers().first() : m_servers->server(name);
    if (!server) { addServer(); return; }
    load(server);
}

void ModelServerSettings::addServer()
{
    m_list->clearSelection(); m_list->setCurrentRow(-1);
    load(nullptr);
    m_name->setFocus();
}

void ModelServerSettings::load(const Server *server)
{
    m_was = server ? server->name : QString();
    m_hasKey = server && server->hasKey;
    m_keyChange.clear();
    m_name->setText(server ? server->name : QString());
    m_url->setText(server ? server->baseUrl : QString());
    (server && server->kind == "anthropic" ? m_anthropic : m_openai)->setChecked(true);
    m_key->clear();
    m_machine->clear();
    const QStringList known = machines ? machines() : QStringList();
    m_machine->addItems(known);
    const QString ssh = server ? server->ssh : QString();
    if (ssh.isEmpty()) m_sshNone->setChecked(true);
    else if (known.contains(ssh)) { m_sshMachine->setChecked(true); m_machine->setCurrentText(ssh); }
    else { m_sshAlias->setChecked(true); m_alias->setText(ssh); }
    if (!m_sshAlias->isChecked()) m_alias->clear();
    m_machine->setVisible(m_sshMachine->isChecked()); m_alias->setVisible(m_sshAlias->isChecked());
    m_status->setText(server ? ModelServers::argvText(server->status) : QString());
    m_up->setText(server ? ModelServers::argvText(server->up) : QString());
    m_down->setText(server ? ModelServers::argvText(server->down) : QString());
    m_logs->setText(server ? ModelServers::argvText(server->logs) : QString());
    for (auto *label : {m_apiResult, m_statusResult, m_logsResult, m_saveError}) { label->clear(); label->hide(); }
    m_raw->hide();
    m_delete->setVisible(server);
    setKeyMode();
}

void ModelServerSettings::setKeyMode()
{
    // A saved key is never shown again: only that one is stored.
    const bool stored = m_hasKey && m_keyChange.isEmpty();
    m_stored->setText(m_keyChange == "remove" ? tr("removed when you save") : tr("••••••••••••  in the private store"));
    m_stored->setVisible(stored || m_keyChange == "remove");
    m_key->setVisible(!stored && m_keyChange != "remove");
    m_replace->setVisible(stored);
    m_removeKey->setVisible(stored);
}

QJsonObject ModelServerSettings::formServer(bool withControl) const
{
    QJsonObject server{{"name", m_name->text().trimmed()}, {"base_url", m_url->text().trimmed()},
                       {"kind", m_anthropic->isChecked() ? "anthropic" : "openai"}};
    const QString ssh = m_sshMachine->isChecked() ? m_machine->currentText() : m_sshAlias->isChecked() ? m_alias->text().trimmed() : QString();
    if (!ssh.isEmpty()) server.insert("ssh", ssh);
    if (withControl) {
        const QList<std::pair<const char *, QLineEdit *>> fields{{"status", m_status}, {"up", m_up}, {"down", m_down}, {"logs", m_logs}};
        for (const auto &[field, edit] : fields)
            if (const auto argv = ModelServers::argvFromText(edit->text()); !argv.isEmpty()) server.insert(QLatin1String(field), QJsonArray::fromStringList(argv));
    }
    if (const auto *stored = m_servers->server(m_was); stored && stored->upMinutes > 0) server.insert("up_minutes", stored->upMinutes);
    if (!m_was.isEmpty()) server.insert("was", m_was);
    // The key only when you typed one or removed it; otherwise the stored one stays.
    if (m_keyChange == "remove") server.insert("key", QJsonValue::Null);
    else if (!m_key->text().isEmpty()) server.insert("key", m_key->text());
    return server;
}

void ModelServerSettings::save()
{
    QJsonArray servers;
    bool replaced = false;
    for (const auto &server : m_servers->servers()) {
        if (server.name == m_was) { servers.append(formServer(true)); replaced = true; continue; }
        auto other = server.json(); other.insert("was", server.name);
        servers.append(other);
    }
    if (!replaced) servers.append(formServer(true));
    m_save->setEnabled(false);
    const QString name = m_name->text().trimmed();
    m_servers->save(servers, [this, name](const QJsonObject &result, const QString &error) {
        m_save->setEnabled(true);
        const QString why = !error.isEmpty() ? error : result.value("status").toString() != "saved" ? result.value("detail").toString(tr("Not saved")) : QString();
        if (!why.isEmpty()) { m_saveError->setText(tr("Not saved: %1").arg(why)); m_saveError->show(); return; }
        m_key->clear();
        renderList();
        load(m_servers->server(name));
        m_servers->refreshStatus(name);
    });
}

void ModelServerSettings::remove()
{
    if (m_was.isEmpty()) return;
    const QString title = tr("Delete %1?").arg(m_was);
    const QString text = tr("Delete %1 from this computer's model servers? Its key is removed with it. Nothing on the server changes and no model stops.").arg(m_was);
    bool yes = false;
    if (confirm) yes = confirm(title, text, tr("Delete server"));
    else {
        QMessageBox box(QMessageBox::Warning, title, text, QMessageBox::NoButton, this);
        box.setObjectName("modelServerDeleteConfirm");
        auto *ok = box.addButton(tr("Delete server"), QMessageBox::DestructiveRole);
        box.setDefaultButton(box.addButton(QMessageBox::Cancel));
        box.exec();
        yes = box.clickedButton() == ok;
    }
    if (!yes) return;
    QJsonArray servers;
    for (const auto &server : m_servers->servers()) {
        if (server.name == m_was) continue;
        auto other = server.json(); other.insert("was", server.name);
        servers.append(other);
    }
    m_servers->save(servers, [this](const QJsonObject &result, const QString &error) {
        const QString why = !error.isEmpty() ? error : result.value("status").toString() != "saved" ? result.value("detail").toString(tr("Not saved")) : QString();
        if (!why.isEmpty()) { m_saveError->setText(tr("Not deleted: %1").arg(why)); m_saveError->show(); return; }
        renderList();
        showServer({});
    });
}

void ModelServerSettings::test(const QString &what)
{
    QLabel *target = what == "api" ? m_apiResult : what == "logs" ? m_logsResult : m_statusResult;
    target->setProperty("result", QString()); target->setText(tr("Testing once…")); target->show();
    target->style()->unpolish(target); target->style()->polish(target);
    if (what == "logs") {
        // Logs reads the saved server once, with a model its status listed.
        const auto *saved = m_servers->server(m_was);
        const auto status = m_servers->status(m_was);
        if (!saved || saved->logs != ModelServers::argvFromText(m_logs->text())) { showResult(target, {}, tr("Save the server first: Logs reads the saved command"), what); return; }
        const QString model = status.models.isEmpty() ? QString() : status.models.first().name;
        if (model.isEmpty()) { showResult(target, {}, tr("Read the status first: Logs needs one of the server's models"), what); return; }
        m_servers->logs(m_was, model, [this, target, model](const QJsonObject &result, const QString &error) { showResult(target, result, error, model); });
        return;
    }
    // Test API asks the API itself (no status command); Test asks the status command.
    m_servers->test(formServer(what == "status"), [this, target, what](const QJsonObject &result, const QString &error) { showResult(target, result, error, what); });
}

void ModelServerSettings::showResult(QLabel *target, const QJsonObject &result, const QString &error, const QString &what)
{
    QString text, tone = QStringLiteral("ok");
    const bool failed = !error.isEmpty() || result.value("status").toString() == "error";
    // #703 (#658 N2): Test API reads the root's model list; say which URL.
    const QString read = target == m_apiResult ? ModelServers::modelsUrl(m_url->text()) : QString();
    if (failed) {
        tone = QStringLiteral("error");
        text = !error.isEmpty() ? error : result.value("detail").toString(result.value("error").toString());
        if (!read.isEmpty()) text = tr("GET %1: %2").arg(read, text);
    } else if (target == m_logsResult) {
        text = tr("%1: %2 lines read").arg(what).arg(result.value("lines").toArray().size());
    } else {
        const auto status = ModelServers::parseStatus(result);
        const int models = int(status.models.size());
        const QString count = models == 1 ? tr("1 model") : tr("%1 models").arg(models);
        if (status.source == "api") text = tr("GET %1 answered: %2").arg(read.isEmpty() ? QStringLiteral("/v1/models") : read, count);
        else if (!status.raw.isEmpty() && status.models.isEmpty()) {
            tone = QStringLiteral("warn");
            text = tr("Neither the JSON contract nor NAME: STATE lines: shown as raw text; the models come from the API");
        } else text = status.hasMemory() ? tr("%1, %2").arg(count, ModelServers::memoryText(status.free, status.total)) : count;
        if (target == m_statusResult) {
            m_raw->setPlainText(status.raw.join(QLatin1Char('\n')));
            m_raw->setVisible(!status.raw.isEmpty());
        }
    }
    target->setText(text); target->setProperty("result", tone); target->show();
    target->style()->unpolish(target); target->style()->polish(target);
}
