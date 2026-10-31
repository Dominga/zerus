// U62 (#712): Settings pages, appearance, the rail, window layer and previews, from test_sessionswindow.
#include "SessionsWindowFixture.h"

class TestSessionsWindow : public SessionsWindowFixture {
    Q_OBJECT
private slots:
    void updatesEntryOpensExactSettingsPage();
    void fileCacheSettingsNavigation();
    void everySettingsPageShowsTheDarkTheme();
    void automaticApprovalSettingsNavigation_data();
    void automaticApprovalSettingsNavigation();
    void railPinTogglesAlwaysOnTop();
    void windowLayerControlsKeepFocusAndLayout_data();
    void windowLayerControlsKeepFocusAndLayout();
    void compactWorkspaceGeometry();
    void worktreePreview();
    void usageIndicatorSettingUpdatesFooter();
    void attentionNavigationResetsFiltersAndTab();
    void sessionsRailResetsFiltersAndShowsAttention();
    void railButtonsIgnoreNativeLayoutMargins();
    void buttonFocusFollowsKeyboardAndNotClicks();
    void preview();
    void controlConsolePreview();
};

void TestSessionsWindow::updatesEntryOpensExactSettingsPage(){
    SessionsWindow window(program());window.show();window.showUpdates();
    auto *sections=window.findChild<QListWidget *>("settingsSections");QVERIFY(sections);QCOMPARE(sections->currentRow(),4);QCOMPARE(sections->currentItem()->text(),QString("Updates"));
    QVERIFY(window.findChild<QWidget *>("settingsPage")->isVisible());window.showSessionList();window.showUpdates();QCOMPARE(sections->currentRow(),4);
}

void TestSessionsWindow::fileCacheSettingsNavigation(){
    QSettings().setValue("downloads/folder", m_dir->filePath("Zerus Files"));
    SessionsWindow window(program()); window.setFleet(fleet()); window.show(); window.showUpdates();
    auto *sections = window.findChild<QListWidget *>("settingsSections"); QVERIFY(sections);
    QCOMPARE(sections->item(6)->text(), QString("Files"));
    QCOMPARE(sections->item(sections->count()-1)->text(), QString("About"));
    QCOMPARE(sections->item(sections->count()-2)->text(), QString("Extensions"));
    QCOMPARE(sections->item(sections->count()-3)->text(), QString("Private analytics"));
    QCOMPARE(sections->item(sections->count()-4)->text(), QString("Keyboard"));
    QCOMPARE(sections->item(sections->count()-5)->text(), QString("Voice input"));
    sections->setCurrentRow(6);
    auto *files = window.findChild<DownloadCacheSettings *>("downloadCacheSettings"); QVERIFY(files); QVERIFY(files->isVisible());
    QCOMPARE(files->findChild<QComboBox *>("downloadCachePolicy")->currentText(), QString("Never clear"));
    QVERIFY(!files->findChild<QDoubleSpinBox *>("downloadCacheSize")->isEnabled());
    QTRY_VERIFY(files->findChild<QLabel *>("downloadCacheUsage")->text().contains("0 files"));
    if (qEnvironmentVariableIsSet("HGS_TEST_DOWNLOAD_SETTINGS_PREVIEW")) {
        files->findChild<QLineEdit *>("downloadCacheFolder")->setText("/Users/example/Downloads/Zerus Files");
        window.resize(1180, 820); QTest::qWait(80);
        QVERIFY(window.grab().save(qEnvironmentVariable("HGS_TEST_DOWNLOAD_SETTINGS_PREVIEW")));
    }
}

// User report 2026-10-28: Settings > Worktrees showed a light page in the dark
// theme. A page in a scroll area paints its own palette unless the theme's
// transparent rule names it; every page shows the theme's background instead.
void TestSessionsWindow::everySettingsPageShowsTheDarkTheme(){
    QSettings().setValue("workspace/theme", "dark");
    SessionsWindow window(program()); keepDetailsPollsOut(window); window.resize(1280, 900);
    window.setFleet(fleet()); window.show(); QVERIFY(QTest::qWaitForWindowExposed(&window));
    window.findChild<QPushButton *>("workspaceSettings")->click();
    auto *sections = window.findChild<QListWidget *>("settingsSections"); QVERIFY(sections);
    auto *settings = window.findChild<QWidget *>("settingsPage"); QVERIFY(settings);
    const QColor background = settings->grab().toImage().pixelColor(4, 4);
    QVERIFY2(background.lightness() < 80, qPrintable(background.name()));
    for (int row = 0; row < sections->count(); ++row) {
        sections->setCurrentRow(row); QTest::qWait(30);
        auto *stack = settings->findChild<QStackedWidget *>(); QVERIFY(stack);
        auto *scroll = qobject_cast<QScrollArea *>(stack->currentWidget()); QVERIFY(scroll);
        auto *page = scroll->widget(); QVERIFY(page);
        // The bottom right corner of the scroll area is blank on every page;
        // the settings page is grabbed whole, since a child's grab leaves its
        // parent's background transparent.
        const QPoint at = scroll->mapTo(settings, QPoint(scroll->width() - 3, scroll->height() - 3));
        const QColor corner = settings->grab().toImage().pixelColor(at);
        QVERIFY2(qAbs(corner.lightness() - background.lightness()) < 24,
                 qPrintable(QString("%1 (%2): %3 against %4").arg(sections->item(row)->text(), page->objectName(), corner.name(), background.name())));
    }
}

void TestSessionsWindow::automaticApprovalSettingsNavigation_data(){
    QTest::addColumn<QString>("theme");
    QTest::newRow("dark") << QString("dark"); QTest::newRow("light") << QString("light");
}

