#include "WslFileOpenDialog.h"
#include "WslPathMapping.h"
#include "DownloadCache.h"
#include <QCheckBox>
#include <QDialogButtonBox>
#include <QFormLayout>
#include <QJsonArray>
#include <QJsonDocument>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QRegularExpression>
#include <QSettings>
#include <QUuid>
#include <QVBoxLayout>

WslFileOpenDialog::WslFileOpenDialog(const QString &hgsPath,const QString &host,const QJsonObject &scope,
                                   QWidget *parent,Verify verify,Open open)
    :QDialog(parent),m_client(hgsPath,this),m_host(host),m_scope(scope),
      m_verify(verify?std::move(verify):Verify(MappedLocalFile::verify)),
      m_opener(open?std::move(open):Open([](const QString &path){return ExternalFile::open(path,ExternalFile::Origin::ThisComputer);}))
{
    setObjectName("wslFileOpenDialog");setWindowTitle(tr("Open a mapped Windows original"));resize(720,440);
    auto *layout=new QVBoxLayout(this);
    m_target=new QLabel(tr("Reading the selected local WSL connection…"));m_target->setObjectName("mappedFileTarget");
    m_target->setTextFormat(Qt::PlainText);m_target->setWordWrap(true);layout->addWidget(m_target);
    auto *source=new QLabel(tr("Conversation: %1\nNative path: %2").arg(scope["session"].toString(),scope["path"].toString()));
    source->setTextFormat(Qt::PlainText);source->setWordWrap(true);source->setTextInteractionFlags(Qt::TextSelectableByMouse);layout->addWidget(source);
    auto *intro=new QLabel(tr("This opens the original Windows file, not a downloaded copy. Editing it changes that file. "
        "The selected conversation must still authorize the WSL read, and both sides must have matching contents. "
        "An explicit mapping is an alias you select, not proof that two files are the same filesystem object."));
    intro->setTextFormat(Qt::PlainText);intro->setWordWrap(true);layout->addWidget(intro);
    auto *form=new QFormLayout;
    m_root=new QLineEdit;m_root->setObjectName("fileMappingWindowsRoot");m_root->setPlaceholderText("C:\\Projects\\Example");
    m_mount=new QLineEdit;m_mount->setObjectName("fileMappingMountRoot");m_mount->setPlaceholderText(tr("Explicit WSL drive mount root"));
    m_workspace=new QLineEdit;m_workspace->setObjectName("fileMappingWorkspaceId");
    form->addRow(tr("Allowed Windows workspace root"),m_root);form->addRow(tr("WSL drive mount root"),m_mount);form->addRow(tr("Workspace identity"),m_workspace);layout->addLayout(form);
    m_remember=new QCheckBox(tr("Remember this explicit mapping on this device"));m_remember->setObjectName("rememberFileMapping");layout->addWidget(m_remember);
    m_notice=new QLabel;m_notice->setObjectName("mappedFileNotice");m_notice->setTextFormat(Qt::PlainText);m_notice->setWordWrap(true);m_notice->setTextInteractionFlags(Qt::TextSelectableByMouse);layout->addWidget(m_notice);
    auto *buttons=new QDialogButtonBox;
    m_check=buttons->addButton(tr("Preview and verify"),QDialogButtonBox::ActionRole);m_check->setObjectName("previewMappedFile");
    m_open=buttons->addButton(tr("Open mapped original"),QDialogButtonBox::ActionRole);m_open->setObjectName("openMappedFile");
    m_cancel=buttons->addButton(QDialogButtonBox::Cancel);m_cancel->setDefault(true);m_check->setAutoDefault(false);m_open->setAutoDefault(false);layout->addWidget(buttons);
    for(auto *field:{m_root,m_mount,m_workspace})connect(field,&QLineEdit::textChanged,this,[this]{invalidate();});
    connect(m_cancel,&QPushButton::clicked,this,&WslFileOpenDialog::reject);
    connect(m_check,&QPushButton::clicked,this,[this]{submit(false);});connect(m_open,&QPushButton::clicked,this,[this]{submit(true);});
    connect(&m_client,&HgsClient::machinesReady,this,[this](quint64 request,const QJsonObject &data){
        if(request!=m_lookup)return;m_lookup=0;QJsonObject connection;
        for(const auto &value:data["machines"].toArray()){const auto row=value.toObject();if(row["alias"]==m_host)connection=row["connection"].toObject();}
        m_profile=connection["wsl"].toObject();m_machine=data["local"].toString();
        if(m_machine.isEmpty()||connection["transport"]!="wsl-local"||connection["enabled"]==false
            ||m_profile["distribution"].toString().isEmpty()||m_profile["user"].toString().isEmpty()||m_profile["hgs_path"].toString().isEmpty()){
            failure(tr("Choose an enabled, explicitly configured local WSL connection. SSH and default-distribution fallbacks are not allowed."));return;
        }
        m_target->setText(tr("Machine: %1\nDistribution: %2\nLinux user: %3").arg(m_host,m_profile["distribution"].toString(),m_profile["user"].toString()));
        m_key=WslPathMapping::key(m_host,m_machine,m_profile);const auto saved=WslPathMapping::load(m_key);
        m_root->setText(saved["windows_root"].toString());m_mount->setText(saved["mount_root"].toString());m_workspace->setText(saved["workspace_id"].toString());
        invalidate();if(!saved["mapping_id"].toString().isEmpty())m_mappingId=saved["mapping_id"].toString();m_loaded=true;controls();
    });
    connect(&m_client,&HgsClient::machinesFailed,this,[this](quint64 request,const QString &error){if(request==m_lookup){m_lookup=0;failure(error);}});
    connect(&m_client,&HgsClient::wslFileProofReady,this,[this](quint64 request,const QJsonObject &proof){if(request==m_pending){m_pending=0;receive(proof);}});
    connect(&m_client,&HgsClient::wslFileProofFailed,this,[this](quint64 request,const QString &error){if(request==m_pending){m_pending=0;failure(error);}});
    invalidate();m_lookup=m_client.requestMachines();controls();
}
QJsonObject WslFileOpenDialog::selection()const
{
    QJsonObject mapping{{"mapping_id",m_mappingId},{"workspace_id",m_workspace->text()},{"machine_id",m_machine},
        {"windows_environment_id","windows-native"},{"wsl_environment_id","wsl:"+m_profile["distribution"].toString()+':'+m_profile["user"].toString()},
        {"distribution",m_profile["distribution"]},{"user",m_profile["user"]},{"mount_root",m_mount->text()},{"windows_root",m_root->text()}};
    QJsonObject value=m_scope;value["schema_version"]=1;value["request_id"]=m_requestId;
    value["machine_id"]=m_machine;value["connection"]=m_profile;value["mapping"]=mapping;return value;
}
void WslFileOpenDialog::invalidate()
{
    m_token.clear();m_preview={};m_requestId=QUuid::createUuid().toString(QUuid::WithoutBraces);m_mappingId=QUuid::createUuid().toString(QUuid::WithoutBraces);
    m_notice->setText(tr("Preview checks access and contents without opening anything. No original file or downloaded-file marker is modified."));controls();
}
void WslFileOpenDialog::controls()
{
    const bool busy=m_lookup||m_pending||m_localPending;
    for(auto *field:{m_root,m_mount,m_workspace})field->setEnabled(!busy&&m_loaded);
    m_remember->setEnabled(!busy&&m_loaded);m_cancel->setEnabled(!busy);
    m_check->setEnabled(!busy&&m_loaded&&!m_root->text().isEmpty()&&!m_mount->text().isEmpty()&&!m_workspace->text().isEmpty());
    m_open->setEnabled(!busy&&m_loaded&&!m_token.isEmpty());
}
void WslFileOpenDialog::submit(bool opening)
{
    if(m_lookup||m_pending||m_localPending||!m_loaded||(opening&&m_token.isEmpty()))return;
    m_sent=selection();m_opening=opening;QJsonObject request{{"selection",m_sent}};
    if(opening)request["expected_preview"]=m_token;
    m_pending=m_client.requestWslFileProof(m_host,request);
    m_notice->setText(opening?tr("Rechecking the exact conversation, connection and file before opening…"):tr("Reading the file through its original conversation in the selected WSL environment…"));controls();
}
void WslFileOpenDialog::failure(const QString &message){m_token.clear();m_preview={};m_notice->setText(message);controls();}
void WslFileOpenDialog::receive(const QJsonObject &proof)
{
    const auto target=proof["target"].toObject(),mapping=m_sent["mapping"].toObject();
    const auto token=proof["preview_identity"].toString(),digest=proof["sha256"].toString(),path=target["native_path"].toString();
    const auto length=proof["bytes"].toDouble(-1);
    const bool exact=proof["schema_version"].toInt()==1&&proof["selection"].toObject()==m_sent&&selection()==m_sent
        &&proof["alias"]==m_host&&proof["native_changed"].isBool()&&!proof["native_changed"].toBool()
        &&proof["local_verified"].isBool()&&!proof["local_verified"].toBool()
        &&target["schema_version"].toInt()==1&&target["machine_id"]==m_machine&&target["environment_id"]=="windows-native"
        &&target["workspace_id"]==mapping["workspace_id"]&&target["mapping_id"]==mapping["mapping_id"]&&target["path_flavor"]=="windows"
        &&QRegularExpression("^[0-9a-f]{64}$").match(token).hasMatch()&&QRegularExpression("^[0-9a-f]{64}$").match(digest).hasMatch()
        &&length>=1&&length<=10*1024*1024&&double(qint64(length))==length&&MappedLocalFile::safeDrivePath(path);
    if(!exact||(m_opening&&(token!=m_token||proof!=m_preview))){failure(tr("The file proof no longer matches this preview. Nothing was opened."));return;}
    m_localPending=true;controls();m_notice->setText(tr("Verifying the Windows file without following reparse points…"));
    const auto verify=m_verify;
    DownloadCache::async<MappedLocalFile::Result>(this,[verify,path,digest,length]{return verify(path,digest,qint64(length));},
        [this,proof,path](const MappedLocalFile::Result &local){
            m_localPending=false;
            if(!local.ok()||local.path!=path){failure(local.error.isEmpty()?tr("The checked local path did not match."):local.error);return;}
            if(!m_opening){
                m_preview=proof;m_token=proof["preview_identity"].toString();
                m_notice->setText(tr("WSL: %1\nWindows original: %2\nSHA-256: %3\nAccess and contents match. Opening rechecks them; runnable files are shown, not executed.")
                    .arg(proof["native_path"].toString(),path,proof["sha256"].toString()));controls();return;
            }
            // local.guard pins checked path components and contents during this
            // dispatch only. Editing after the handler opens is the owner's act.
            const auto outcome=m_opener(path);
            if(outcome==ExternalFile::Outcome::OpenFailed||outcome==ExternalFile::Outcome::RevealFailed){failure(tr("The mapped file was verified, but the application or file manager could not open it."));return;}
            if(m_remember->isChecked())SettingsJson::store(m_key,QJsonDocument(m_sent["mapping"].toObject()));
            m_outcome=outcome==ExternalFile::Outcome::Revealed?ExternalFile::revealedNotice(path):tr("Opened the verified Windows original: %1").arg(path);
            QDialog::accept();
        });
}
void WslFileOpenDialog::reject(){if(!m_lookup&&!m_pending&&!m_localPending)QDialog::reject();}
