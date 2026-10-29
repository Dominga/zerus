#include "OpenCodeExistingDialog.h"
#include <QDialogButtonBox>
#include <QFormLayout>
#include <QJsonDocument>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QRegularExpression>
#include <QUuid>
#include <QVBoxLayout>

OpenCodeExistingDialog::OpenCodeExistingDialog(const QString &hgsPath, const QString &host,
                                               const QString &agentHome, QWidget *parent)
    : QDialog(parent), m_client(hgsPath, this), m_host(host), m_home(agentHome)
{
    setObjectName("openCodeExistingDialog");
    setWindowTitle(tr("Bind an existing OpenCode conversation"));
    resize(640, 380);
    auto *layout = new QVBoxLayout(this);
    auto *intro = new QLabel(tr("Choose one native conversation. Preview verifies its exact ID, directory and profile. "
        "Binding adds it to Zerus without creating a native conversation, changing its history or sending a message."));
    intro->setWordWrap(true); intro->setTextFormat(Qt::PlainText); layout->addWidget(intro);
    auto *target = new QLabel(tr("Machine: %1\nNative profile: %2").arg(host.isEmpty() ? tr("This computer") : host, agentHome));
    target->setObjectName("existingConversationTarget"); target->setTextFormat(Qt::PlainText);
    target->setWordWrap(true); target->setTextInteractionFlags(Qt::TextSelectableByMouse); layout->addWidget(target);
    auto *form = new QFormLayout;
    m_nativeId = new QLineEdit; m_nativeId->setObjectName("existingConversationId"); m_nativeId->setPlaceholderText("ses_...");
    m_directory = new QLineEdit; m_directory->setObjectName("existingConversationDirectory"); m_directory->setPlaceholderText(tr("Exact absolute directory inside the selected environment"));
    m_name = new QLineEdit; m_name->setObjectName("existingConversationName"); m_name->setPlaceholderText("opencode/project/session");
    form->addRow(tr("Native conversation ID"), m_nativeId); form->addRow(tr("Native directory"), m_directory);
    form->addRow(tr("Zerus session name"), m_name); layout->addLayout(form);
    m_notice = new QLabel; m_notice->setObjectName("existingConversationNotice"); m_notice->setTextFormat(Qt::PlainText);
    m_notice->setWordWrap(true); m_notice->setTextInteractionFlags(Qt::TextSelectableByMouse); layout->addWidget(m_notice);
    auto *buttons = new QDialogButtonBox;
    m_preview = buttons->addButton(tr("Preview"), QDialogButtonBox::ActionRole); m_preview->setObjectName("previewExistingConversation");
    m_bind = buttons->addButton(tr("Bind conversation"), QDialogButtonBox::ActionRole); m_bind->setObjectName("bindExistingConversation");
    m_cancel = buttons->addButton(QDialogButtonBox::Cancel);
    m_preview->setAutoDefault(false); m_bind->setAutoDefault(false); m_cancel->setDefault(true);
    layout->addWidget(buttons);
    for (auto *field : {m_nativeId, m_directory, m_name}) connect(field, &QLineEdit::textChanged, this, [this] { invalidate(); });
    connect(m_cancel, &QPushButton::clicked, this, &OpenCodeExistingDialog::reject);
    connect(m_preview, &QPushButton::clicked, this, [this] { submit(false); });
    connect(m_bind, &QPushButton::clicked, this, [this] { submit(true); });
    connect(&m_client, &HgsClient::accountsReady, this, [this](quint64 request, const QString &target, const QJsonObject &result) { receive(request, target, result); });
    connect(&m_client, &HgsClient::accountsFailed, this, [this](quint64 request, const QString &target, const QString &error) {
        if (request != m_pending || target != m_host) return;
        m_pending = 0; m_identity.clear();
        m_notice->setText(m_committing ? tr("Binding was not confirmed. Preview this same selection to check its recorded result.\n%1").arg(error) : error);
        controls();
    });
    invalidate();
}

QJsonObject OpenCodeExistingDialog::selection() const
{
    return {{"schema_version",1},{"request_id",m_requestId},{"name",m_name->text()},
        {"conversation_id",m_nativeId->text()},{"directory",m_directory->text()},{"agent_home",m_home}};
}
void OpenCodeExistingDialog::invalidate()
{
    m_identity.clear(); m_requestId = QUuid::createUuid().toString(QUuid::WithoutBraces);
    m_notice->setText(tr("No binding will be saved until you preview and choose Bind conversation.")); controls();
}
void OpenCodeExistingDialog::controls()
{
    const bool busy = m_pending != 0;
    const bool filled = !m_home.isEmpty() && !m_nativeId->text().isEmpty() && !m_directory->text().isEmpty() && !m_name->text().isEmpty();
    for (auto *field : {m_nativeId, m_directory, m_name}) field->setEnabled(!busy);
    m_preview->setEnabled(!busy && filled); m_bind->setEnabled(!busy && filled && !m_identity.isEmpty()); m_cancel->setEnabled(!busy);
}
void OpenCodeExistingDialog::submit(bool commit)
{
    if (m_pending || (commit && m_identity.isEmpty())) return;
    m_sent = selection(); m_sent["preview"] = !commit;
    if (commit) m_sent["expected_identity"] = m_identity;
    m_committing = commit;
    m_pending = m_client.requestAccounts(m_host, {"bind-existing","native-opencode","--json"}, QJsonDocument(m_sent).toJson(QJsonDocument::Compact));
    m_notice->setText(commit ? tr("Recording the selected binding…") : tr("Verifying the selected native conversation…")); controls();
}
void OpenCodeExistingDialog::receive(quint64 request, const QString &host, const QJsonObject &result)
{
    if (request != m_pending || host != m_host) return;
    m_pending = 0;
    bool exact = result["schema_version"].toInt() == 1 && result["native_changed"].isBool() && !result["native_changed"].toBool();
    for (const auto *key : {"request_id","name","conversation_id","directory","agent_home"})
        exact = exact && result.value(key) == m_sent.value(key) && result.value(key) == selection().value(key);
    const auto token = result["identity"].toString(), status = result["status"].toString();
    exact = exact && QRegularExpression("^[0-9a-f]{64}$").match(token).hasMatch();
    if (m_committing) exact = exact && token == m_identity && (status == "bound" || status == "already_bound")
        && !QUuid(result["run_id"].toString()).isNull();
    else exact = exact && (status == "preview" || status == "already_bound");
    if (!exact) { m_identity.clear(); m_notice->setText(tr("The reply does not match this selection. Nothing is confirmed; preview again.")); controls(); return; }
    if (m_committing) { m_bound = result; QDialog::accept(); return; }
    m_identity = token;
    m_notice->setText(tr("Verified native title: %1\n%2\nNative ID, directory and profile match. No native data will be changed.")
        .arg(result["native_title"].toString(), status == "already_bound" ? tr("This exact conversation is already bound; its run identity will be preserved.") : tr("Ready to create one local Zerus binding.")));
    controls();
}
void OpenCodeExistingDialog::reject()
{
    // Do not close an admitted local write while its result is still pending.
    if (!m_pending) QDialog::reject();
}