void TestSessionsWindow::automaticApprovalSettingsNavigation(){
    QFETCH(QString,theme); QSettings().setValue("workspace/theme",theme);
    QFile original(script()); QVERIFY(original.open(QIODevice::ReadOnly));
    const QByteArray journal = R"PY(elif c=='--help':print('hgs approval-log begin|finish|list|detail')
elif c=='approval-log':
 p=json.load(sys.stdin)
 if a[1]=='list':print(json.dumps(dict(entries=[dict(id=i,at=1792075200000+i*1000,host='desktop-a',session='claude/sample/review',prompt='Run this tool?',status=('unknown' if i==5 else 'approved')) for i in range(5,0,-1)],next=None,previous=None)))
 elif a[1]=='detail':print(json.dumps(dict(id=p['id'],status='unknown',question=dict(questions=[dict(id='approval',question='Run this tool?',body='printf fixture',options=[dict(id='yes',label='Yes')])]),answers=[dict(question_id='approval',selected_option_ids=['yes'])],detail='Delivery was not confirmed. Check Activity or Terminal.')))
)PY";
    const auto path=m_dir->filePath("approval-settings-"+theme);
    QVERIFY(FakeCli::install(path,original.readAll().replace("if False:pass\n","if False:pass\n"+journal)));
    SessionsWindow window(FakeCli::program(path)); window.setFleet(fleet()); window.resize(1280,900); window.show(); window.showUpdates();
    auto *sections=window.findChild<QListWidget *>("settingsSections"); QVERIFY(sections);
    const auto approvalSections=sections->findItems(QString("Automatic approvals"),Qt::MatchExactly);
    QCOMPARE(approvalSections.size(),1); sections->setCurrentItem(approvalSections.front());
    auto *page=window.findChild<AutomaticApprovalSettings *>("automaticApprovalSettings"); QVERIFY(page); QVERIFY(page->isVisible());
    QVERIFY(!page->findChild<QCheckBox *>("automaticApprovalsEnabled")->isChecked());
    auto *table=page->findChild<QTableWidget *>("approvalJournal"); QTRY_COMPARE(table->rowCount(),5);
    table->setCurrentCell(0,0); auto *detail=page->findChild<QPlainTextEdit *>("approvalJournalDetail");
    QTRY_VERIFY(detail->toPlainText().contains("printf fixture")); QVERIFY(detail->toPlainText().contains("Selected answer: Yes"));
    if (qEnvironmentVariableIsSet("HGS_AUTOMATIC_APPROVALS_PREVIEW")) {
        QTest::qWait(80); QVERIFY(window.grab().save(qEnvironmentVariable("HGS_AUTOMATIC_APPROVALS_PREVIEW")+"-"+theme+".png"));
    }
}

void TestSessionsWindow::railPinTogglesAlwaysOnTop()
{
    // The pin above Settings switches the same preference as Settings → Appearance.
    const auto above = [](QWidget &window) { return window.windowHandle()->flags().testFlag(Qt::WindowStaysOnTopHint); };
    SessionsWindow window(program()); window.setFleet(fleet()); window.show(); QVERIFY(QTest::qWaitForWindowExposed(&window));
    auto *pin = window.findChild<QPushButton *>("windowPin");
    QVERIFY(pin && pin->isVisible() && pin->isCheckable() && !pin->isChecked());
    const auto unpinnedIcon = pin->icon().pixmap(pin->iconSize()).toImage();
    const auto pinAction = pin->toolTip();
    pin->click();
    QVERIFY(pin->isChecked()); QVERIFY(above(window)); QVERIFY(window.isVisible());
    QVERIFY(pin->icon().pixmap(pin->iconSize()).toImage() != unpinnedIcon);
    QVERIFY(pin->toolTip() != pinAction); QCOMPARE(pin->accessibleName(), pin->toolTip());
    QVERIFY(Settings::value("workspace/alwaysOnTop").toBool());
    auto *option = window.findChild<QCheckBox *>("workspaceAlwaysOnTop"); QVERIFY(option && option->isChecked());
    option->setChecked(false);
    QVERIFY(!pin->isChecked()); QVERIFY(!above(window));
    QCOMPARE(pin->icon().pixmap(pin->iconSize()).toImage(), unpinnedIcon); QCOMPARE(pin->toolTip(), pinAction);
    option->setChecked(true);
    QVERIFY(pin->isChecked()); QVERIFY(above(window));
    auto *theme = window.findChild<QComboBox *>("workspaceTheme"); QVERIFY(theme);
    const int originalTheme = theme->currentIndex();
    theme->setCurrentIndex(1); const auto darkPinned = pin->icon().pixmap(pin->iconSize()).toImage();
    theme->setCurrentIndex(2);
    QVERIFY(pin->isChecked()); QVERIFY(above(window));
    QVERIFY(pin->icon().pixmap(pin->iconSize()).toImage() != darkPinned);
    theme->setCurrentIndex(originalTheme);
    SessionsWindow restored(program()); restored.setFleet(fleet());
    QVERIFY(restored.findChild<QPushButton *>("windowPin")->isChecked());
    pin->click();
    QVERIFY(!option->isChecked()); QVERIFY(!above(window));
    QSettings().remove("workspace/alwaysOnTop");
}

void TestSessionsWindow::windowLayerControlsKeepFocusAndLayout_data()
{
    QTest::addColumn<QString>("controlName");
    QTest::addColumn<bool>("keyboard");
    QTest::newRow("checkbox-keyboard") << QString("workspaceAlwaysOnTop") << true;
    QTest::newRow("checkbox-mouse") << QString("workspaceAlwaysOnTop") << false;
    QTest::newRow("rail-pin-keyboard") << QString("windowPin") << true;
    QTest::newRow("rail-pin-mouse") << QString("windowPin") << false;
}

void TestSessionsWindow::windowLayerControlsKeepFocusAndLayout()
{
    QFETCH(QString, controlName); QFETCH(bool, keyboard);
    SessionsWindow window(program()); window.setFleet(fleet());
    // Give the synthetic window its own title, separate from the live workspace.
    window.setWindowTitle("Zerus window-layer controls " + QUuid::createUuid().toString(QUuid::Id128));
    window.show();
    QVERIFY(QTest::qWaitForWindowExposed(&window)); window.activateWindow();
    QVERIFY(QTest::qWaitForWindowActive(&window));
    window.findChild<QPushButton *>("workspaceSettings")->click();
    auto *layer = window.findChild<WindowLayer *>();
    auto *control = window.findChild<QAbstractButton *>(controlName);
    auto *hint = window.findChild<QLabel *>("workspaceAlwaysOnTopHint");
    QVERIFY(layer && control && hint);
    QTRY_VERIFY(layer->supported() && !layer->busy());
    QTest::qWait(30);
    const auto text = hint->text(); const auto geometry = hint->geometry();
    int pending = 0;
    bool pendingKeptFocus = true, pendingKeptLayout = true;
    connect(layer, &WindowLayer::changed, &window, [&] {
        if (!layer->busy()) return;
        ++pending;
        pendingKeptFocus &= control->isEnabled() && control->hasFocus();
        pendingKeptLayout &= hint->text() == text && hint->geometry() == geometry;
    });
    for (const bool on : {true, false}) {
        control->setFocus(Qt::TabFocusReason); QTRY_VERIFY(control->hasFocus());
        if (keyboard) QTest::keyClick(control, Qt::Key_Space);
        else QTest::mouseClick(control, Qt::LeftButton, Qt::NoModifier, QPoint(8,control->height()/2));
        QVERIFY(control->isEnabled()); QVERIFY(control->hasFocus());
        QCOMPARE(hint->text(), text); QCOMPARE(hint->geometry(), geometry);
        QTRY_VERIFY(!layer->busy() && layer->onTop() == on);
        QTest::qWait(30);
        QVERIFY(control->hasFocus()); QCOMPARE(control->isChecked(), on);
        QCOMPARE(hint->text(), text); QCOMPARE(hint->geometry(), geometry);
    }
    if (QGuiApplication::platformName().startsWith("wayland")) QVERIFY(pending >= 2);
    QVERIFY(pendingKeptFocus); QVERIFY(pendingKeptLayout);
    QSettings().remove("workspace/alwaysOnTop");
}

