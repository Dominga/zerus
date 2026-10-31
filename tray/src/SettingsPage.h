#pragma once
#include "Settings.h"
#include "PhonePairing.h"
#include "RecoverySettings.h"
#include "ActivityWidth.h"
#include "ContentScale.h"
#include "WorkspaceIcons.h"
#include "UpdatesWidget.h"
#include "RelaySettings.h"
#include "WindowsMobileSettings.h"
#ifdef Q_OS_WIN
#include "WindowsMobileServices.h"
#endif
#include "WorkspaceSettings.h"
#include "MobileDeliverySettings.h"
#include "DownloadCacheSettings.h"
#include "AutomaticApprovalSettings.h"
#include "speech/SpeechSettings.h"
#include "KeyboardSettings.h"
#include "AnalyticsSettings.h"
#include "Extensions.h"
#include "WorktreeSettings.h"
#include <QComboBox>
#include <QCoreApplication>
#include <QDesktopServices>
#include <QListWidget>
#include <QPointer>
#include <QScrollArea>
#include <QSlider>
#include <QStackedWidget>
#include <QDoubleSpinBox>
#include <QFormLayout>
#include <QUrl>
#include <QToolButton>
#include "ProcessSettings.h"
#include "ForkUpdater.h"
#include "ChildProcess.h"
#include "JsonParse.h"
#include "ProcessRunner.h"
#include "QuestionNotificationSettings.h"
#include "UiLanguage.h"
#include "WorkspaceSkin.h"
#include <QHBoxLayout>
#include <QInputDialog>
#include <QLineEdit>
#include <QMessageBox>
#include <QPushButton>
#include <QProcess>
#include <QJsonDocument>
#include <QJsonObject>
#include <QUuid>
#include <QSignalBlocker>
#include <QTimer>
#include <functional>

// U19, U22: settings hgs keeps, not QSettings (`hgs <command> get|set --json`).
// N34 of #499: each call runs in the background, bounded to 8 s, so a slow or
// older hgs never stalls opening Zerus or a click.
inline void hgsSettingCall(QObject *owner, const QString &executable, const QStringList &args, const QByteArray &input,
                           const std::function<void(const QJsonObject &result, const QString &error)> &done)
{
    // Not the owner's child: a page closed while hgs runs would destroy the
    // process among its widgets, and its finish would reach deleted ones. The
    // call ends on its own (8 s at most) and answers only a page still there.
    auto *process = new QProcess;
    auto *timer = new QTimer(process); timer->setSingleShot(true);
    auto over = std::make_shared<bool>(false);
    const QPointer<QObject> alive(owner);
    const auto finish = [process, over, alive, done](const QJsonObject &result, const QString &error) {
        if (*over) return;
        *over = true; process->disconnect(); process->deleteLater();
        if (alive) done(result, error);
    };
    QObject::connect(process, &QProcess::errorOccurred, process, [finish](QProcess::ProcessError error) {
        if (error == QProcess::FailedToStart) finish({}, QCoreApplication::translate("SettingsPage", "hgs did not start"));
    });
    QObject::connect(process, &QProcess::finished, process, [process, finish](int code, QProcess::ExitStatus status) {
        const auto bytes = process->readAllStandardOutput();
        const auto error = QString::fromUtf8(process->readAllStandardError()).trimmed().section(QLatin1Char('\n'), 0, 0).left(300);
        if (status != QProcess::NormalExit || code != 0) {
            finish({}, !error.isEmpty() ? error : QCoreApplication::translate("SettingsPage", "hgs exited with code %1").arg(code));
            return;
        }
        ProcessRunner::post(process, [bytes] { QString error; const auto object = JsonParse::object(bytes, &error); return qMakePair(object, error); },
            [finish, code, error](const QPair<QJsonObject, QString> &parsed) {
                if (parsed.second.isEmpty()) finish(parsed.first, {});
                else finish({}, !error.isEmpty() ? error : QCoreApplication::translate("SettingsPage", "hgs exited with code %1").arg(code));
            });
    });
    QObject::connect(timer, &QTimer::timeout, process, [finish] { finish({}, QCoreApplication::translate("SettingsPage", "hgs did not answer within 8 seconds")); });
    ChildProcess::inheritStdioOnly(*process);
    process->start(executable, args);
    if (!input.isEmpty()) process->write(input);
    process->closeWriteChannel();
    timer->start(8000);
}

// A check box for one of those settings. It is disabled while a call runs,
// shows what hgs reads after every change, and says under itself when a read
// or a change failed. An unavailable hgs keeps the default.
inline void bindHgsSetting(QObject *owner, const QString &executable, QCheckBox *box, QLabel *note,
                           const QString &command, const QString &key, bool fallback)
{
    const auto show = [box](bool value) { const QSignalBlocker blocker(box); box->setChecked(value); };
    show(fallback); note->hide();
    if (executable.isEmpty()) return;
    // The value hgs confirmed last, which the box keeps when a read fails.
    auto confirmed = std::make_shared<bool>(fallback);
    const auto read = [=](const QString &failure) {
        box->setEnabled(false);
        hgsSettingCall(owner, executable, {command, QStringLiteral("get"), QStringLiteral("--json")}, {},
                       [=](const QJsonObject &result, const QString &error) {
            const auto value = result.value(key);
            if (value.isBool()) *confirmed = value.toBool();
            show(*confirmed); box->setEnabled(true);
            const QString text = !failure.isEmpty() ? failure : value.isBool() ? QString()
                : QCoreApplication::translate("SettingsPage", "Could not read this setting from hgs: %1").arg(error.isEmpty() ? QStringLiteral("no value") : error);
            note->setText(text); note->setVisible(!text.isEmpty());
        });
    };
    read({});
    QObject::connect(box, &QCheckBox::toggled, owner, [=](bool on) {
        box->setEnabled(false);
        // C6: these settings are the owner's; the desktop speaks as the person.
        const QJsonObject input{{"request_id", QUuid::createUuid().toString(QUuid::WithoutBraces)}, {key, on}, {"by", QStringLiteral("human")}};
        hgsSettingCall(owner, executable, {command, QStringLiteral("set"), QStringLiteral("--json")}, QJsonDocument(input).toJson(QJsonDocument::Compact),
                       [=](const QJsonObject &result, const QString &error) {
            // Either way the box then shows what hgs reads.
            read(result.value(key).isBool() ? QString()
                 : QCoreApplication::translate("SettingsPage", "Could not change this setting: %1").arg(error.isEmpty() ? QStringLiteral("no answer") : error));
        });
    });
}

