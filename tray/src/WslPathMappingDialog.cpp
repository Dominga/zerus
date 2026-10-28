#include "WslPathMappingDialog.h"
#include "WslPathMapping.h"
#include <QCheckBox>
#include <QCryptographicHash>
#include <QDialogButtonBox>
#include <QFormLayout>
#include <QJsonArray>
#include <QJsonDocument>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QSettings>
#include <QUuid>
#include <QVBoxLayout>

WslPathMappingDialog::WslPathMappingDialog(const QString &hgsPath, const QString &host,
                                         const QString &machineId, const QJsonObject &profile, QWidget *parent)
    : QDialog(parent), m_client(hgsPath, this), m_host(host), m_machine(machineId), m_profile(profile)
{
    setObjectName("wslPathMappingDialog"); setWindowTitle(tr("Map a Windows path to this WSL environment")); resize(660,420);
    m_environment = "wsl:" + profile["distribution"].toString() + ':' + profile["user"].toString();
    m_key = WslPathMapping::key(host,machineId,profile);
    const auto saved = WslPathMapping::load(m_key);
    m_mappingId = saved["mapping_id"].toString(); if (m_mappingId.isEmpty()) m_mappingId = QUuid::createUuid().toString(QUuid::WithoutBraces);
    auto *layout = new QVBoxLayout(this);
    auto *target = new QLabel(tr("Machine: %1\nDistribution: %2\nLinux user: %3\nEnvironment: %4").arg(host,profile["distribution"].toString(),profile["user"].toString(),m_environment));
    target->setTextFormat(Qt::PlainText); target->setWordWrap(true); layout->addWidget(target);
    auto *intro = new QLabel(tr("Define the workspace alias explicitly. Preview is lexical only: it does not create folders, change saved project identities or prove that the mount exists."));
    intro->setWordWrap(true); intro->setTextFormat(Qt::PlainText); layout->addWidget(intro);
    auto *form = new QFormLayout;
    m_source = new QLineEdit; m_source->setObjectName("mappingWindowsPath");
    m_root = new QLineEdit(saved["windows_root"].toString()); m_root->setObjectName("mappingWindowsRoot"); m_root->setPlaceholderText("C:\\Projects\\Example");
    m_mount = new QLineEdit(saved["mount_root"].toString()); m_mount->setObjectName("mappingMountRoot"); m_mount->setPlaceholderText(tr("For example /mnt — enter the configured mount root"));
    m_workspace = new QLineEdit(saved["workspace_id"].toString()); m_workspace->setObjectName("mappingWorkspaceId");
    form->addRow(tr("Windows path to map"),m_source); form->addRow(tr("Allowed Windows workspace root"),m_root);
    form->addRow(tr("WSL drive mount root"),m_mount); form->addRow(tr("Workspace identity"),m_workspace); layout->addLayout(form);
    m_remember = new QCheckBox(tr("Remember this mapping on this device after using it")); m_remember->setObjectName("rememberWslMapping"); layout->addWidget(m_remember);
    m_notice = new QLabel(tr("Enter every mapping field, then preview the exact target path.")); m_notice->setObjectName("pathMappingNotice");
    m_notice->setWordWrap(true); m_notice->setTextFormat(Qt::PlainText); m_notice->setTextInteractionFlags(Qt::TextSelectableByMouse); layout->addWidget(m_notice);
    auto *buttons = new QDialogButtonBox;
    m_preview = buttons->addButton(tr("Preview mapping"),QDialogButtonBox::ActionRole); m_preview->setObjectName("previewWslMapping");
    m_use = buttons->addButton(tr("Use mapped path"),QDialogButtonBox::ActionRole); m_use->setObjectName("useWslMapping");
    m_cancel = buttons->addButton(QDialogButtonBox::Cancel); m_cancel->setDefault(true); m_preview->setAutoDefault(false);m_use->setAutoDefault(false);layout->addWidget(buttons);
    connect(m_source,&QLineEdit::textChanged,this,[this]{invalidate(false);});
    for(auto *field:{m_root,m_mount,m_workspace})connect(field,&QLineEdit::textChanged,this,[this]{invalidate(true);});
    connect(m_preview,&QPushButton::clicked,this,[this]{preview();}); connect(m_cancel,&QPushButton::clicked,this,&WslPathMappingDialog::reject);
    connect(m_use,&QPushButton::clicked,this,[this]{
        if(m_pending||m_profileRequest||m_mapped.isEmpty())return;
        m_profileRequest=m_client.requestMachines();m_notice->setText(tr("Rechecking the selected WSL connection…"));controls();
    });
    connect(&m_client,&HgsClient::machinesReady,this,[this](quint64 request,const QJsonObject &data){
        if(request!=m_profileRequest)return;m_profileRequest=0;
        QJsonObject current;
        for(const auto &value:data["machines"].toArray()){const auto row=value.toObject();if(row["alias"]==m_host)current=row["connection"].toObject();}
        if(data["local"]!=m_machine||current["transport"]!="wsl-local"||current["wsl"].toObject()!=m_profile){fail(tr("The WSL connection changed. Close this preview and select the machine again."));return;}
        if(m_remember->isChecked())SettingsJson::store(m_key,QJsonDocument(m_mapping));
        QDialog::accept();
    });
    connect(&m_client,&HgsClient::machinesFailed,this,[this](quint64 request,const QString &error){if(request==m_profileRequest){m_profileRequest=0;fail(error);}});
    connect(&m_client,&HgsClient::pathRefFailed,this,[this](quint64 request,const QString &error){if(request==m_pending){m_pending=0;fail(error);}});
    connect(&m_client,&HgsClient::pathRefReady,this,[this](quint64 request,const QJsonObject &result){
        if(request!=m_pending)return;m_pending=0;
        const bool common=result["schema_version"].toInt()==1&&result["machine_id"]==m_machine
            &&result["workspace_id"]==m_mapping["workspace_id"]&&result["mapping_id"]==m_mappingId;
        if(!m_mappingPhase){
            if(!common||result["environment_id"]!="windows-native"||result["path_flavor"]!="windows"||result["native_path"]!=m_input["native_path"]){fail(tr("The source identity response does not match this mapping."));return;}
            m_mappingPhase=true;m_input=result;m_pending=m_client.requestPathRef("map",{{"path",result},{"mapping",m_mapping}});controls();return;
        }
        const auto path=result["native_path"].toString();
        if(!common||result["environment_id"]!=m_environment||result["path_flavor"]!="posix"
            ||result["canonical_path"]!=m_input["canonical_path"]||!path.startsWith('/')||path.startsWith("//")||path.contains(QChar::Null)){
            fail(tr("The mapped response does not match the selected environment."));return;
        }
        m_mapped=result;m_notice->setText(tr("Windows: %1\nWSL: %2\nCanonical identity: %3\nThe folder browser will verify the native directory before it can be selected.")
            .arg(m_source->text(),path,result["canonical_path"].toString()));controls();
    });
    controls();
}
QJsonObject WslPathMappingDialog::mapping() const
{
    return {{"mapping_id",m_mappingId},{"workspace_id",m_workspace->text()},{"machine_id",m_machine},
        {"windows_environment_id","windows-native"},{"wsl_environment_id",m_environment},
        {"distribution",m_profile["distribution"]},{"user",m_profile["user"]},
        {"mount_root",m_mount->text()},{"windows_root",m_root->text()}};
}
void WslPathMappingDialog::invalidate(bool changedMapping)
{
    m_mapped={};if(changedMapping)m_mappingId=QUuid::createUuid().toString(QUuid::WithoutBraces);
    m_notice->setText(tr("The selection changed. Preview its exact mapping before using it."));controls();
}
void WslPathMappingDialog::controls()
{
    const bool busy=m_pending||m_profileRequest;
    for(auto *field:{m_source,m_root,m_mount,m_workspace})field->setEnabled(!busy);
    m_remember->setEnabled(!busy);m_cancel->setEnabled(!busy);
    m_preview->setEnabled(!busy&&!m_machine.isEmpty()&&!m_profile["distribution"].toString().isEmpty()&&!m_profile["user"].toString().isEmpty()
        &&!m_source->text().isEmpty()&&!m_root->text().isEmpty()&&!m_mount->text().isEmpty()&&!m_workspace->text().isEmpty());
    m_use->setEnabled(!busy&&!m_mapped.isEmpty());
}
void WslPathMappingDialog::preview()
{
    if(m_pending||m_profileRequest)return;m_mapped={};m_mapping=mapping();m_mappingPhase=false;
    m_input={{"machine_id",m_machine},{"environment_id","windows-native"},{"path_flavor","windows"},
        {"native_path",m_source->text()},{"workspace_id",m_workspace->text()}};
    m_pending=m_client.requestPathRef("describe",{{"path",m_input},{"mapping",m_mapping}});
    m_notice->setText(tr("Validating the explicit mapping…"));controls();
}
void WslPathMappingDialog::fail(const QString &message){m_mapped={};m_notice->setText(message);controls();}
void WslPathMappingDialog::reject(){if(!m_pending&&!m_profileRequest)QDialog::reject();}