void TestSessionsWindow::compactWorkspaceGeometry()
{
    SessionsWindow window(program()); window.setFleet(fleet()); window.resize(960, 650); window.show(); QTest::qWait(30);
    window.showSession({}, "codex/hgs/dashboard");
    const auto *sidebar = window.findChild<QWidget *>("sidebar"); QCOMPARE(sidebar->width(), 56);
    for (const auto *button : sidebar->findChildren<QPushButton *>("railButton")) {
        QVERIFY(button->height() >= 44); QCOMPARE(button->iconSize(), QSize(24, 24));
    }
    const auto *search = window.findChild<QLineEdit *>("search"); const auto *list = window.findChild<QListWidget *>("sessionList");
    QVERIFY(qAbs(search->width() - list->width()) < 8);
    const auto *tabs = window.findChild<QTabWidget *>("sessionDetailTabs");
    QVERIFY2(tabs->mapTo(&window, QPoint()).y() < 100, "Normal session header must leave the workspace near the top");
    QVERIFY(tabs->height() > 500);
    QVERIFY(window.findChild<QAction *>("pauseAction")->toolTip().contains("Pause"));
    QVERIFY(window.findChild<QLabel *>("sessionMeta")->toolTip().contains("gpt-6.1-sol"));
    // The header keeps frequent actions; connect command, pause and resume live in its menu.
    auto *more = window.findChild<QPushButton *>("more"); QList<QPushButton *> row;
    for (auto *button : more->parentWidget()->findChildren<QPushButton *>(Qt::FindDirectChildrenOnly))
        if (button->isVisible() && button->property("sessionHeaderAction").toBool() && qAbs(button->y() - more->y()) < 4) row << button;
    std::sort(row.begin(), row.end(), [](const QPushButton *a, const QPushButton *b) { return a->x() < b->x(); });
    QStringList header; for (const auto *button : row) header << button->objectName();
    QCOMPARE(header, (QStringList{"openSessionFolderAction", "openFolderShellAction", "headerCopySessionIdAction", "clearSessionAction", "more"}));
    // The menu also names every header icon.
    emit more->menu()->aboutToShow();
    QStringList menu; for (const auto *action : more->menu()->actions()) if (!action->isSeparator() && action->isVisible()) menu << action->objectName();
    QCOMPARE(menu, (QStringList{"menuOpenSessionFolderAction", "menuOpenFolderShellAction", "menuCopySessionIdAction", "renameSessionAction",
        "forkSessionAction", "openSessionAction", "pauseAction", "menuClearSessionAction", "forgetSessionAction"}));
    const QHash<QString, QString> mirrored{{"openSessionFolderAction", "menuOpenSessionFolderAction"}, {"openFolderShellAction", "menuOpenFolderShellAction"},
        {"headerCopySessionIdAction", "menuCopySessionIdAction"}, {"clearSessionAction", "menuClearSessionAction"}};
    for (const auto *button : row) if (button != more) {
        const auto *action = more->menu()->findChild<QAction *>(mirrored.value(button->objectName()));
        QVERIFY2(action, qPrintable(button->objectName())); QCOMPARE(action->isEnabled(), button->isEnabled());
    }
    QCOMPARE(window.findChild<QAction *>("pauseAction")->text(), QString("Pause session"));
    QVERIFY(!window.findChild<QPushButton *>("restoreSessionAction")->isVisible());
}

void TestSessionsWindow::worktreePreview()
{
    const auto destination=qEnvironmentVariable("HGS_WORKTREE_PREVIEW");if(destination.isEmpty())QSKIP("Set HGS_WORKTREE_PREVIEW to write screenshots");QDir().mkpath(destination);
    const auto original=qApp->palette();
    for(bool dark:{true,false}){
        auto palette=original;palette.setColor(QPalette::Window,QColor(dark?"#161b21":"#f5f7f9"));palette.setColor(QPalette::Base,QColor(dark?"#1c2229":"#ffffff"));palette.setColor(QPalette::Text,QColor(dark?"#e8edf4":"#1a2733"));palette.setColor(QPalette::WindowText,palette.color(QPalette::Text));qApp->setPalette(palette);
        auto state=fleet();auto local=state.local();local.sessions[0].cwd="/repo";local.sessions[0].canonicalCwd="/repo";local.sessions[0].gitRoot="/repo";local.sessions[1].cwd="/linked";local.sessions[1].canonicalCwd="/linked";local.sessions[1].gitRoot="/linked";state.setLocal(local,QDateTime::currentMSecsSinceEpoch());
        SessionOrganization org;const auto project=org.createGroup("Worktree catalog");const auto rootId=org.addFolder(project,"arch","/repo","Core");
        for(const auto &name:{"Website","Backend","Android","Scripts","Docs","Tests"})org.addFolder(project,"arch","/workspace/"+QString(name).toLower(),name);
        org.markImported("arch");org.markImported("mac");setJsonSetting("workspace/organization",QJsonDocument(org.toJson()).toJson(QJsonDocument::Compact));
        SessionsWindow window(program());window.setFleet(state);window.resize(1260,900);window.show();window.showSession({},"codex/hgs/dashboard");
        window.findChild<QPushButton *>("toggleInspector")->setChecked(true);QTest::qWait(100);
        QVERIFY(window.grab().save(destination+(dark?"/details-dark.png":"/details-light.png")));
        openInspectorCard(window,"worktrees");QTest::qWait(100);
        QVERIFY(window.grab().save(destination+(dark?"/worktrees-dark.png":"/worktrees-light.png")));
        window.showProjects();auto *page=window.findChild<ProjectsDialog *>("projectsPage");page->selectProject(project);QTest::qWait(100);
        QVERIFY(window.grab().save(destination+(dark?"/projects-unselected-dark.png":"/projects-unselected-light.png")));
        auto *folders=page->findChild<QTableWidget *>("projectFolders");
        for(int row=0;row<folders->rowCount();++row)if(folders->item(row,0)->data(Qt::UserRole).toString()==rootId)folders->setCurrentCell(row,0);
        QTest::qWait(100);
        QVERIFY(window.grab().save(destination+(dark?"/projects-dark.png":"/projects-light.png")));
        window.resize(1260,760);QTest::qWait(100);QVERIFY(window.grab().save(destination+(dark?"/projects-short-dark.png":"/projects-short-light.png")));
        NewSessionDialog launch(program(),state,{},"codex",project,&window);launch.setGroups(org,project);launch.show();QTest::qWait(100);QVERIFY(launch.grab().save(destination+(dark?"/launch-dark.png":"/launch-light.png")));
        QTimer::singleShot(0,&launch,[&]{auto *form=launch.findChild<QDialog *>("newWorktreeDialog");QVERIFY(form);QTest::qWait(50);QVERIFY(form->grab().save(destination+(dark?"/create-dark.png":"/create-light.png")));form->reject();});launch.findChild<QAction *>("newLaunchWorktree")->trigger();
        QTimer::singleShot(0,&launch,[&]{auto *picker=launch.findChild<QDialog *>("chooseWorktreeDialog");QVERIFY(picker);QTest::qWait(100);QVERIFY(picker->grab().save(destination+(dark?"/picker-dark.png":"/picker-light.png")));picker->reject();});launch.findChild<QAction *>("chooseLaunchWorktree")->trigger();launch.reject();
    }
    qApp->setPalette(original);
}