class SettingsPage : public QWidget {
public:
    std::function<void()> appearanceChanged;
    std::function<void()> contentScaleChanged;
    std::function<void()> activityWidthChanged;
    std::function<void(bool)> windowLayerChanged;
    std::function<void()> pollingChanged;
    std::function<void()> extensionsChanged;
    explicit SettingsPage(const QString &executable,QWidget *parent=nullptr):QWidget(parent){
        setObjectName("settingsPage");auto *outer=new QVBoxLayout(this);outer->setContentsMargins(24,20,24,20);outer->setSpacing(20);
        auto *title=new QLabel(tr("Settings"));title->setObjectName("heading");outer->addWidget(title);
        auto *body=new QHBoxLayout;body->setSpacing(24);nav=new QListWidget;nav->setObjectName("settingsSections");nav->setFixedWidth(180);nav->addItems({tr("Appearance"),tr("Sessions"),tr("Automatic recovery"),tr("Processes"),tr("Updates"),tr("Mobile connection"),tr("Files"),tr("Automatic approvals"),tr("Voice input"),tr("Keyboard"),tr("Private analytics"),tr("Extensions"),tr("About")});body->addWidget(nav);
        pages=new QStackedWidget;pages->setMinimumWidth(0);body->addWidget(pages,1);outer->addLayout(body,1);
        auto makePage=[&](const QString &heading){auto *content=new QWidget;content->setMaximumWidth(850);auto *layout=new QVBoxLayout(content);layout->setContentsMargins(0,0,12,0);layout->setSpacing(18);auto *h=new QLabel(heading);h->setObjectName("heading");layout->addWidget(h);addPage(content);return layout;};
        auto *appearance=makePage(tr("Appearance"));auto *theme=new QComboBox;theme->setObjectName("workspaceTheme");theme->addItem(tr("Follow system appearance"),"system");theme->addItem(tr("Dark"),"dark");theme->addItem(tr("Light"),"light");theme->setCurrentIndex(qMax(0,theme->findData(Settings::value("workspace/theme","system"))));appearance->addWidget(new QLabel(tr("Theme")));appearance->addWidget(theme);
        appearance->addWidget(new QLabel(tr("Interface language")));
        appearance->addWidget(zerusLanguageChoice());
        auto *languageHint=new QLabel(tr("Restart Zerus to apply the interface language. Agent sessions keep running."));
        languageHint->setWordWrap(true);appearance->addWidget(languageHint);
        auto *translationHint=new QLabel(tr("Russian is a preview. Untranslated text stays in English."));
        translationHint->setWordWrap(true);appearance->addWidget(translationHint);
        if(zerusUiLanguage()=="ru")nav->setFixedWidth(qMax(180,nav->sizeHintForColumn(0)+32));
        auto *usageIndicator=new QComboBox;usageIndicator->setObjectName("workspaceUsageIndicator");
        usageIndicator->addItem(tr("Usage and elapsed time bars"),"rails");usageIndicator->addItem(tr("Pace marker"),"marker");usageIndicator->addItem(tr("Pace marker and reset countdown"),"marker-countdown");
        usageIndicator->setCurrentIndex(qMax(0,usageIndicator->findData(Settings::value("workspace/usageIndicator","rails"))));
        appearance->addWidget(new QLabel(tr("Usage limit indicator")));appearance->addWidget(usageIndicator);
        auto *usageHint=new QLabel(tr("Compares used quota with the elapsed part of each limit window. The bars show usage above elapsed time; the marker shows elapsed time on the usage bar. Colors warn when the current pace would use up the limit before it resets."));usageHint->setWordWrap(true);appearance->addWidget(usageHint);
        connect(usageIndicator,&QComboBox::currentIndexChanged,this,[this,usageIndicator]{Settings::set("workspace/usageIndicator",usageIndicator->currentData());if(appearanceChanged)appearanceChanged();});
        appearance->addWidget(new QLabel(tr("Content scale")));
        auto *scaleRow=new QHBoxLayout;scaleRow->setSpacing(4);auto *scale=new QSlider(Qt::Horizontal);scale->setObjectName("workspaceContentScale");
        // Twentieths keep the slider on 5% steps for both dragging and the keyboard.
        scale->setRange(qRound(ContentScale::Minimum*20),qRound(ContentScale::Maximum*20));scale->setPageStep(5);
        const auto scaleFactor=ContentScale::factor();scale->setValue(qRound(scaleFactor*20));
        // Normalize preferences saved by older versions to the current bounds.
        Settings::set("workspace/contentScale",scaleFactor);
        scale->setAccessibleName(tr("Content scale"));scale->setMaximumWidth(360);
        // A value keeps its widest text's width, so the reset button right after it stays put while dragging.
        auto *scaleValue=new QLabel;scaleValue->setObjectName("workspaceContentScaleValue");scaleValue->setMinimumWidth(scaleValue->fontMetrics().horizontalAdvance("200%")+2);
        scaleRow->addWidget(scale,1);scaleRow->addSpacing(8);scaleRow->addWidget(scaleValue);
        scaleRow->addWidget(resetButton(scale,qRound(ContentScale::Default*20),tr("Reset to %1%").arg(qRound(ContentScale::Default*100)),"resetContentScale"));
        scaleRow->addStretch();appearance->addLayout(scaleRow);
        auto *scaleHint=new QLabel(tr("Adjusts Activity, the message field and Terminal from 75% to 200%. The session list and the side panel keep their size."));scaleHint->setWordWrap(true);appearance->addWidget(scaleHint);
        // Rebuilding a long transcript on every slider step would stall dragging.
        auto *scaleApply=new QTimer(this);scaleApply->setSingleShot(true);scaleApply->setInterval(150);
        connect(scaleApply,&QTimer::timeout,this,[this]{if(contentScaleChanged)contentScaleChanged();});
        auto showScale=[scaleValue](int steps){scaleValue->setText(QStringLiteral("%1%").arg(steps*5));};showScale(scale->value());
        connect(scale,&QSlider::valueChanged,this,[scaleApply,showScale](int steps){Settings::set("workspace/contentScale",steps/20.);showScale(steps);scaleApply->start();});
        appearance->addWidget(new QLabel(tr("Activity width")));
        auto *widthRow=new QHBoxLayout;widthRow->setSpacing(4);activityWidth=new QSlider(Qt::Horizontal);activityWidth->setObjectName("workspaceActivityWidth");
        activityWidth->setRange(ActivityWidth::Minimum,ActivityWidth::Maximum);activityWidth->setSingleStep(10);activityWidth->setPageStep(100);
        activityWidth->setAccessibleName(tr("Maximum Activity width"));activityWidth->setMaximumWidth(360);
        activityWidthValue=new QLabel;activityWidthValue->setObjectName("workspaceActivityWidthValue");activityWidthValue->setMinimumWidth(activityWidthValue->fontMetrics().horizontalAdvance("2400 px")+2);
        widthRow->addWidget(activityWidth,1);widthRow->addSpacing(8);widthRow->addWidget(activityWidthValue);
        widthRow->addWidget(resetButton(activityWidth,ActivityWidth::Default,tr("Reset to %1 px").arg(ActivityWidth::Default),"resetActivityWidth"));
        widthRow->addStretch();appearance->addLayout(widthRow);
        activityFullWidth=new QCheckBox(tr("Full-width Activity"));activityFullWidth->setObjectName("workspaceActivityFullWidth");appearance->addWidget(activityFullWidth);
        auto *widthHint=new QLabel(tr("In a wide window, Activity, questions and the message field stay centered at this width, which grows with content scale. Drag the column edges in Activity to adjust it, or double-click an edge to restore %1 px. Full-width Activity fills the whole pane.").arg(ActivityWidth::Default));widthHint->setWordWrap(true);appearance->addWidget(widthHint);
        syncActivityWidth();
        connect(activityWidth,&QSlider::valueChanged,this,[this](int width){ActivityWidth::setWidth(width);activityWidthValue->setText(tr("%1 px").arg(width));if(activityWidthChanged)activityWidthChanged();});
        connect(activityFullWidth,&QCheckBox::toggled,this,[this](bool full){ActivityWidth::setFullWidth(full);activityWidth->setEnabled(!full);updateResets();if(activityWidthChanged)activityWidthChanged();});
        onTop=new QCheckBox(tr("Keep Zerus above other windows"));onTop->setObjectName("workspaceAlwaysOnTop");onTop->setEnabled(false);appearance->addWidget(onTop);
        onTopHint=new QLabel;onTopHint->setObjectName("workspaceAlwaysOnTopHint");onTopHint->setWordWrap(true);appearance->addWidget(onTopHint);
        connect(onTop,&QCheckBox::toggled,this,[this](bool on){if(windowLayerChanged)windowLayerChanged(on);});
        auto *local=new QLabel(tr("Appearance and session list preferences apply to this Zerus."));local->setWordWrap(true);appearance->addWidget(local);appearance->addStretch();
        // WhiteWh fork: switchable skins (WorkspaceSkin.h). A duplicate is a JSON file
        // the person edits; the workspace reloads it when it is saved. The longer page
        // scrolls instead of squeezing its rows, so rows never move as others change.
        if(auto *page=appearance->parentWidget()){auto policy=page->sizePolicy();policy.setVerticalPolicy(QSizePolicy::Minimum);page->setSizePolicy(policy);}
        auto *skin=new QComboBox;skin->setObjectName("workspaceSkin");
        const auto fillSkins=[skin]{
            const QSignalBlocker blocker(skin);skin->clear();
            for(const auto &value:WorkspaceSkin::available())skin->addItem(value.builtin?value.name:QObject::tr("%1 (custom)").arg(value.name),value.id);
            skin->setCurrentIndex(qMax(0,skin->findData(WorkspaceSkin::currentId())));
        };
        fillSkins();
        connect(WorkspaceSkin::changes(), &QTimer::timeout, this, [this, fillSkins] { fillSkins(); if (appearanceChanged) appearanceChanged(); });
        auto *duplicateSkin=new QPushButton(tr("Duplicate…"));duplicateSkin->setObjectName("duplicateWorkspaceSkin");duplicateSkin->setAutoDefault(false);
        auto *skinFolder=new QPushButton(tr("Open skins folder"));skinFolder->setObjectName("openWorkspaceSkins");skinFolder->setAutoDefault(false);
        auto *skinRow=new QHBoxLayout;skinRow->setSpacing(8);skinRow->addWidget(skin,1);skinRow->addWidget(duplicateSkin);skinRow->addWidget(skinFolder);
        // Right under Theme: the skin changes the whole window, so it leads the page.
        appearance->insertWidget(3,new QLabel(tr("Skin")));appearance->insertLayout(4,skinRow);
        // One short line: a wrapping label would move the rows below as the page width changes.
        auto *skinHint=new QLabel(tr("Colors, window shape and screen layouts. Duplicate a skin to edit it."));
        skinHint->setObjectName("muted");skinHint->setToolTip(tr("A duplicate is a JSON file in the skins folder; saved changes apply at once."));
        appearance->insertWidget(5,skinHint);
        connect(skin,&QComboBox::currentIndexChanged,this,[this,skin]{Settings::set("workspace/skin",skin->currentData());if(appearanceChanged)appearanceChanged();});
        connect(duplicateSkin,&QPushButton::clicked,this,[this,fillSkins]{
            const auto current=WorkspaceSkin::current();bool ok=false;
            const auto name=QInputDialog::getText(this,tr("Duplicate skin"),tr("Name of the copy"),QLineEdit::Normal,tr("%1 copy").arg(current.name),&ok);
            if(!ok)return;
            QString error;const auto id=WorkspaceSkin::duplicate(current,name,&error);
            if(id.isEmpty()){QMessageBox::warning(this,tr("Duplicate skin"),error);return;}
            Settings::set("workspace/skin",id);fillSkins();if(appearanceChanged)appearanceChanged();
        });
        connect(skinFolder,&QPushButton::clicked,this,[]{QDir().mkpath(WorkspaceSkin::directory());QDesktopServices::openUrl(QUrl::fromLocalFile(WorkspaceSkin::directory()));});
        connect(theme,&QComboBox::currentIndexChanged,this,[this,theme]{Settings::set("workspace/theme",theme->currentData());if(appearanceChanged)appearanceChanged();});
        auto *sessions=makePage(tr("Sessions"));
        for(const auto &option:QList<QStringList>{{"workspaceCompact","workspace/compact",tr("Compact session rows"),"false"},{"workspaceExpandSessionsOnHover","workspace/expandSessionsOnHover",tr("Expand the collapsed session list on hover"),"true"},{"workspaceHideEmptyProjects","workspace/hideEmptyProjects",tr("Hide projects without active sessions"),"true"},{"workspaceAnimateActivity","workspace/animateActivity",tr("Animate working sessions"),"true"}}){
            auto *check=new QCheckBox(option[2]);check->setObjectName(option[0]);check->setChecked(Settings::value(option[1],option[3]=="true").toBool());sessions->addWidget(check);connect(check,&QCheckBox::toggled,this,[this,key=option[1]](bool on){Settings::set(key,on);if(appearanceChanged)appearanceChanged();});
        }
        auto *completed=new QCheckBox(tr("Show completed subagents"));completed->setObjectName("workspaceShowCompletedSubagents");completed->setChecked(Settings::value("workspace/showCompletedSubagents",false).toBool());sessions->addWidget(completed);
        auto *completedHint=new QLabel(tr("Include agents whose latest turn has finished in the session list and its counts. Plan & agents always shows all reported agents. Agents with unknown status remain visible."));completedHint->setWordWrap(true);sessions->addWidget(completedHint);
        connect(completed,&QCheckBox::toggled,this,[this](bool on){Settings::set("workspace/showCompletedSubagents",on);if(appearanceChanged)appearanceChanged();});
        auto *optionalQuestions=new QCheckBox(tr("Notify about optional questions"));optionalQuestions->setObjectName("notifyOptionalQuestions");optionalQuestions->setChecked(QuestionNotificationSettings::enabled());sessions->addWidget(optionalQuestions);
        auto *optionalHint=new QLabel(tr("Off by default. Shows a system notification once for each new optional question, which the agent keeps working without. It is withdrawn when the questions are answered or skipped, or the session is marked as read. Questions that need an answer always notify."));optionalHint->setWordWrap(true);sessions->addWidget(optionalHint);
        connect(optionalQuestions,&QCheckBox::toggled,this,[](bool on){QuestionNotificationSettings::setEnabled(on);});
        // U19: read and write the hgs setting; text says what the ask contains.
        auto *subagents=new QCheckBox(tr("Ask before an agent starts subagents"));subagents->setObjectName("workspaceConfirmSubagents");sessions->addWidget(subagents);
        auto *subagentsError=new QLabel;subagentsError->setObjectName("workspaceConfirmSubagentsError");subagentsError->setWordWrap(true);sessions->addWidget(subagentsError);
        bindHgsSetting(this,executable,subagents,subagentsError,QStringLiteral("subagents-confirm"),QStringLiteral("confirm"),true);
        auto *subagentsHint=new QLabel(tr("Zerus asks you before a session starts subagents, with how many, which type and model."));subagentsHint->setWordWrap(true);sessions->addWidget(subagentsHint);
        // U22: block DeepSeek's own session-log upload; on by default.
        auto *upload=new QCheckBox(tr("Block DeepSeek session-log upload"));upload->setObjectName("workspaceSessionUploadBlock");sessions->addWidget(upload);
        auto *uploadError=new QLabel;uploadError->setObjectName("workspaceSessionUploadBlockError");uploadError->setWordWrap(true);sessions->addWidget(uploadError);
        bindHgsSetting(this,executable,upload,uploadError,QStringLiteral("session-upload-block"),QStringLiteral("block"),true);
        auto *uploadHint=new QLabel(tr("On by default. Zerus starts and resumes every DeepSeek session with the official session-log upload off, without editing your dsh configuration. Off restores dsh's own behaviour."));uploadHint->setWordWrap(true);sessions->addWidget(uploadHint);
#ifdef Q_OS_WIN
        // WhiteWh fork: other computers start sessions here over SSH (src/windows_peer.rs).
        auto *residentHost=new QCheckBox(tr("Keep the session host running for other computers"));residentHost->setObjectName("residentSessionHost");
        residentHost->setChecked(WindowsMobileServices::residentHost());sessions->addWidget(residentHost);
        auto *residentHint=new QLabel(tr("Off by default. On: Zerus keeps this computer's session host running, so the Zerus desktop of another computer can start sessions here over SSH. A session host is never started from an SSH session."));
        residentHint->setWordWrap(true);sessions->addWidget(residentHint);
        connect(residentHost,&QCheckBox::toggled,this,[executable](bool on){Settings::set(QStringLiteral("machines/residentHost"),on);if(on)WindowsMobileServices::keepResidentHost(executable);});
#endif
        auto *hint=new QLabel(tr("Closing Zerus keeps agents running. Projects and the arrangement of your workspace are saved on this device."));hint->setWordWrap(true);sessions->addWidget(hint);sessions->addStretch();
        sync=new RecoverySync(executable,this);recovery=new RecoverySettings(sync);recovery->setMaximumWidth(850);addPage(recovery);
        auto *processes=makePage(tr("Processes"));
        auto *enabled=new QCheckBox(tr("Enable Processes tab"));enabled->setObjectName("processesEnabled");enabled->setChecked(ProcessSettings::enabled());processes->addWidget(enabled);
        auto *timings=new QWidget;auto *form=new QFormLayout(timings);form->setContentsMargins(0,0,0,0);form->setFieldGrowthPolicy(QFormLayout::FieldsStayAtSizeHint);timings->setEnabled(enabled->isChecked());processes->addWidget(timings);
        connect(enabled,&QCheckBox::toggled,this,[this,timings](bool on){Settings::set("processes/enabled",on);timings->setEnabled(on);if(pollingChanged)pollingChanged();});
        auto timing=[&](const char *key,const QString &label,int value,double low,double high){
            auto *spin=new QDoubleSpinBox;spin->setObjectName(QLatin1String(key));spin->setRange(low,high);spin->setDecimals(1);spin->setSingleStep(0.5);spin->setSuffix(tr(" s"));spin->setValue(value/1000.);form->addRow(label,spin);
            connect(spin,&QDoubleSpinBox::valueChanged,this,[this,key](double seconds){Settings::set(QStringLiteral("processes/")+QLatin1String(key),seconds);if(pollingChanged)pollingChanged();});
        };
        timing("activeSeconds",tr("Open Processes tab"),ProcessSettings::activeMs(),1,60);
        timing("backgroundSeconds",tr("Background process count"),ProcessSettings::backgroundMs(),5,600);
        timing("unknownSeconds",tr("Unconfirmed process output"),ProcessSettings::unknownMs(),1,600);
        timing("loadingSeconds",tr("Show loading indicator after"),ProcessSettings::loadingMs(),0,5);
        auto *processHint=new QLabel(tr("Off by default. When disabled, the tab is hidden and processes are not polled. Enable it to inspect running shells and background commands, read recorded output and stop selected processes. Only the selected session is inspected; opening Processes refreshes immediately. Output is read only while the tab is visible. Timings apply to this Zerus. Background refresh also controls session inspection while Terminal or Native UI is open; session overview and account limits refresh separately."));processHint->setWordWrap(true);processes->addWidget(processHint);processes->addStretch();
        auto *updates=makePage(tr("Updates"));updates->addWidget(new UpdatesWidget);updates->addStretch();
        // This computer's connector configuration, then the timing every computer and the phone share.
        auto *mobile=new QWidget;mobile->setMaximumWidth(850);auto *mobileLayout=new QVBoxLayout(mobile);mobileLayout->setContentsMargins(0,0,0,0);mobileLayout->setSpacing(28);
        workspaceConnection=new WorkspaceSettings::Panel(executable);mobileLayout->addWidget(workspaceConnection);
        auto *legacyToggle=new QToolButton;legacyToggle->setText(tr("Advanced: legacy connector configuration"));legacyToggle->setCheckable(true);legacyToggle->setToolButtonStyle(Qt::ToolButtonTextBesideIcon);legacyToggle->setArrowType(Qt::RightArrow);legacyToggle->setEnabled(false);mobileLayout->addWidget(legacyToggle,0,Qt::AlignLeft);
        auto *legacy=new QWidget;auto *legacyLayout=new QVBoxLayout(legacy);legacyLayout->setContentsMargins(0,0,0,0);
#ifdef Q_OS_WIN
        legacyLayout->addWidget(new WindowsMobileSettings::Panel);
        // WhiteWh fork: start the connector with Zerus, and a relay on this
        // computer, shown with the connection rather than the legacy settings.
        mobileLayout->insertWidget(mobileLayout->indexOf(legacyToggle), WindowsMobileServices::optionsPanel());
#else
        legacyLayout->addWidget(new RelaySettings::Panel);
#endif
        legacy->setEnabled(false);legacy->hide();mobileLayout->addWidget(legacy);
        connect(legacyToggle,&QToolButton::toggled,this,[legacy,legacyToggle](bool show){legacy->setVisible(show);legacyToggle->setArrowType(show?Qt::DownArrow:Qt::RightArrow);});
        workspaceConnection->protectionChanged=[legacy,legacyToggle](bool encrypted){legacy->setEnabled(!encrypted);legacyToggle->setEnabled(!encrypted);if(encrypted){legacyToggle->setChecked(false);legacy->hide();}};
        auto *phones=new PhonePairing::Panel(executable);phones->setObjectName("settingsPhonesPanel");phonesPanel=phones;mobileLayout->addWidget(phones);
        workspaceConnection->connectionChanged=[phones]{phones->refresh();};
        mobileDelivery=new MobileDeliverySettings(executable);mobileLayout->addWidget(mobileDelivery);mobileLayout->addStretch();addPage(mobile);
        files=new DownloadCacheSettings;addPage(files);
        worktreeSection=pages->count();nav->insertItem(worktreeSection,tr("Worktrees"));worktrees=new WorktreeSettings(executable);addPage(worktrees);
        approvalSection=pages->count();approvals=new AutomaticApprovalSettings(executable);addPage(approvals);
        speechSection=pages->count();speech=new SpeechSettings(executable);speech->setMaximumWidth(850);addPage(speech);
        keyboardSection=pages->count();keyboard=new KeyboardSettings(ShortcutRegistry::instance());keyboard->setMaximumWidth(850);addPage(keyboard);
        analyticsSection=pages->count();analytics=new AnalyticsSettings(executable);addPage(analytics);
        // WhiteWh fork: optional extensions (Extensions.h), each with its own options.
        auto *extensions=makePage(tr("Extensions"));
        auto *extensionsHint=new QLabel(tr("Integrations with optional third-party tools. They are not part of Zerus and stay off until you turn them on."));extensionsHint->setWordWrap(true);extensions->addWidget(extensionsHint);
        for(const auto &extension:Extensions::available()){
            auto *check=new QCheckBox(extension.name);check->setObjectName(QStringLiteral("extension_")+extension.id);check->setChecked(Extensions::enabled(extension.id));extensions->addWidget(check);
            auto *about=new QLabel(extension.description);about->setWordWrap(true);about->setObjectName("muted");extensions->addWidget(about);
            auto *options=new QWidget;auto *optionsLayout=new QVBoxLayout(options);optionsLayout->setContentsMargins(24,0,0,0);optionsLayout->setSpacing(6);options->setEnabled(check->isChecked());
            if(extension.id==QLatin1String("socraticode")){
                auto *nativeQdrant=new QCheckBox(tr("Run SocratiCode on a native Qdrant (no Docker)"));nativeQdrant->setObjectName("socratiCodeNativeQdrant");
                nativeQdrant->setChecked(Settings::value("socraticode/nativeQdrant",false).toBool());optionsLayout->addWidget(nativeQdrant);
                auto *nativeHint=new QLabel(tr("Off by default: SocratiCode keeps each folder's index in its own Qdrant container in Docker. On: Zerus uses a Qdrant program on 127.0.0.1:6333, which it can install and start, with Ollama for embeddings. Folders whose Claude Code settings name a Qdrant always use that one."));
                nativeHint->setWordWrap(true);optionsLayout->addWidget(nativeHint);
                connect(nativeQdrant,&QCheckBox::toggled,this,[](bool on){Settings::set("socraticode/nativeQdrant",on);});
            }
            QPointer<QWidget> kept;if(optionsLayout->count()){extensions->addWidget(options);kept=options;}else delete options;
            connect(check,&QCheckBox::toggled,this,[this,id=extension.id,kept](bool on){Extensions::setEnabled(id,on);if(kept)kept->setEnabled(on);if(extensionsChanged)extensionsChanged();});
        }
        extensions->addStretch();
        aboutSection=pages->count();
        auto *about=makePage(tr("About"));auto *installedVersion=new QLabel(tr("Zerus %1").arg(UpdateController::installedVersion()));installedVersion->setObjectName("aboutZerusVersion");installedVersion->setTextInteractionFlags(Qt::TextSelectableByMouse);about->addWidget(installedVersion);
        cliVersion=new QLabel(tr("hgs CLI version unavailable"));cliVersion->setObjectName("aboutHgsVersion");
        cliVersion->setTextInteractionFlags(Qt::TextSelectableByMouse|Qt::TextSelectableByKeyboard);about->addWidget(cliVersion);
        versionClient=new HgsClient(executable,this);
        connect(versionClient,&HgsClient::versionReady,this,[this](const QString &version){cliVersion->setText(version.isEmpty()?tr("hgs CLI version unavailable"):tr("hgs CLI %1").arg(version));});
        auto *repository=new QPushButton(QStringLiteral("ufna/zerus"));repository->setObjectName("aboutRepository");repository->setProperty("glyph","github");
        repository->setIcon(workspaceIcon("github",palette().color(QPalette::WindowText)));repository->setIconSize(QSize(20,20));repository->setAutoDefault(false);
        repository->setCursor(Qt::PointingHandCursor);repository->setToolTip(QStringLiteral("https://github.com/ufna/zerus"));repository->setAccessibleName(tr("Open Zerus repository on GitHub"));
        connect(repository,&QPushButton::clicked,this,[]{QDesktopServices::openUrl(QUrl(QStringLiteral("https://github.com/ufna/zerus")));});
        // WhiteWh fork: this build is the fork and its releases are the update source.
        auto *fork=new QPushButton(QStringLiteral("WhiteWh/zerus"));fork->setObjectName("aboutForkRepository");fork->setProperty("glyph","github");
        fork->setIcon(workspaceIcon("github",palette().color(QPalette::WindowText)));fork->setIconSize(QSize(20,20));fork->setAutoDefault(false);fork->setCheckable(true);fork->setChecked(true);
        fork->setCursor(Qt::PointingHandCursor);fork->setToolTip(tr("Active: this build is the WhiteWh fork · https://github.com/WhiteWh/zerus"));fork->setAccessibleName(tr("Open the WhiteWh fork on GitHub (the active repository)"));
        connect(fork,&QPushButton::clicked,this,[fork]{fork->setChecked(true);QDesktopServices::openUrl(QUrl(QStringLiteral("https://github.com/WhiteWh/zerus")));});
        auto *repositories=new QHBoxLayout;repositories->setSpacing(8);repositories->addWidget(repository);repositories->addWidget(fork);repositories->addStretch();
        about->addLayout(repositories);
        auto *updater=ForkUpdater::instance();
        forkStatus=new QLabel;forkStatus->setObjectName("aboutForkStatus");forkStatus->setWordWrap(true);
        forkUpdate=new QPushButton;forkUpdate->setObjectName("aboutForkUpdate");forkUpdate->setAutoDefault(false);
        connect(forkUpdate,&QPushButton::clicked,this,[this,updater]{
            if(updater->ready()){updater->startInstall();return;}
            // #511 N10: a build without a fork number is replaced only once confirmed.
            if(const qint64 build=updater->offeredBuild();build>0){
                if(confirmForkReplacement(build,updater->runningCommit(),updater->offeredCommit()))updater->confirmReplacement();
                return;
            }
            updater->checkNow();
        });
        auto *updateRow=new QHBoxLayout;updateRow->setSpacing(10);updateRow->addWidget(forkUpdate);updateRow->addWidget(forkStatus,1);
        about->addLayout(updateRow);
        connect(updater,&ForkUpdater::changed,this,[this]{refreshForkUpdate();});
        refreshForkUpdate();
        about->addStretch();
        connect(nav,&QListWidget::currentRowChanged,pages,&QStackedWidget::setCurrentIndex);nav->setCurrentRow(0);
        connect(nav,&QListWidget::currentRowChanged,this,[this](int row){if(row==worktreeSection)worktrees->refresh();if(row==aboutSection)refreshCliVersion();if(row==speechSection)speech->refresh();if(row==6)files->refresh();if(row==approvalSection)approvals->refresh();if(row==analyticsSection)analytics->refresh();});
    }
    // WhiteWh fork: the About page's update from the fork's releases.
    void refreshForkUpdate(){
        auto *updater=ForkUpdater::instance();
        const bool ready=updater->ready();
        const qint64 offered=updater->offeredBuild();
        forkUpdate->setText(ready?tr("Restart to install fork build %1").arg(updater->readyBuild())
                            :offered>0?tr("Replace this build with fork build %1…").arg(offered):tr("Check for updates from WhiteWh/zerus"));
        forkUpdate->setEnabled(updater->manualAvailable()&&!updater->installing());
        forkUpdate->setToolTip(updater->installing()?tr("The fork update is being installed"):updater->manualAvailable()?QString():tr("Updates from the fork run in its portable Windows build or a per-user Linux source installation"));
        forkStatus->setText(updater->status());
    }
    // The running build has no fork number: the feed's build is not known to be newer.
    std::function<bool(qint64,const QString&,const QString&)> confirmForkReplacement=[this](qint64 build,const QString &running,const QString &offered){
        QMessageBox box(QMessageBox::Warning,tr("Replace this build"),
            tr("This build has no fork build number, so Zerus cannot tell whether fork build %1 is newer.").arg(build),
            QMessageBox::NoButton,this);
        box.setInformativeText(tr("This build: commit %1.\nFork build %2: commit %3.\n\nFork build %2 downloads now and replaces this build when Zerus restarts; this build stays as the rollback copy.")
            .arg(running.isEmpty()?tr("unknown"):running,QString::number(build),offered));
        auto *replace=box.addButton(tr("Replace with build %1").arg(build),QMessageBox::DestructiveRole);
        box.setDefaultButton(box.addButton(QMessageBox::Cancel));
        box.exec();
        return box.clickedButton()==replace;
    };
    void setPeers(const QStringList &peers){sync->setPeers(peers);speech->setPeers(peers);analytics->setPeers(peers);worktrees->setPeers(peers);}
    // A control-only computer keeps no recovery policy of its own.
    void setLocalSessions(bool enabled,bool offered=false){sync->setLocalSessions(enabled);sync->setLocalSessionsOffered(offered);}
    // Workspace enrollment is available before a local gateway starts.
    void setMobileGateway(bool gateway){
        Q_UNUSED(gateway); // Enrollment is available before a gateway starts.
        nav->item(5)->setHidden(false);
    }
    void openUpdates(){nav->setCurrentRow(4);}
    // WhiteWh fork: a section a feature adds (agent teams), placed after Automatic approvals
    // so the upstream sections keep their order.
    int addSection(const QString &name,QWidget *page){
        const int at=speechSection;nav->insertItem(at,name);
        if(page->objectName().isEmpty())page->setObjectName("settingsContent");
        auto *scroll=new QScrollArea;scroll->setFrameShape(QFrame::NoFrame);scroll->setWidgetResizable(true);scroll->setWidget(page);scroll->setMinimumWidth(0);
        page->setAutoFillBackground(false);scroll->viewport()->setAutoFillBackground(false);   // as addPage
        pages->insertWidget(at,scroll);
        for(int *later:{&speechSection,&keyboardSection,&analyticsSection,&aboutSection})if(*later>=at)++*later;
        return at;
    }
    void openSection(int row){nav->setCurrentRow(row);}
    void openAbout(){nav->setCurrentRow(aboutSection);}
    void openKeyboard(){nav->setCurrentRow(keyboardSection);}
    void openRecovery(){nav->setCurrentRow(2);sync->refresh();}
    void setWindowLayerState(bool on,bool enabled,const QString &hint){const QSignalBlocker block(onTop);onTop->setChecked(on);onTop->setEnabled(enabled);onTopHint->setText(hint);}
    void refresh(){sync->refresh();mobileDelivery->refresh();if(nav->currentRow()==5)workspaceConnection->refresh();if(nav->currentRow()==aboutSection)refreshCliVersion();if(nav->currentRow()==speechSection)speech->refresh();if(nav->currentRow()==analyticsSection)analytics->refresh();}
    void setSwarmSnapshot(const QJsonObject &snapshot){mobileDelivery->setSwarmSnapshot(snapshot);}
    // Activity's column edges and menu change the same preference.
    void syncActivityWidth(){
        const QSignalBlocker width(activityWidth),full(activityFullWidth);
        activityWidth->setValue(ActivityWidth::width());activityWidthValue->setText(tr("%1 px").arg(activityWidth->value()));
        activityFullWidth->setChecked(ActivityWidth::fullWidth());activityWidth->setEnabled(!activityFullWidth->isChecked());
        updateResets();
    }
    // #650 N1: the Phones panel of Mobile connection draws its own colours,
    // so it takes the window's theme here, and again on each change.
    void setDark(bool dark){if(phonesPanel)phonesPanel->setTheme(dark);}
    // Keyboard zoom changes the content scale; the window applies it itself.
    void syncContentScale(){
        auto *scale=findChild<QSlider *>("workspaceContentScale");if(!scale)return;
        {const QSignalBlocker block(scale);scale->setValue(qRound(ContentScale::factor()*20));}
        if(auto *value=findChild<QLabel *>("workspaceContentScaleValue"))value->setText(QStringLiteral("%1%").arg(scale->value()*5));
        updateResets();
    }
private:
    void refreshCliVersion(){cliVersion->setText(tr("hgs CLI: checking…"));versionClient->requestVersion();}
    HgsClient *versionClient;QLabel *cliVersion;MobileDeliverySettings *mobileDelivery;
    QLabel *forkStatus=nullptr;QPushButton *forkUpdate=nullptr;   // WhiteWh fork: About
    WorkspaceSettings::Panel *workspaceConnection;
    PhonePairing::Panel *phonesPanel=nullptr;
    SpeechSettings *speech;
    DownloadCacheSettings *files;
    AutomaticApprovalSettings *approvals;
    KeyboardSettings *keyboard;
    AnalyticsSettings *analytics;
    WorktreeSettings *worktrees;
    int worktreeSection=-1;
    int speechSection=-1,keyboardSection=-1,aboutSection=-1,approvalSection=-1,analyticsSection=-1;
    // An icon button restoring a slider's default, shown only after a change.
    // It keeps its space while hidden, so the rows below do not move. Focus
    // moves to the slider first: a focused button that hides itself would
    // pass focus onward.
    QPushButton *resetButton(QSlider *slider,int value,const QString &caption,const QString &name){
        auto *button=new QPushButton;button->setObjectName(name);button->setProperty("glyph","reset");button->setProperty("settingReset",true);
        button->setToolTip(caption);button->setAccessibleName(caption);button->setAutoDefault(false);
        button->setFixedSize(26,26);button->setIconSize(QSize(16,16));
        button->setStyleSheet("QPushButton { padding:0; min-width:24px; max-width:24px; min-height:24px; max-height:24px; }");
        button->setIcon(workspaceIcon("reset",palette().color(QPalette::WindowText)));
        auto policy=button->sizePolicy();policy.setRetainSizeWhenHidden(true);button->setSizePolicy(policy);button->hide();
        connect(button,&QPushButton::clicked,this,[button,slider,value]{if(button->hasFocus())slider->setFocus(Qt::OtherFocusReason);slider->setValue(value);});
        connect(slider,&QSlider::valueChanged,this,[this]{updateResets();});
        resets.append({button,slider,value});return button;
    }
    void updateResets(){for(const auto &reset:resets)reset.button->setVisible(reset.slider->isEnabled()&&reset.slider->value()!=reset.value);}
    struct Reset{QPushButton *button;QSlider *slider;int value;};QList<Reset> resets;
    // QScrollArea::setWidget turns the page's background fill on, which paints
    // the system palette (light on Windows) under a page whose own name the
    // theme's transparent rule does not list (Worktrees, user report
    // 2026-10-28): every page shows the theme's background instead.
    void addPage(QWidget *page){if(page->objectName().isEmpty())page->setObjectName("settingsContent");auto *scroll=new QScrollArea;scroll->setFrameShape(QFrame::NoFrame);scroll->setWidgetResizable(true);scroll->setWidget(page);page->setAutoFillBackground(false);scroll->viewport()->setAutoFillBackground(false);scroll->setMinimumWidth(0);pages->addWidget(scroll);}
    QListWidget *nav;QStackedWidget *pages;RecoverySync *sync;RecoverySettings *recovery;QCheckBox *onTop;QLabel *onTopHint;QSlider *activityWidth;QLabel *activityWidthValue;QCheckBox *activityFullWidth;
};