void TestSessionsWindow::usageIndicatorSettingUpdatesFooter()
{
    QSettings().remove("workspace/usageIndicator");
    SessionsWindow window(script()); window.setFleet(fleet());
    auto *usage = static_cast<AccountUsage::Button *>(window.findChild<QPushButton *>("sessionAccountUsage"));
    auto *option = window.findChild<QComboBox *>("workspaceUsageIndicator"); QVERIFY(option);
    QCOMPARE(option->currentData().toString(), QString("rails")); QCOMPARE(usage->indicator(), AccountUsage::Indicator::Rails);
    option->setCurrentIndex(option->findData("marker-countdown"));
    QCOMPARE(Settings::value("workspace/usageIndicator").toString(), QString("marker-countdown"));
    QCOMPARE(usage->indicator(), AccountUsage::Indicator::MarkerCountdown);
    SessionsWindow restored(script()); restored.setFleet(fleet());
    QCOMPARE(restored.findChild<QComboBox *>("workspaceUsageIndicator")->currentData().toString(), QString("marker-countdown"));
    QCOMPARE(static_cast<AccountUsage::Button *>(restored.findChild<QPushButton *>("sessionAccountUsage"))->indicator(), AccountUsage::Indicator::MarkerCountdown);
    QSettings().remove("workspace/usageIndicator");
}

void TestSessionsWindow::attentionNavigationResetsFiltersAndTab()
{
    SessionsWindow window(program()); window.setFleet(fleet()); window.show();
    auto *search = window.findChild<QLineEdit *>("search"); QVERIFY(search);
    search->setText("does-not-match");
    auto *tabs = window.findChild<QTabWidget *>("sessionDetailTabs"); QVERIFY(tabs);
    tabs->setCurrentIndex(1);
    window.showProjects("mac");
    window.showAttentionSession("mac", "claude/infra/review");
    QCOMPARE(tabs->currentIndex(), 0); QVERIFY(search->text().isEmpty());
    auto *list = window.findChild<QListWidget *>("sessionList"); QVERIFY(list->currentItem());
    QCOMPARE(list->currentItem()->data(Qt::UserRole).toString(), QString("mac\nclaude/infra/review"));
    QVERIFY(list->isVisible());
}

void TestSessionsWindow::sessionsRailResetsFiltersAndShowsAttention()
{
    SessionsWindow window(program()); auto state=fleet(); window.setFleet(state); window.show(); QTest::qWait(70);
    QPushButton *sessions=nullptr;
    for (auto *button:window.findChildren<QPushButton *>()) if (button->property("glyph")=="sessions" && button->objectName()=="railButton") sessions=button;
    QVERIFY(sessions);
    auto *brand=window.findChild<QPushButton *>("brandMark");
    QVERIFY(brand->property("attentionCount").toInt()>0);
    QVERIFY(window.findChild<QLabel *>("railAttentionBadge")->isVisible());
    auto *search=window.findChild<QLineEdit *>("search");search->setText("no matching sessions");
    for (auto *button:window.findChildren<QPushButton *>()) if (button->property("filter")=="active") button->click();
    sessions->click();QVERIFY(search->text().isEmpty());
    for (auto *button:window.findChildren<QPushButton *>()) if (button->property("filter")=="all") QVERIFY(button->isChecked());
    auto box=state.local();for(auto &session:box.sessions){session.phase="idle";session.activity="idle";session.unreadReply=false;}
    state.setLocal(box,QDateTime::currentMSecsSinceEpoch());
    for(const auto &host:state.peerNames()){auto peer=*state.peer(host);for(auto &session:peer.sessions){session.phase="idle";session.activity="idle";session.unreadReply=false;}state.setPeer(peer,QDateTime::currentMSecsSinceEpoch());}
    window.setFleet(state);QCOMPARE(brand->property("attentionCount").toInt(),0);
    QVERIFY(window.findChild<QLabel *>("railAttentionBadge")->isHidden());
}

void TestSessionsWindow::railButtonsIgnoreNativeLayoutMargins()
{
    NativeLayoutMargins style(QStyleFactory::create("Fusion"));
    SessionsWindow window(program()); window.setFleet(fleet()); window.show(); QTest::qWait(50);
    // A window stylesheet would hide the native metric; apply it directly.
    window.setStyleSheet({});
    auto *sidebar = window.findChild<QWidget *>("sidebar"); auto *side = sidebar->layout();
    for (auto *button : sidebar->findChildren<QPushButton *>(Qt::FindDirectChildrenOnly)) button->setStyle(&style);
    side->activate(); QTest::qWait(20);
    QList<QWidget *> rail;
    for (int i = 0; i < side->count(); ++i)
        if (auto *widget = side->itemAt(i)->widget(); widget && widget->isVisible()) rail << widget;
    QVERIFY(rail.size() >= 7);
    for (int i = 1; i < rail.size(); ++i)
        QVERIFY2(rail[i - 1]->geometry().bottom() < rail[i]->geometry().top(),
                 qPrintable(QString("%1 overlaps %2").arg(rail[i - 1]->objectName(), rail[i]->objectName())));
}

void TestSessionsWindow::buttonFocusFollowsKeyboardAndNotClicks()
{
    SessionsWindow window(program()); window.setFleet(fleet()); window.show(); QTest::qWait(50);
    auto *toggle = window.findChild<QPushButton *>("toggleInspector");
    QTest::mouseClick(toggle, Qt::LeftButton);
    QCOMPARE(toggle->property("keyboardFocus").toBool(), false);
    toggle->setFocus(Qt::TabFocusReason); QTest::keyClick(toggle, Qt::Key_Tab);
    auto *focused = qobject_cast<QPushButton *>(QApplication::focusWidget());
    QVERIFY(focused); QVERIFY(focused->property("keyboardFocus").toBool());
    QTest::mouseClick(focused, Qt::LeftButton);
    QVERIFY(!focused->property("keyboardFocus").toBool());
    // Pressing a key on an already focused button restores its keyboard ring.
    QTest::keyClick(focused, Qt::Key_Shift); QVERIFY(!focused->property("keyboardFocus").toBool());
    QTest::keyClick(focused, Qt::Key_Right); QVERIFY(focused->property("keyboardFocus").toBool());

    // Refresh disables itself while connecting. Qt then transfers focus to
    // the next enabled button, even though the user has not pressed Tab.
    QWidget controls; QVBoxLayout layout(&controls);
    QPushButton refresh("Refresh"), details("Details");
    layout.addWidget(&refresh); layout.addWidget(&details);
    connect(&refresh, &QPushButton::clicked, &refresh, [&] { refresh.setEnabled(false); });
    controls.show(); controls.activateWindow(); QTest::qWait(20);
    QTest::mouseClick(&refresh, Qt::LeftButton);
    QVERIFY(details.hasFocus()); QVERIFY(!details.property("keyboardFocus").toBool());
    refresh.setEnabled(true); refresh.setFocus(Qt::MouseFocusReason);
    QTest::keyClick(&refresh, Qt::Key_Space);
    QVERIFY(details.hasFocus()); QVERIFY(details.property("keyboardFocus").toBool());
}

void TestSessionsWindow::preview()
{
    const QString destination = qEnvironmentVariable("HGS_PREVIEW_DIR");
    if (destination.isEmpty()) QSKIP("Set HGS_PREVIEW_DIR to render design previews");
    QDir().mkpath(destination);
    const QPalette original = qApp->palette();
    for (bool dark : {false, true}) {
        QPalette palette = original;
        palette.setColor(QPalette::Window, QColor(dark ? "#161b21" : "#f5f7f9"));
        palette.setColor(QPalette::Base, QColor(dark ? "#1c2229" : "#ffffff"));
        palette.setColor(QPalette::Text, QColor(dark ? "#e8edf4" : "#1a2733"));
        palette.setColor(QPalette::WindowText, palette.color(QPalette::Text)); qApp->setPalette(palette);
        auto previewFleet = fleet(); auto previewBox = previewFleet.local();
        auto &main = previewBox.sessions[0]; main.cwd = "/workspace/zerus-ui"; main.gitRoot = main.cwd;
        main.gitBranch = "feat/zerus"; main.gitWorktree = true; main.gitWorktreeName = "zerus-ui";
        main.activitySummary = "Bash"; main.activityDetail = "ctest --test-dir tray/build";
        main.subagentSource = "hooks"; main.subagentCountsComplete = true; main.subagentActiveCount = 6; main.subagentTotalCount = 14;
        const QStringList projects{"api", "project-docs", "project-docs", "hgs", "sample-project", "website", "api"};
        const QStringList names{"auth-flow", "dashboard", "wheel", "persistence", "research", "content", "search"};
        const QStringList commands{"claude", "codex", "codex", "codex", "kimi", "claude", "kimi"};
        const QStringList branches{"feat/auth", "feat/operations", "fix/wheel", "main", "chore/hooks", "main", "feat/search"};
        const QStringList actionTexts{"Read: Validate token expiry", "Read: Review service configuration", "apply_patch: Add keyboard navigation", "Ready: Waiting for your next message", "ReadFile: Compare deployment patterns", "Ready: Content review finished", "Shell: Check search indexes"};
        for (int i = 0; i < names.size(); ++i) {
            auto row = previewBox.sessions[0]; row.project = projects[i]; row.tag = names[i]; row.cmd = commands[i]; row.name = row.cmd + '/' + row.project + '/' + row.tag;
            row.gitBranch = branches[i]; row.gitWorktree = i == 1 || i == 2; row.gitWorktreeName = i == 1 ? "w2" : "wheel";
            row.cwd = "/workspace/" + (row.gitWorktree ? row.gitWorktreeName : row.project); row.gitRoot = row.cwd;
            row.activitySummary = actionTexts[i]; row.activityDetail.clear(); row.subagentActiveCount = 0; row.subagentTotalCount = 0;
            if (i == 3 || i == 5) { row.activity = "idle"; row.phase = "idle"; row.currentTool.clear(); }
            if (i == 3) { row.conversationId = "preview-reply"; row.replyId = "3:1790928000"; }
            if (i == 4) { row.subagentSource = "hook_profiles"; row.subagentActiveCount = 3; row.subagentTotalCount = 12; }
            if (i == 6) { row.subagentSource = "hook_profiles"; row.subagentTotalCount = -1; row.subagentCountsComplete = false; }
            previewBox.sessions.append(row);
        }
        previewFleet.setLocal(previewBox, QDateTime::currentMSecsSinceEpoch());
        SessionOrganization previewOrg;
        const auto releaseGroup = previewOrg.createGroup("Release / Zerus"), infraGroup = previewOrg.createGroup("Infrastructure"), experimentsGroup = previewOrg.createGroup("Research & experiments");
        previewOrg.setColor(releaseGroup,"#4ba3ff"); previewOrg.setColor(infraGroup,"#ffcc00"); previewOrg.setColor(experimentsGroup,"#bb86fc");
        previewOrg.addFolder(releaseGroup,"arch","/workspace/zerus","Linux checkout");
        previewOrg.addFolder(releaseGroup,"mac","/Users/example/work/hgs","Mac checkout");
        for (const auto &row : previewBox.sessions) {
            const auto id = previewOrg.observe("arch", row.name, "");
            previewOrg.moveSession(id, row.project == "hgs" ? releaseGroup : row.project == "sample-project" ? infraGroup : experimentsGroup);
        }
        previewOrg.moveSession(previewOrg.observe("mac", "claude/infra/review", ""), infraGroup); previewOrg.setCollapsed(infraGroup, true);
        setJsonSetting("workspace/organization", QJsonDocument(previewOrg.toJson()).toJson(QJsonDocument::Compact));
        SessionsWindow window(program()); window.setFleet(previewFleet); window.resize(1240, 800); window.show(); QTest::qWait(100);
        auto *pin = window.findChild<QPushButton *>("windowPin");
        for (bool pinned : {false, true}) {
            pin->setChecked(pinned);
            QTest::mouseMove(&window, QPoint(window.width() - 10, 10)); QTest::qWait(30);
            const auto name = QString("/window-%1-%2").arg(pinned ? "pinned" : "unpinned", dark ? "dark" : "light");
            const auto crop = pin->geometry().adjusted(-7, -8, 7, 55);
            QVERIFY(pin->parentWidget()->grab(crop).save(destination + name + ".png"));
            QTest::mouseMove(pin, pin->rect().center()); QTest::qWait(30);
            QVERIFY(pin->parentWidget()->grab(crop).save(destination + name + "-hover.png"));
        }
        pin->setChecked(false);
        window.showSession({}, "codex/hgs/dashboard"); QTest::qWait(100);
        auto *client = window.findChild<HgsClient *>();
        QJsonObject data{{"tracked", true}, {"conversation_id", "conversation-one"}, {"phase", "tool"}, {"activity", "busy"},
            {"cwd", "/workspace/zerus"}, {"model", "gpt-6.1-sol"}, {"prompt", "Build a clear view of every agent across our machines.\nKeep the interface quiet, with attention where it matters."}, {"cursor", 3},
            {"events", QJsonArray{
                QJsonObject{{"seq", 1}, {"at", 1790927810}, {"type", "UserPromptSubmit"}, {"detail", "Build the session dashboard."}},
                QJsonObject{{"seq", 2}, {"at", 1790927990}, {"type", "PostToolUse"}, {"tool", "apply_patch"}, {"detail", "Updated tray/src/SessionsWindow.cpp"}},
                QJsonObject{{"seq", 3}, {"at", 1790928000}, {"type", "PreToolUse"}, {"tool", "Bash"}, {"detail", "ctest --test-dir tray/build --output-on-failure"}}}}};
        QJsonObject children;
        const QStringList childNames{"UI layout", "Session state", "macOS parity", "Search", "Git metadata", "Accessibility", "Concurrency", "Hooks", "Archive", "Projects", "Packaging", "Tests", "Docs", "Terminal"};
        const QStringList childActions{"Check compact row geometry", "Verify saved context recovery", "Build native menu integration", "Test branch and label filters", "Read linked worktree roots", "Check keyboard focus order", "Review lock ordering", "Inspect activity event contract", "Test archive identity", "Verify remote folder browser", "Check release bundle", "Run focused regression tests", "Update session examples", "Verify attached title changes"};
        for (int i = 0; i < childNames.size(); ++i) children[QString("child-%1").arg(i)] = QJsonObject{{"label", childNames[i]}, {"state", i < 6 ? "working" : "finished"}, {"detail", childActions[i]}, {"updated", 1790928000 - i}};
        data["task_lists"] = QJsonObject{{"main", QJsonObject{{"items", QJsonArray{
            QJsonObject{{"title", "Inspect the workspace and existing hooks"}, {"status", "completed"}},
            QJsonObject{{"title", "Build the collapsible session inspector"}, {"status", "in_progress"}},
            QJsonObject{{"title", "Verify native behavior on Arch and macOS"}, {"status", "pending"}}
        }}}}};
        data["goal"] = QJsonObject{{"id", "preview-goal"}, {"objective", "Complete the migration and verify restore on both machines"},
            {"status", "active"}, {"tokens_used", 24000}, {"token_budget", 100000}, {"time_used_seconds", 154800}};
        data["subagents"] = children; data["subagent_source"] = "hooks"; data["subagent_active_count"] = 6; data["subagent_total_count"] = 14;
        data["cwd"] = "/workspace/zerus-ui"; data["git_root"] = "/workspace/zerus-ui"; data["git_branch"] = "feat/zerus";
        data["git_worktree"] = true; data["git_worktree_name"] = "zerus-ui";
        data["effort"] = "high"; data["settings_change_supported"] = true; data["settings_apply_when"] = "ready";
        data["model_options"] = QJsonArray{
            QJsonObject{{"id", "gpt-6.1-sol"}, {"effort_options", QJsonArray{"low", "medium", "high", "xhigh"}}},
            QJsonObject{{"id", "gpt-6-astra"}, {"effort_options", QJsonArray{"high", "xhigh", "ultra"}}}};
        client->inspectionReady({}, "codex/hgs/dashboard", data); QTest::qWait(50);
        auto *tree = window.findChild<SessionList *>("sessionList");
        for (int i = 0; i < tree->count(); ++i) if (tree->item(i)->data(SessionRoles::HasChildren).toBool()) { tree->childrenToggled(tree->item(i)->data(SessionRoles::Key).toString()); break; }
        QTest::qWait(30);
        auto *usage=static_cast<AccountUsage::Button *>(window.findChild<QPushButton *>("sessionAccountUsage"));
        usage->setData(QJsonObject{{"provider","codex"},{"status","ok"},{"windows",QJsonArray{
            QJsonObject{{"used_percent",78},{"window_minutes",300}},QJsonObject{{"used_percent",94},{"window_minutes",10080}}}}});
        QVERIFY(window.grab().save(destination + (dark ? "/tree-dark.png" : "/tree-light.png")));
        for (int i = 0; i < tree->count(); ++i) if (tree->item(i)->data(SessionRoles::HasChildren).toBool()) { tree->childrenToggled(tree->item(i)->data(SessionRoles::Key).toString()); break; }
        auto *tabs = window.findChild<QTabWidget *>("sessionDetailTabs");
        auto *inspectorToggle = window.findChild<QPushButton *>("toggleInspector");
        inspectorToggle->setChecked(true); QTest::qWait(50);
        QVERIFY(window.grab().save(destination + (dark ? "/swarm-dark.png" : "/swarm-light.png")));
        inspectorToggle->setChecked(false); QTest::qWait(30);
        QVERIFY(window.grab().save(destination + (dark ? "/sessions-dark.png" : "/sessions-light.png")));
        window.showNotificationNotice("Session updated. Refreshing…");
        QVERIFY(window.grab().save(destination + (dark ? "/status-dark.png" : "/status-light.png")));
        window.findChild<MessageComposer *>("messageComposer")->findChild<QPushButton *>("sessionModelSettings")->click(); QTest::qWait(30);
        auto *modelPopup = window.findChild<MessageComposer *>("messageComposer")->findChild<QWidget *>("sessionSettingsPopup");
        QVERIFY(modelPopup->grab().save(destination + (dark ? "/model-settings-dark.png" : "/model-settings-light.png"))); modelPopup->hide();
        auto *sessionList = window.findChild<QListWidget *>("sessionList");
        QTest::keyClick(sessionList, Qt::Key_Menu); QTest::qWait(50);
        auto *contextMenu = window.findChild<QMenu *>("sessionContextMenu");
        QVERIFY(contextMenu->grab().save(destination + (dark ? "/session-menu-dark.png" : "/session-menu-light.png")));
        contextMenu->hide();
        auto *groupsMenu = window.findChild<QMenu *>("sessionProjectsMenu");
        groupsMenu->popup(sessionList->mapToGlobal(QPoint(8, 8))); QTest::qWait(30);
        groupsMenu->setActiveAction(window.findChild<QAction *>("markAllSessionsRead"));
        QVERIFY(groupsMenu->grab().save(destination + (dark ? "/mark-all-read-dark.png" : "/mark-all-read-light.png"))); groupsMenu->hide();
        QTimer::singleShot(0, &window, [&]() {
            auto *dialog = qobject_cast<QDialog *>(QApplication::activeModalWidget()); QVERIFY(dialog);
            QTimer::singleShot(1000, dialog, &QDialog::reject);
            dialog->findChild<QLineEdit *>("renameSessionName")->setText("release planning");
            QTest::qWait(50);
            QVERIFY(dialog->grab().save(destination + (dark ? "/rename-dark.png" : "/rename-light.png")));
            dialog->reject();
        });
        window.findChild<QAction *>("renameSessionAction")->trigger();
        auto *actions = window.findChild<QPushButton *>("batchActions");
        auto *menu = actions->menu(); menu->popup(actions->mapToGlobal(QPoint(0, actions->height())));
        QTest::qWait(50); menu->setActiveAction(menu->actions().first());
        QVERIFY(menu->grab().save(destination + (dark ? "/actions-dark.png" : "/actions-light.png")));
        auto *machineMenu = menu->actions().first()->menu();
        machineMenu->popup(menu->mapToGlobal(menu->actionGeometry(menu->actions().first()).topRight())); QTest::qWait(50);
        QVERIFY(machineMenu->grab().save(destination + (dark ? "/machine-actions-dark.png" : "/machine-actions-light.png")));
        machineMenu->hide(); menu->hide();
        window.resize(960, 650); QTest::qWait(50);
        QVERIFY(window.grab().save(destination + (dark ? "/sessions-dark-compact.png" : "/sessions-light-compact.png")));
        inspectorToggle->setChecked(true); QTest::qWait(50);
        QVERIFY(window.grab().save(destination + (dark ? "/swarm-dark-compact.png" : "/swarm-light-compact.png")));
        inspectorToggle->setChecked(false);
        window.showSession("mac", "claude/infra/review"); QTest::qWait(50);
        QVERIFY(window.grab().save(destination + (dark ? "/attention-dark.png" : "/attention-light.png")));
        auto archiveFleet = fleet(); auto archiveBox = archiveFleet.local();
        auto archived = archiveBox.sessions[0]; archived.name = "codex/website/navigation";
        archived.project = "website"; archived.tag = "navigation"; archived.state = "archived";
        archived.archiveId = "preview-archive"; archived.archivedAt = 1790928000;
        archived.currentTool.clear(); archived.prompt = "Simplify the mobile navigation and document the final design.";
        // The older completed instance has its own identity; no active binding occupies its name.
        archiveBox.sessions.removeAt(2); archiveBox.sessions.append(archived);
        archiveFleet.setLocal(archiveBox, QDateTime::currentMSecsSinceEpoch()); window.setFleet(archiveFleet);
        window.resize(1240, 800);
        for (auto *button : window.findChildren<QPushButton *>()) if (button->property("filter") == "archived") { button->click(); button->setFocus(); }
        QTest::qWait(60);
        auto archiveDetails = data; archiveDetails["prompt"] = archived.prompt; archiveDetails["phase"] = "ended";
        archiveDetails["activity"] = "ended"; archiveDetails["cwd"] = "/workspace/projects/website";
        archiveDetails["last_message"] = "Navigation updated. The mobile menu now preserves focus and the design notes are in place.";
        archiveDetails["completion_reason"] = "clean_exit";
        client->inspectionReady({}, archived.name, archiveDetails, archived.archiveId); QTest::qWait(50);
        QVERIFY(window.grab().save(destination + (dark ? "/archive-dark.png" : "/archive-light.png")));
        window.setFleet(fleet()); window.showSession({}, "codex/hgs/dashboard");
        NewSessionDialog launch(program(), fleet(), "mac", "codex", {}, &window); launch.show(); QTest::qWait(100);
        SessionOrganization launchProjects; const auto launchId=launchProjects.createGroup("Zerus");
        launchProjects.addFolder(launchId,"mac","/Users/example/work/new project"); launchProjects.addFolder(launchId,"arch","/workspace/zerus");
        launch.setGroups(launchProjects,launchId);
        QVERIFY(launch.grab().save(destination + (dark ? "/new-session-dark.png" : "/new-session-light.png"))); launch.close();
        DirectoryDialog browser(program(), "mac", "~", &window); browser.show(); QTest::qWait(100);
        QVERIFY(browser.grab().save(destination + (dark ? "/folders-dark.png" : "/folders-light.png"))); browser.close();
        window.resize(1240, 800); window.showProjects("mac"); QTest::qWait(100);
        QVERIFY(window.grab().save(destination + (dark ? "/projects-dark.png" : "/projects-light.png")));
        // Compare selection, activity, unread and action states at the smallest
        // supported sidebar width, using the real delegate in both densities.
        removeSetting("workspace/organization");
        BoxState states; states.host = "arch"; states.ok = true;
        const QStringList stateNames{"nebula", "infrastructure-rollout-with-a-long-name", "persistence", "deploy", "research", "tests", "navigation", "content", "legacy"};
        for (int i = 0; i < stateNames.size(); ++i) {
            auto row = fleet().local().sessions[0]; row.tag = stateNames[i]; row.name = "codex/hgs/" + row.tag;
            row.gitBranch = "main"; row.activitySummary = "Bash"; row.activityDetail = "Run the deployment checks";
            row.model = "gpt-6-astra"; row.effort = "xhigh";
            if (i == 0) {
                row.turnStarted = QDateTime::currentSecsSinceEpoch() - 2729;
                row.subagentSource = "hooks"; row.subagentCountsComplete = true;
                row.subagentActiveCount = 2; row.subagentTotalCount = 3;
                row.subagents = {{"review", QJsonObject{{"name", "Review"}, {"state", "working"}}}};
            }
            if (i == 1 || i == 2) { row.conversationId = "preview-" + row.tag; row.replyId = "1:1790928000"; }
            if (i == 2 || i >= 6) { row.activity = "idle"; row.phase = "idle"; row.activitySummary.clear(); row.activityDetail.clear(); row.currentTool.clear(); row.prompt.clear(); }
            if (i == 3) { row.phase = "approval"; row.activityDetail = "Approve the deployment command"; }
            if (i == 4) { row.phase = "input"; row.activityDetail = "Choose the target environment"; }
            if (i == 5) { row.phase = "error"; row.activityDetail = "Connection timed out"; }
            if (i == 6) row.state = "paused";
            if (i == 8) { row.tracked = false; row.activity.clear(); row.phase.clear(); }
            states.sessions.append(row);
        }
        FleetState stateFleet; stateFleet.setLocal(states, QDateTime::currentMSecsSinceEpoch());
        SessionsWindow stateWindow(program()); stateWindow.setFleet(stateFleet); stateWindow.resize(1000, 1180); stateWindow.show();
        stateWindow.showSession({}, states.sessions[0].name);
        auto *stateList = stateWindow.findChild<SessionList *>("sessionList");
        auto *railSplitter = qobject_cast<QSplitter *>(stateWindow.findChild<QWidget *>("sessionListSlot")->parentWidget());
        QVERIFY(railSplitter);
        for (const int railWidth : {270, 380}) for (bool compact : {true, false}) {
            railSplitter->setSizes({railWidth, railSplitter->width() - railSplitter->handleWidth() - railWidth});
            stateList->setProperty("compact", compact); stateList->doItemsLayout(); QTest::qWait(60);
            QPixmap rendered(stateList->size() * stateList->devicePixelRatioF()); rendered.setDevicePixelRatio(stateList->devicePixelRatioF());
            rendered.fill(QColor(dark ? "#161b21" : "#f5f7f9")); stateList->render(&rendered);
            QVERIFY(rendered.save(destination + QString("/states-%1-%2%3.png").arg(dark ? "dark" : "light", compact ? "compact" : "comfortable", railWidth == 270 ? "" : "-wide")));
        }
        for (const auto *pageName : {"dashboardPage", "machinesPage", "accountsPage"}) {
            auto *page = stateWindow.findChild<QWidget *>(pageName); QVERIFY(page);
            auto *stack = qobject_cast<QStackedWidget *>(page->parentWidget()); QVERIFY(stack);
            stack->setCurrentWidget(page); QTest::qWait(30);
            QVERIFY(page->grab(QRect(0, 0, page->width(), 90)).save(destination + QString("/header-%1-%2.png").arg(dark ? "dark" : "light", pageName)));
        }
    }
    qApp->setPalette(original);
}

// Screenshots of a control console (hgs.exe on Windows): this computer runs no
// sessions and manages the remote machine "mac".
void TestSessionsWindow::controlConsolePreview()
{
    const QString destination = qEnvironmentVariable("HGS_CONTROL_PREVIEW_DIR");
    if (destination.isEmpty()) QSKIP("Set HGS_CONTROL_PREVIEW_DIR to render control-console screenshots");
    QDir().mkpath(destination);
    QTemporaryDir fixture; QVERIFY(fixture.isValid()); const auto fake = fixture.filePath("hgs");
    QVERIFY(FakeCli::install(fake, "#!/usr/bin/env python3\nimport json,runpy,sys\na=sys.argv[1:]\n"
        "if a[:2]==['machine','ls']:print(json.dumps({'local':'windows-pc','revision':'r1','machines':[{'alias':'mac',"
        "'connection':{'hostname':'mac.example.test','enabled':True},'enabled':True,'source':'local','inherited':False}]}))\n"
        "elif a[:2]==['machine','resolve']:print(json.dumps({'alias':'mac','effective':{'hostname':'mac.example.test','user':'dev','port':'22','identity_files':[]}}))\n"
        "elif 'account' in a[:2]:print(json.dumps({'profiles':[]}))\n"
        "else:\n sys.argv[0]=" + FakeCli::pythonString(script()) + ";runpy.run_path(sys.argv[0],run_name='__main__')\n"));
    const auto now = QDateTime::currentMSecsSinceEpoch();
    auto state = fleet(); auto local = state.local(); const auto sessions = local.sessions;
    local.host = "windows-pc"; local.sessions.clear(); local.localSessions = false;
    local.metrics = QJsonObject{{"platform", "windows"}, {"cpu_count", 16}, {"sampled_at", now / 1000.}};
    state.setLocal(local, now);
    auto mac = *state.peer("mac"); mac.sessions.append(sessions); state.setPeer(mac, now);
    SessionsWindow window(FakeCli::program(fake)); window.resize(1280, 800); window.setFleet(state); window.show(); QTest::qWait(300);
    window.findChild<QPushButton *>("brandMark")->click(); QTest::qWait(300);
    QVERIFY(window.grab().save(destination + "/control-console-overview.png"));
    window.showSession("mac", "codex/hgs/dashboard"); QTest::qWait(500);
    QVERIFY(window.grab().save(destination + "/control-console-session.png"));
    for (auto *button : window.findChildren<QPushButton *>("railButton"))
        if (button->toolTip().startsWith("Machines")) button->click();
    QTest::qWait(1500);
    QVERIFY(window.grab().save(destination + "/control-console-machines.png"));
}

QTEST_MAIN(TestSessionsWindow)
#include "test_sessionswindow_settings2.moc"
