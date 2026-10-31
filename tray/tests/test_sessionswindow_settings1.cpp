// U62 (#712): Settings pages, appearance, the rail, window layer and previews, from test_sessionswindow.
#include "SessionsWindowFixture.h"

class TestSessionsWindow : public SessionsWindowFixture {
    Q_OBJECT
private slots:
    void nativeSettingsNavigationOnExplicitSpeechHost();
    void colorPickerUsesWorkspaceThemeAndCancelsOutside_data();
    void colorPickerUsesWorkspaceThemeAndCancelsOutside();
    void whiteWhWorkspaceView();
    void compactSkinStartsWithACollapsedList();
    void socratiCodeIsAnOptionalExtension();
    void readmeScreenshots();
    void contentScaleLeavesWorkspaceChrome();
    void settingsSlidersResetToDefaults();
    void followSystemAppearanceFollowsARuntimePaletteChange();
    void alwaysOnTopSettingKeepsWindowAbove();
    void dashboardAndMultiMachineNavigation();
    void hiddenPagesRenderWhenShown();
    void pollsKeepUnchangedActivity();
    void dashboardActivityRowsStayReadable_data();
    void dashboardActivityRowsStayReadable();
    void tasksPreview();
};

void TestSessionsWindow::nativeSettingsNavigationOnExplicitSpeechHost()
{
    const auto executable=qEnvironmentVariable("ZERUS_SPEECH_SETTINGS_HGS");
    const auto host=qEnvironmentVariable("ZERUS_SPEECH_SETTINGS_HOST");
    const auto expected=qEnvironmentVariable("ZERUS_SPEECH_SETTINGS_PROFILE");
    if(executable.isEmpty() || expected.isEmpty()) QSKIP("Requires an explicit installed CLI and speech profile; settings requests are read-only.");
    QSettings().setValue("speech/host",host); QSettings().setValue("speech/profile",expected);
    SettingsPage page(executable); page.setPeers({host}); page.show();
    auto *sections=page.findChild<QListWidget *>("settingsSections"); QVERIFY(sections);
    QCOMPARE(sections->item(sections->count()-1)->text(),"About");
    QCOMPARE(sections->item(sections->count()-2)->text(),"Extensions");
    QCOMPARE(sections->item(sections->count()-3)->text(),"Private analytics");
    QCOMPARE(sections->item(sections->count()-4)->text(),"Keyboard");
    const int voice=sections->count()-5; QCOMPARE(sections->item(voice)->text(),"Voice input");
    sections->setCurrentRow(sections->count()-1);
    auto *version=page.findChild<QLabel *>("aboutHgsVersion"); QVERIFY(version);
    QTRY_VERIFY_WITH_TIMEOUT(version->text().startsWith("hgs CLI 1."),10000);
    auto *provider=page.findChild<QComboBox *>("speechProfile"); QVERIFY(provider);
    auto *refresh=page.findChild<QPushButton *>("speechRefresh"); QVERIFY(refresh);
    auto *status=page.findChild<QLabel *>("speechSettingsStatus"); QVERIFY(status);
    sections->setCurrentRow(voice);
    QTRY_VERIFY_WITH_TIMEOUT(refresh->isEnabled(),65000);
    QVERIFY2(provider->isEnabled(),qPrintable(status->text())); QCOMPARE(provider->currentData().toString(),expected);
    page.hide(); page.show(); page.refresh();
    QVERIFY(!refresh->isEnabled());
    QTRY_VERIFY_WITH_TIMEOUT(refresh->isEnabled(),65000);
    QVERIFY2(provider->isEnabled(),qPrintable(status->text())); QCOMPARE(provider->currentData().toString(),expected);
    sections->setCurrentRow(sections->count()-1);
    QTRY_VERIFY_WITH_TIMEOUT(version->text().startsWith("hgs CLI 1."),10000);
}

void TestSessionsWindow::colorPickerUsesWorkspaceThemeAndCancelsOutside_data()
{
    QTest::addColumn<QString>("theme");
    QTest::newRow("dark")<<QString("dark");QTest::newRow("light")<<QString("light");
}

void TestSessionsWindow::colorPickerUsesWorkspaceThemeAndCancelsOutside()
{
    QFETCH(QString,theme);QSettings().setValue("workspace/theme",theme);
    SessionsWindow window(program());window.setFleet(fleet());window.resize(1100,760);window.show();QVERIFY(QTest::qWaitForWindowExposed(&window));
    WorkspaceColorDialog picker(QColor("#5c9abf"),&window);picker.setWindowTitle("Color (synthetic preview)");
    bool expired=false,handled=false;
    QTimer::singleShot(5000,&picker,[&]{expired=true;picker.reject();});
    QTimer::singleShot(0,&picker,[&]{
        QCOMPARE(picker.palette().color(QPalette::Window),QColor(theme=="dark"?"#161b21":"#f5f7f9"));
        auto *buttons=picker.findChild<QDialogButtonBox *>();QVERIFY(buttons);
        auto *cancel=buttons->button(QDialogButtonBox::Cancel);QVERIFY(cancel);
        QTRY_VERIFY(cancel->geometry().left()>buttons->button(QDialogButtonBox::Ok)->geometry().right());
        const auto previews=qEnvironmentVariable("HGS_COLOR_PICKER_PREVIEW");
        if(!previews.isEmpty()){QDir().mkpath(previews);QVERIFY(picker.grab().save(previews+"/color-"+theme+".png"));}
        const QPoint point(window.width()-8,window.height()-8);QVERIFY(!picker.frameGeometry().contains(window.mapToGlobal(point)));
        QVERIFY(PopupMouse::click(&window,Qt::LeftButton,point));
        QTRY_VERIFY(!picker.isVisible());handled=true;
    });
    QCOMPARE(picker.exec(),int(QDialog::Rejected));QVERIFY(handled);QVERIFY(!expired);QVERIFY(!picker.selectedColor().isValid());
}

void TestSessionsWindow::whiteWhWorkspaceView()
{
    QSettings().setValue("workspace/skin", "whitewh"); QSettings().setValue("workspace/theme", "dark"); QSettings().setValue("extensions/socraticode", true);
    const auto restore = qScopeGuard([] {
        QSettings().remove("workspace/skin"); QSettings().remove("workspace/theme"); QSettings().remove("extensions/socraticode"); sessionCardTileInset() = 0;
    });
    const auto state = fleet();
    SessionsWindow window(FakeCli::program(script())); window.resize(1280, 760); window.setFleet(state); window.show();
    window.showSession({}, state.local().sessions[0].name);
    auto *toggle = window.findChild<QPushButton *>("railSessionsToggle"); auto *railNew = window.findChild<QPushButton *>("newSession");
    QVERIFY(toggle); QVERIFY(railNew);
    QTRY_VERIFY(toggle->isVisible()); QVERIFY(!railNew->isVisible()); QVERIFY(!window.findChild<QPushButton *>("sessionPanelToggle")->isVisible());
    QCOMPARE(sessionCardTileInset(), 38);
    auto *composer = window.findChild<MessageComposer *>("messageComposer");
    QVERIFY(composer->findChild<QFrame *>("composerCard")); QVERIFY(composer->findChild<QFrame *>("composerCard")->isVisible());
    auto *socratic = window.findChild<QWidget *>("socratiCodePanel"); QVERIFY(socratic);
    QCOMPARE(qobject_cast<QBoxLayout *>(socratic->parentWidget()->layout())->indexOf(socratic), 0);
    // The compact composer shows the model, so the header does not repeat it.
    QVERIFY(!window.findChild<QLabel *>("sessionModel")->isVisible());
#ifdef Q_OS_WIN
    // The skin's own title bar replaces the system one; the window keeps its width.
    QTRY_VERIFY(window.findChild<QWidget *>("windowTitleBar")->isVisible()); QVERIFY(window.windowFlags() & Qt::FramelessWindowHint);
    // The rail's controls move into the title bar, and the version into the status bar.
    QVERIFY(!window.findChild<QWidget *>("sidebar")->isVisible());
    QVERIFY(window.findChild<QWidget *>("windowTitleBar")->isAncestorOf(toggle));
    QVERIFY(window.findChild<QWidget *>("windowTitleBar")->isAncestorOf(window.findChild<QWidget *>("brandMark")));
    QVERIFY(window.findChild<QWidget *>("workspaceStatus")->isAncestorOf(window.findChild<QWidget *>("appVersion")));
#endif
    auto *listPanel = window.findChild<QWidget *>("sessionListPanel"); QVERIFY(listPanel);
    toggle->click(); QTRY_VERIFY(railNew->isVisible());
    // Collapsed, the cards list leaves no strip: only the rail remains.
    QTRY_VERIFY(!listPanel->isVisible());
    toggle->click(); QTRY_VERIFY(!railNew->isVisible()); QTRY_VERIFY(listPanel->isVisible());
    // Other pages have no session list, so no list toggle.
    window.findChild<QPushButton *>("workspaceSettings")->click(); QTRY_VERIFY(!toggle->isVisible());
    window.findChild<QPushButton *>("railButton")->click(); QTRY_VERIFY(toggle->isVisible());
    const auto preview = qEnvironmentVariable("HGS_WHITEWH_PREVIEW");
    if (!preview.isEmpty()) {
        QDir().mkpath(preview);
        auto *inspector = window.findChild<QWidget *>("sessionInspector");
        if (!inspector->isVisible()) window.findChild<QPushButton *>("toggleInspector")->click();
        QTRY_VERIFY(inspector->isVisible());
        QTest::qWait(250);
        QVERIFY(window.grab().save(preview + QString("/workspace-0.png")));
        // Settings → Appearance, where the skin is chosen.
        window.findChild<QPushButton *>("workspaceSettings")->click(); QTest::qWait(300);
        QVERIFY(window.grab().save(preview + "/settings.png"));
        window.findChild<QPushButton *>("railButton")->click(); QTest::qWait(100);
    }
    QSettings().remove("workspace/skin"); window.applySkin();
    QVERIFY(!window.findChild<QWidget *>("windowTitleBar")->isVisible()); QVERIFY(!(window.windowFlags() & Qt::FramelessWindowHint));
    QVERIFY(window.findChild<QWidget *>("sidebar")->isVisible()); QVERIFY(window.findChild<QWidget *>("sidebar")->isAncestorOf(railNew));
    QVERIFY(window.findChild<QWidget *>("sidebar")->isAncestorOf(window.findChild<QWidget *>("appVersion")));
    QVERIFY(window.width() < 2000);
    QTRY_VERIFY(!toggle->isVisible()); QVERIFY(railNew->isVisible()); QCOMPARE(sessionCardTileInset(), 0);
    QVERIFY(!composer->findChild<QFrame *>("composerCard")->isVisible());
}

// WhiteWh fork: a list collapsed when Zerus quit restores while the window is
// still being built, before its pages and status bar exist.
void TestSessionsWindow::compactSkinStartsWithACollapsedList()
{
    QSettings().setValue("workspace/skin", "whitewh"); QSettings().setValue("workspace/sessionsCollapsed", true);
    const auto restore = qScopeGuard([] { QSettings().remove("workspace/skin"); QSettings().remove("workspace/sessionsCollapsed"); sessionCardTileInset() = 0; });
    const auto state = fleet();
    SessionsWindow window(FakeCli::program(script())); window.resize(1280, 760); window.setFleet(state); window.show();
    auto *listPanel = window.findChild<QWidget *>("sessionListPanel"); QVERIFY(listPanel);
    QTRY_VERIFY(!listPanel->isVisible());
    auto *toggle = window.findChild<QPushButton *>("railSessionsToggle"); QVERIFY(toggle);
    QTRY_VERIFY(toggle->isVisible());
#ifdef Q_OS_WIN
    QVERIFY(window.findChild<QWidget *>("windowTitleBar")->isAncestorOf(toggle));
    QVERIFY(window.findChild<QWidget *>("workspaceStatus")->isAncestorOf(window.findChild<QWidget *>("appVersion")));
#endif
    toggle->click(); QTRY_VERIFY(listPanel->isVisible());
}

// WhiteWh fork: SocratiCode is an optional extension. Off, Details has no
// SocratiCode strip; Settings → Extensions turns it on and off at once.
void TestSessionsWindow::socratiCodeIsAnOptionalExtension()
{
    QSettings().setValue("extensions/socraticode", false);
    const auto restore = qScopeGuard([] { QSettings().remove("extensions/socraticode"); });
    auto state = fleet(); auto box = state.local(); box.sessions[0].cwd = "/repo"; state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(script())); window.resize(1120, 680); window.setFleet(state); window.show();
    window.showSession({}, box.sessions[0].name);
    auto *socratic = window.findChild<QWidget *>("socratiCodePanel"); QVERIFY(socratic);
    auto *check = window.findChild<QCheckBox *>("extension_socraticode"); QVERIFY(check); QVERIFY(!check->isChecked());
    QTest::qWait(100); QVERIFY(socratic->isHidden());
    auto *native = window.findChild<QCheckBox *>("socratiCodeNativeQdrant"); QVERIFY(native); QVERIFY(!native->isEnabledTo(native->window()));
    check->click(); QVERIFY(Extensions::enabled("socraticode")); QTRY_VERIFY(!socratic->isHidden()); QVERIFY(native->isEnabledTo(native->window()));
    check->click(); QVERIFY(!Extensions::enabled("socraticode")); QTRY_VERIFY(socratic->isHidden());
}

// WhiteWh fork: the desktop pictures of README.md. Opt-in: with
// ZERUS_README_SHOTS=<dir> the window renders the synthetic workspace above with
// the platform's own plugin, in the Compact skin and the dark theme, and saves
// each picture there as PNG. Run it without QT_QPA_PLATFORM=offscreen.
void TestSessionsWindow::readmeScreenshots()
{
    const auto shots = qEnvironmentVariable("ZERUS_README_SHOTS");
    if (shots.isEmpty()) QSKIP("Set ZERUS_README_SHOTS=<dir> to render the README pictures.");
    QVERIFY(QDir().mkpath(shots));
    const QStringList keys{"workspace/skin", "workspace/theme", "workspace/organization", "workspace/inspectorVisible", "workspace/collapsedTeams",
                           "workspace/sessionsCollapsed", "workspace/splitter", "sessions/geometry", "workspace/organizationBeforeSwarm",
                           "swarm/observedOrganization", "swarm/observedVersions", "swarm/driveFoldersShared"};
    const auto restore = qScopeGuard([keys] { for (const auto &key : keys) removeSetting(key); sessionCardTileInset() = 0; });
    QSettings().setValue("workspace/skin", "whitewh"); QSettings().setValue("workspace/theme", "dark");
    setJsonSetting("workspace/organization", QJsonDocument(ReadmeShots::organization()).toJson(QJsonDocument::Compact));
    QSettings().setValue("workspace/inspectorVisible", true);
    const double now = QDateTime::currentSecsSinceEpoch();
    // The New team coordinator needs an existing Git folder; the picture shows D:\work\orbit.
    QTemporaryDir folders; QVERIFY(folders.isValid());
    const QString gitFolder = QDir::toNativeSeparators(folders.filePath("orbit"));
    QVERIFY(QDir().mkpath(folders.filePath("orbit/.git")));
    const auto plain = ReadmeShots::fleet(now), designing = ReadmeShots::fleet(now, gitFolder);
    QJsonObject inspect = ReadmeShots::conversations(designing, now);
    const QJsonObject plainInspect = ReadmeShots::conversations(plain, now);
    for (auto it = plainInspect.constBegin(); it != plainInspect.constEnd(); ++it)
        if (!inspect.contains(it.key())) inspect.insert(it.key(), it.value());
    const QJsonObject data{{"inspect", inspect}, {"accounts", ReadmeShots::accounts(now)}, {"team", ReadmeShots::team(now)},
                           {"feed", ReadmeShots::feed(now)}, {"catalog", ReadmeShots::catalog()}, {"organization", ReadmeShots::organization()}};
    const QString fixture = m_dir->filePath("hgs-readme");
    QVERIFY(FakeCli::install(fixture, ReadmeShots::program));
    QVERIFY(FakeCli::replace(fixture + ".json", QJsonDocument(data).toJson(QJsonDocument::Compact)));

    SessionsWindow window(FakeCli::program(fixture)); window.resize(1440, 960); window.setFleet(plain); window.show();
    auto *controller = window.findChild<TeamController *>(); QVERIFY(controller);
    QTRY_COMPARE_WITH_TIMEOUT(controller->teams().size(), 1, 10000);
    // A list a little wider than the default keeps machine and branch on one line.
    auto *listPanel = window.findChild<QWidget *>("sessionListPanel"); QVERIFY(listPanel);
    auto *slot = window.findChild<QWidget *>("sessionListSlot"); QVERIFY(slot);
    auto *splitter = qobject_cast<QSplitter *>(slot->parentWidget()); QVERIFY(splitter);
    splitter->setSizes({350, splitter->sizes().value(0) + splitter->sizes().value(1) - 350});
    // Settles the window at its size, then takes the picture; `ready` may change
    // what shows just before. A window that cannot be as low keeps its minimum.
    const auto save = [&](const QString &name, const QSize &size, const std::function<void()> &ready = {}) {
        window.resize(size); QTest::qWait(1500);
        if (ready) ready();
        const auto picture = window.grab();
        return picture.width() == size.width() && picture.height() >= size.height() && picture.save(QDir(shots).filePath(name + ".png"));
    };
    auto *tabs = window.findChild<QTabWidget *>("sessionDetailTabs");
    const auto tabNamed = [tabs](const QString &text) { for (int i = 0; i < tabs->count(); ++i) if (tabs->tabText(i) == text) return i; return -1; };

    // 1. The workspace: the team's lead with its members, Activity and the inspector.
    window.showSession({}, "claude/orbit/checkout");
    QTRY_VERIFY(tabNamed("Team") >= 0);
    tabs->setCurrentIndex(0);
    QTRY_VERIFY_WITH_TIMEOUT(window.findChild<ActivityView *>("mainActivity")->plainText().contains("Address validation"), 10000);
    QVERIFY(save("workspace", {1440, 960}));

    // 2. Overview: the four machines, the accounts and the sessions at work.
    window.findChild<QPushButton *>("brandMark")->click();
    auto *dashboard = window.findChild<DashboardPage *>(); QVERIFY(dashboard);
    QTRY_VERIFY_WITH_TIMEOUT(dashboard->findChildren<QPushButton *>("dashboardAccountCard").size() >= 4, 15000);
    QVERIFY(save("fleet", {1440, 1080}));

    // 3. Projects: Orbit's folders on Windows and on the Unix machines.
    window.showProjects();
    auto *projects = window.findChild<ProjectsDialog *>("projectsPage"); QVERIFY(projects);
    projects->selectProject("orbit");
    QVERIFY(save("projects", {1440, 740}));

    // 4. A remote DeepSeek session asks which changes to review first.
    window.showSession("build-02", "dsh/orbit/review");
    emit dashboard->filterRequested("@all", "attention");
    QTRY_VERIFY_WITH_TIMEOUT(window.findChild<QuestionCard *>("questionCard") && window.findChild<QuestionCard *>("questionCard")->isVisible(), 10000);
    window.findChild<QPushButton *>("toggleInspector")->click();
    QTRY_VERIFY(!window.findChild<QWidget *>("sessionInspector")->isVisible());
    QVERIFY(save("attention", {1440, 960}));

    // 5, 6. The Team tab, with the list folded away: the graph beside the members
    // and the lead's chat, then the board.
    window.showSession({}, "claude/orbit/checkout");
    QTRY_VERIFY(tabNamed("Team") >= 0);
    tabs->setCurrentIndex(tabNamed("Team"));
    QTRY_VERIFY_WITH_TIMEOUT(window.findChild<QWidget *>("teamPhaseGraph"), 10000);
    window.findChild<QPushButton *>("railSessionsToggle")->click();
    QTRY_VERIFY(!listPanel->isVisible());
    QVERIFY(save("team-graph", {1440, 900}, [&] {
        // 130%, as large as the whole graph still fits the canvas: the largest zoom, then two steps back.
        for (int step = 0; step < 4; ++step) { window.findChild<QPushButton *>("teamWorkspaceZoomIn")->click(); QTest::qWait(300); }
        for (int step = 0; step < 2; ++step) { window.findChild<QPushButton *>("teamWorkspaceZoomOut")->click(); QTest::qWait(300); }
    }));
    for (auto *b : window.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>()) if (b->property("segment") == "board") { b->click(); break; }
    QTRY_VERIFY_WITH_TIMEOUT(window.findChild<QWidget *>("teamWorkspaceBoard"), 10000);
    QVERIFY(save("team-board", {1440, 900}));

    // 7. New team, led by a session without a team: the lead drafts it with you.
    window.findChild<QPushButton *>("railSessionsToggle")->click();
    QTRY_VERIFY(listPanel->isVisible());
    window.setFleet(designing);
    // F4: the Team designer's card keeps its fixed name and colour.
    {
        auto *list = window.findChild<SessionList *>("sessionList");
        QListWidgetItem *designerItem = nullptr;
        QTRY_VERIFY([&] {
            for (int i = 0; i < list->count(); ++i)
                if (list->item(i)->data(SessionRoles::Identity).toString().contains("claude/team-designer/team-designer")) designerItem = list->item(i);
            return designerItem != nullptr;
        }());
        QCOMPARE(designerItem->data(SessionRoles::Title).toString(), Teams::designerTitle());
        QCOMPARE(designerItem->data(SessionRoles::ProjectColor).value<QColor>(), QColor(Teams::designerColor()));
    }
    window.showSession({}, "claude/orbit/release");
    QTRY_VERIFY_WITH_TIMEOUT(tabNamed("New team") >= 0, 10000);
    tabs->setCurrentIndex(tabNamed("New team"));
    QTRY_VERIFY_WITH_TIMEOUT(tabs->currentWidget()->findChild<QWidget *>("teamNewDialog"), 10000);
    auto *form = tabs->currentWidget()->findChild<QWidget *>("teamNewDialog");
    // F6: the compact template list; F5: the member rows as a table or cards.
    QListWidget *templates = nullptr;
    QTRY_VERIFY((templates = form->findChild<QListWidget *>("teamTemplateList")));
    for (int i = 0; i < templates->count(); ++i) if (templates->item(i)->data(Qt::UserRole) == "feature") templates->setCurrentRow(i);
    QTRY_COMPARE(form->findChildren<QFrame *>("teamNewMemberRow").size() + form->findChildren<QFrame *>("teamNewMemberCard").size(), 3);
    // The members run on three computers.
    const QStringList computers{QString(), "macbook", "build-01"};
    for (int row = 1; row < 3; ++row) {
        auto *computer = form->findChildren<QComboBox *>("teamNewComputer").at(row);
        computer->setCurrentIndex(computer->findData(computers[row])); emit computer->activated(computer->currentIndex());
        QTRY_COMPARE(form->findChildren<QComboBox *>("teamNewComputer").at(row)->currentData().toString(), computers[row]);
    }
    form->findChild<QPlainTextEdit *>("teamNewGoal")->setPlainText(QString::fromLatin1(ReadmeShots::goal));
    form->findChild<QPlainTextEdit *>("teamNewProtocol")->setPlainText(QString::fromLatin1(ReadmeShots::protocol));
    form->findChild<QLineEdit *>("teamNewName")->setText("Saved cards");
    // The lead is this session; nothing exists before Start planning.
    QVERIFY(form->findChild<QRadioButton *>("teamNewLeadThis")->isChecked());
    QTRY_VERIFY(form->findChild<QPushButton *>("teamNewStartPlanning")->isEnabled());
    QVERIFY(!form->findChild<QPushButton *>("teamNewCreate")->isEnabled());
    // The form is long: the picture starts at the templates and ends with the members.
    QVERIFY(save("team-new", {1440, 1500}, [&] {
        form->findChild<QScrollArea *>("teamNewScroll")->verticalScrollBar()->setValue(90);
        // The picture shows the folder as D:\work\orbit; the form keeps what it checked.
        const QString shown = QStringLiteral("D:\\work\\orbit");
        for (auto *edit : form->findChildren<QLineEdit *>()) if (edit->text().contains(gitFolder)) { const QSignalBlocker block(edit); edit->setText(QString(edit->text()).replace(gitFolder, shown)); }
        for (auto *label : form->findChildren<QLabel *>()) if (label->text().contains(gitFolder)) label->setText(QString(label->text()).replace(gitFolder, shown));
    }));
}

void TestSessionsWindow::contentScaleLeavesWorkspaceChrome()
{
    QSettings().remove("workspace/contentScale");
    SessionsWindow window(program()); window.resize(1280, 860); window.setFleet(fleet()); window.show();
    window.showSession({}, "codex/hgs/dashboard");
    const auto preview = [&window](const QString &name) {
        const auto directory = qEnvironmentVariable("HGS_PREVIEW_DIR"); if (directory.isEmpty()) return true;
        window.showSession({}, "codex/hgs/dashboard");
        const QJsonObject details{{"tracked", true}, {"run_id", "run"}, {"conversation_id", "conversation-one"}, {"runtime_state", "live"},
            {"process_state", "running"}, {"activity", "idle"}, {"phase", "idle"}, {"cursor", 2}, {"events", QJsonArray{
                QJsonObject{{"seq", 1}, {"at", 2000000000}, {"type", "UserPromptSubmit"}, {"detail", "Make the transcript easier to read."}},
                QJsonObject{{"seq", 2}, {"at", 2000000010}, {"type", "Stop"}, {"detail", "## Done\n\nContent now follows **Settings → Appearance**. Run `ctest` to verify."}}}}};
        window.findChild<HgsClient *>()->inspectionReady({}, "codex/hgs/dashboard", details); QTest::qWait(50);
        return window.grab().save(directory + "/" + name);
    };
    QVERIFY(preview("content-scale-100.png"));
    const auto pixels = [&window](const QString &name) {
        auto *widget = window.findChild<QWidget *>(name); return widget ? QFontInfo(widget->font()).pixelSize() : -1;
    };
    auto *composer = window.findChild<MessageComposer *>("messageComposer"); QVERIFY(composer);
    const auto editorPixels = [composer] { return QFontInfo(composer->editor()->font()).pixelSize(); };
    const QStringList chrome{"sessionList", "search", "detailTitle", "sessionInspector", "sessionInfo", "terminalStatus", "sendMessage", "taskDetailTabs"};
    QMap<QString, int> before; for (const auto &name : chrome) { before.insert(name, pixels(name)); QVERIFY2(before.value(name) > 0, qPrintable(name)); }
    const int tabs = QFontInfo(window.findChild<QTabWidget *>("sessionDetailTabs")->tabBar()->font()).pixelSize();
    QCOMPARE(pixels("activity"), 13); QCOMPARE(pixels("terminalScreen"), 13); QCOMPARE(editorPixels(), 13);
    QCOMPARE(pixels("taskInspectorDetail"), 13);
    window.findChild<QPushButton *>("workspaceSettings")->click();
    auto *settings = window.findChild<QWidget *>("settingsPage"); QVERIFY(settings && settings->isVisible());
    auto *slider = settings->findChild<QSlider *>("workspaceContentScale"); QVERIFY(slider && slider->isVisible());
    QCOMPARE(slider->minimum(), 15); QCOMPARE(slider->maximum(), 40); QCOMPARE(slider->value(), 20);
    QCOMPARE(settings->findChild<QLabel *>("workspaceContentScaleValue")->text(), QString("100%"));
    slider->setValue(15);
    QCOMPARE(QSettings().value("workspace/contentScale").toDouble(), 0.75);
    QCOMPARE(settings->findChild<QLabel *>("workspaceContentScaleValue")->text(), QString("75%"));
    QTRY_COMPARE(editorPixels(), 10);
    QCOMPARE(pixels("activity"), 10); QCOMPARE(pixels("subagentJournal"), 10); QCOMPARE(pixels("terminalScreen"), 10);
    QCOMPARE(pixels("taskInspectorDetail"), 10);
    for (const auto &name : chrome) QCOMPARE(pixels(name), before.value(name));
    QCOMPARE(QFontInfo(window.findChild<QTabWidget *>("sessionDetailTabs")->tabBar()->font()).pixelSize(), tabs);
    QVERIFY(preview("content-scale-75.png"));
    window.findChild<QPushButton *>("workspaceSettings")->click();
    slider->setValue(30); slider->setValue(40);
    QCOMPARE(QSettings().value("workspace/contentScale").toDouble(), 2.0);
    QCOMPARE(settings->findChild<QLabel *>("workspaceContentScaleValue")->text(), QString("200%"));
    QTRY_COMPARE(editorPixels(), 26);
    if (!qEnvironmentVariable("HGS_PREVIEW_DIR").isEmpty())
        QVERIFY(window.grab().save(qEnvironmentVariable("HGS_PREVIEW_DIR") + "/content-scale-settings.png"));
    QCOMPARE(pixels("activity"), 26); QCOMPARE(pixels("subagentJournal"), 26); QCOMPARE(pixels("terminalScreen"), 26);
    QCOMPARE(pixels("taskInspectorDetail"), 26);
    for (const auto &name : chrome) QCOMPARE(pixels(name), before.value(name));
    QCOMPARE(QFontInfo(window.findChild<QTabWidget *>("sessionDetailTabs")->tabBar()->font()).pixelSize(), tabs);
    // Theme changes rebuild workspace styles without dropping the preference.
    QSettings().setValue("workspace/theme", "dark"); settings->findChild<QComboBox *>("workspaceTheme")->setCurrentIndex(1);
    QCOMPARE(editorPixels(), 26); QCOMPARE(pixels("activity"), 26);
    QVERIFY(preview("content-scale-200.png"));
    SessionsWindow restored(program()); restored.setFleet(fleet());
    QCOMPARE(QFontInfo(restored.findChild<MessageComposer *>("messageComposer")->editor()->font()).pixelSize(), 26);
    QSettings().setValue("workspace/contentScale", 2.5);
    SessionsWindow legacy(program()); legacy.setFleet(fleet());
    QCOMPARE(QSettings().value("workspace/contentScale").toDouble(), 2.0);
    QCOMPARE(QFontInfo(legacy.findChild<MessageComposer *>("messageComposer")->editor()->font()).pixelSize(), 26);
    window.findChild<QPushButton *>("workspaceSettings")->click(); slider->setValue(20);
    QTRY_COMPARE(editorPixels(), 13);
    QCOMPARE(pixels("activity"), 13); QCOMPARE(pixels("terminalScreen"), 13);
}

void TestSessionsWindow::settingsSlidersResetToDefaults()
{
    SessionsWindow window(program()); window.resize(1280, 860); window.setFleet(fleet()); window.show();
    window.activateWindow(); QVERIFY(QTest::qWaitForWindowActive(&window));
    window.findChild<QPushButton *>("workspaceSettings")->click();
    auto *settings = window.findChild<QWidget *>("settingsPage"); QVERIFY(settings && settings->isVisible());
    auto *scale = settings->findChild<QSlider *>("workspaceContentScale"), *width = settings->findChild<QSlider *>("workspaceActivityWidth");
    auto *resetScale = settings->findChild<QPushButton *>("resetContentScale"), *resetWidth = settings->findChild<QPushButton *>("resetActivityWidth");
    auto *full = settings->findChild<QCheckBox *>("workspaceActivityFullWidth");
    QVERIFY(scale && width && full && resetScale && resetWidth);
    QCOMPARE(resetScale->toolTip(), QString("Reset to 100%")); QCOMPARE(resetWidth->toolTip(), QString("Reset to 900 px"));
    // Defaults leave nothing to reset, so the buttons appear only after a change.
    QVERIFY(!resetScale->isVisible()); QVERIFY(!resetWidth->isVisible());
    const int below = full->mapTo(settings, QPoint()).y();
    scale->setValue(30); width->setValue(1300);
    QVERIFY(resetScale->isVisible()); QVERIFY(resetWidth->isVisible());
    // Appearing does not move the rows beneath.
    QTest::qWait(50); QCOMPARE(full->mapTo(settings, QPoint()).y(), below);
    // Icon buttons right after each value, named by the default they restore.
    const QList<QPair<QPushButton *, QLabel *>> rows{{resetScale, settings->findChild<QLabel *>("workspaceContentScaleValue")},
                                                     {resetWidth, settings->findChild<QLabel *>("workspaceActivityWidthValue")}};
    for (const auto &[button, value] : rows) {
        QVERIFY(button->text().isEmpty()); QVERIFY(!button->icon().isNull());
        QCOMPARE(button->property("glyph").toString(), QString("reset"));
        QCOMPARE(button->accessibleName(), button->toolTip());
        const auto center = [settings](QWidget *widget) { return widget->mapTo(settings, widget->rect().center()); };
        QVERIFY(qAbs(center(button).y() - center(value).y()) <= 2);
        const int textEnd = value->mapTo(settings, QPoint()).x() + value->fontMetrics().horizontalAdvance(value->text());
        const int gap = button->mapTo(settings, QPoint()).x() - textEnd;
        QVERIFY2(gap >= 0 && gap <= 8, qPrintable(QString("%1: %2").arg(button->objectName()).arg(gap)));
    }
    if (const auto directory = qEnvironmentVariable("HGS_PREVIEW_DIR"); !directory.isEmpty()) {
        auto *theme = settings->findChild<QComboBox *>("workspaceTheme");
        theme->setCurrentIndex(1); QVERIFY(window.grab().save(directory + "/settings-reset-dark.png"));
        theme->setCurrentIndex(2); QVERIFY(window.grab().save(directory + "/settings-reset-light.png"));
        theme->setCurrentIndex(0);
    }
    // Resetting from the keyboard leaves focus on the slider, not on a later control.
    resetScale->setFocus(Qt::TabFocusReason); QVERIFY(resetScale->hasFocus());
    QTest::keyClick(resetScale, Qt::Key_Space);
    QCOMPARE(scale->value(), 20); QCOMPARE(QSettings().value("workspace/contentScale").toDouble(), 1.0);
    QCOMPARE(settings->findChild<QLabel *>("workspaceContentScaleValue")->text(), QString("100%"));
    QVERIFY(!resetScale->isVisible()); QVERIFY(scale->hasFocus());
    QTest::mouseClick(resetWidth, Qt::LeftButton);
    QCOMPARE(width->value(), 900); QCOMPARE(QSettings().value("workspace/activityWidth").toInt(), 900);
    QCOMPARE(settings->findChild<QLabel *>("workspaceActivityWidthValue")->text(), QString("900 px"));
    QVERIFY(!resetWidth->isVisible());
    QTest::qWait(50); QCOMPARE(full->mapTo(settings, QPoint()).y(), below);
    // Full width has no column width to reset.
    width->setValue(1300); QVERIFY(resetWidth->isVisible());
    full->setChecked(true); QVERIFY(!resetWidth->isVisible());
    full->setChecked(false); QVERIFY(resetWidth->isVisible());
    // Activity's column edges and menu keep the button current.
    emit window.findChild<ActivityView *>("mainActivity")->columnResetRequested();
    QCOMPARE(width->value(), 900); QVERIFY(!resetWidth->isVisible());
    emit window.findChild<ActivityView *>("mainActivity")->columnWidthRequested(1200);
    QVERIFY(resetWidth->isVisible());
}

void TestSessionsWindow::followSystemAppearanceFollowsARuntimePaletteChange()
{
    // A system light/dark switch changes the application palette while Zerus runs.
    const auto restore=qScopeGuard([]{QApplication::setPalette(QPalette());QSettings().remove("workspace/theme");});
    QPalette light;light.setColor(QPalette::Window,QColor("#f0f0f0"));light.setColor(QPalette::WindowText,Qt::black);
    QPalette dark;dark.setColor(QPalette::Window,QColor("#202020"));dark.setColor(QPalette::WindowText,Qt::white);
    QApplication::setPalette(light);QSettings().setValue("workspace/theme","system");
    SessionsWindow window(program());window.setFleet(fleet());window.show();
    auto *list=window.findChild<QWidget*>("sessionList");QVERIFY(list);
    QVERIFY(!list->property("hgsDark").toBool());
    QApplication::setPalette(dark);QTRY_VERIFY(list->property("hgsDark").toBool());
    QApplication::setPalette(light);QTRY_VERIFY(!list->property("hgsDark").toBool());
    // An explicit theme ignores the system appearance.
    QSettings().setValue("workspace/theme","light");QApplication::setPalette(dark);
    QTest::qWait(50);QVERIFY(!list->property("hgsDark").toBool());
}

void TestSessionsWindow::alwaysOnTopSettingKeepsWindowAbove()
{
    // Off by default. The option applies in place: changing a visible window's
    // flags through QWidget would hide it from the user's workspace.
    const auto above = [](QWidget &window) { return window.windowHandle()->flags().testFlag(Qt::WindowStaysOnTopHint); };
    SessionsWindow window(program()); window.setFleet(fleet()); window.show(); QVERIFY(QTest::qWaitForWindowExposed(&window));
    QVERIFY(!above(window));
    window.findChild<QPushButton *>("workspaceSettings")->click();
    auto *option = window.findChild<QCheckBox *>("workspaceAlwaysOnTop");
    QVERIFY(option && option->isVisible() && option->isEnabled() && !option->isChecked());
    option->setChecked(true);
    QVERIFY(QSettings().value("workspace/alwaysOnTop").toBool());
    QVERIFY(above(window)); QVERIFY(window.windowFlags().testFlag(Qt::WindowStaysOnTopHint)); QVERIFY(window.isVisible());
    SessionsWindow restored(program()); restored.setFleet(fleet()); restored.show(); QVERIFY(QTest::qWaitForWindowExposed(&restored));
    QVERIFY(above(restored));
    option->setChecked(false);
    QVERIFY(!above(window)); QVERIFY(window.isVisible());
    QVERIFY(!QSettings().value("workspace/alwaysOnTop").toBool());
    QSettings().remove("workspace/alwaysOnTop");
}

void TestSessionsWindow::dashboardAndMultiMachineNavigation()
{
    SessionsWindow window(program()); auto state=fleet(); auto third=state.local(); third.host="build"; state.setPeer(third,0);
    window.setFleet(state); window.show();
    auto *filter=window.findChild<MachineFilter *>("machineFilterChips"); auto *list=window.findChild<SessionList *>("sessionList");
    QVERIFY(filter); QVERIFY(list); filter->setSelection({"mac","build"}); QCOMPARE(list->count(),5);
    for(int i=0;i<list->count();++i) QVERIFY(!list->item(i)->data(SessionRoles::Identity).toString().startsWith("arch\n"));
    auto *brand=window.findChild<QPushButton *>("brandMark"); QVERIFY(brand); brand->click();
    auto *dashboard=window.findChild<DashboardPage *>(); QVERIFY(dashboard->isVisible()); QVERIFY(brand->isChecked()); brand->click(); QVERIFY(brand->isChecked());
    QVERIFY(!window.findChild<QPushButton *>("organizeGroups"));
    bool launchDialogOpened = false;
    QTimer::singleShot(0, &window, [&] {
        auto *dialog = qobject_cast<NewSessionDialog *>(QApplication::activeModalWidget());
        launchDialogOpened = dialog != nullptr;
        if (dialog) dialog->reject();
    });
    QVERIFY(!dashboard->findChild<QPushButton *>("dashboardNewSession"));
    auto *create=window.findChild<QPushButton *>("newSession");QVERIFY(create->isVisible());
    QCOMPARE(create->parentWidget()->objectName(),QString("sidebar"));
    create->click(); QVERIFY(launchDialogOpened);
    dashboard->filterRequested("@all","attention"); QVERIFY(list->isVisible()); QCOMPARE(list->count(),1);
    QVERIFY(filter->selection().isEmpty()); QVERIFY(!brand->isChecked());
    brand->click(); dashboard->filterRequested("@local","all"); QCOMPARE(list->count(),4);
    QCOMPARE(filter->selection(),QSet<QString>{"@local"});
    brand->click(); dashboard->sessionRequested("mac","claude/infra/review");
    QVERIFY(list->currentItem()->data(SessionRoles::Identity).toString().startsWith("mac\n"));
    brand->click(); dashboard->machinesRequested("mac"); auto *machines=window.findChild<MachinesPage *>(); QVERIFY(machines->isVisible());
    machines->accountsRequested("mac"); auto *accounts=window.findChild<AccountsPage *>(); QVERIFY(accounts->isVisible());
    QCOMPARE(accounts->findChild<QComboBox *>("accountMachineFilter")->currentData().toString(),QString("mac"));
}

void TestSessionsWindow::hiddenPagesRenderWhenShown()
{
    // Polls keep hidden pages' data current; each page renders when it is shown.
    SessionsWindow window(program()); auto state = fleet(); window.setFleet(state); window.show();
    auto *list = window.findChild<SessionList *>("sessionList"); auto *dashboard = window.findChild<DashboardPage *>();
    auto *badge = window.findChild<QLabel *>("railAttentionBadge"); auto *sessionsNav = qobject_cast<QPushButton *>(badge->parentWidget());
    auto *brand = window.findChild<QPushButton *>("brandMark"); auto *total = dashboard->findChild<QPushButton *>("dashboardSessions");
    QVERIFY(list); QVERIFY(sessionsNav); QVERIFY(brand); QVERIFY(total);
    const int rows = list->count(); const auto attention = badge->text();
    brand->click(); QVERIFY(dashboard->isVisible()); const auto before = total->text();
    auto box = state.local(); auto extra = box.sessions[0]; extra.name = "codex/hgs/extra"; extra.tag = "extra"; extra.phase = "approval";
    box.sessions.append(extra); state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    QSignalSpy inserted(list->model(), &QAbstractItemModel::rowsInserted);
    window.setFleet(state);
    QCOMPARE(list->count(), rows); QCOMPARE(inserted.size(), 0);
    QVERIFY(total->text() != before); QCOMPARE(badge->text(), QString::number(attention.toInt() + 1));
    sessionsNav->click(); QVERIFY(list->isVisible()); QCOMPARE(list->count(), rows + 1);
    const int shown = inserted.size(); QVERIFY(shown > 0); QTest::qWait(20); QCOMPARE(inserted.size(), shown);
    const auto updated = total->text();
    box.sessions.removeLast(); state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    QCOMPARE(list->count(), rows); QCOMPARE(total->text(), updated);
    brand->click(); QCOMPARE(total->text(), before);
    // A closed window renders its current page when it opens again.
    window.hide(); box.sessions.append(extra); state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    window.show(); QCOMPARE(total->text(), updated);
}

void TestSessionsWindow::pollsKeepUnchangedActivity()
{
    // Fleet polls and repeated inspections of an unchanged session leave Activity alone.
    SessionsWindow window(program()); auto state = fleet(); window.setFleet(state); window.show();
    auto *client = window.findChild<HgsClient *>(); QSignalSpy inspected(client, &HgsClient::inspectionReady);
    window.showSession({}, "codex/hgs/dashboard"); QTRY_VERIFY(!inspected.isEmpty());
    auto *activity = window.findChild<ActivityView *>("mainActivity"); QVERIFY(activity); QTRY_VERIFY(!activity->renderPending());
    const int passes = activity->renderPasses();
    QJsonObject details{{"tracked", true}, {"conversation_id", "conversation-one"}, {"events", QJsonArray{}}, {"cursor", 0}, {"goal_observed_at", 200.}};
    window.setFleet(state); client->inspectionReady({}, "codex/hgs/dashboard", details);
    details["goal_observed_at"] = 205.; client->inspectionReady({}, "codex/hgs/dashboard", details); window.setFleet(state);
    QCOMPARE(activity->renderPasses(), passes);
    details["cursor"] = 1; details["events"] = QJsonArray{QJsonObject{{"seq", 1}, {"type", "Stop"}, {"detail", "Fresh reply"}, {"at", 10.}}};
    client->inspectionReady({}, "codex/hgs/dashboard", details); QTRY_VERIFY(activity->renderPasses() > passes);
    QTRY_VERIFY(activity->plainText().contains("Fresh reply"));
    // Process links depend on a setting that the next render must follow.
    const int linked = activity->renderPasses(); QSettings().setValue("processes/enabled", true); window.setFleet(state);
    QTRY_VERIFY(activity->renderPasses() > linked);
}

void TestSessionsWindow::dashboardActivityRowsStayReadable_data()
{
    QTest::addColumn<QString>("theme");
    QTest::newRow("dark") << QString("dark");
    QTest::newRow("light") << QString("light");
}

void TestSessionsWindow::dashboardActivityRowsStayReadable()
{
    QFETCH(QString, theme);
    QSettings().setValue("workspace/theme", theme);
    SessionsWindow window(program()); window.resize(1170, 660); window.show();
    window.findChild<QPushButton *>("brandMark")->click();
    auto state = fleet(); auto local = state.local(); auto &session = local.sessions[0];
    session.gitBranch = "main"; session.turnStarted = QDateTime::currentSecsSinceEpoch() - 76;
    session.subagentSource = "hooks"; session.subagentCountsComplete = true;
    session.subagentActiveCount = 2; session.subagentTotalCount = 4;
    state.setLocal(local, QDateTime::currentMSecsSinceEpoch());
    QPointer<QPushButton> workingRow;
    // The real window stylesheet and repeated fleet updates must retain both
    // card geometry and identity/focus. Dashboard cards have no roster control.
    for (int poll = 0; poll < 3; ++poll) {
        window.setFleet(state); QTest::qWait(20);
        const auto rows = window.findChildren<QPushButton *>("dashboardSessionRow");
        QCOMPARE(rows.size(), 2);
        for (auto *row : rows) {
            QVERIFY2(row->height() >= 104, qPrintable(QString("Activity row collapsed to %1px").arg(row->height())));
            auto *model = row->findChild<QStandardItemModel *>("dashboardSessionModel");
            QVERIFY(model); const auto index = model->index(0, 0);
            for (const int role : {SessionRoles::Title, SessionRoles::Status,
                    SessionRoles::Agent, SessionRoles::Host, SessionRoles::Detail, SessionRoles::Model, SessionRoles::Effort})
                QVERIFY(!index.data(role).toString().isEmpty());
            QVERIFY(!index.data(SessionRoles::HasChildren).toBool());
            if (index.data(SessionRoles::Working).toBool()) {
                QCOMPARE(index.data(SessionRoles::Title).toString(), QString("dashboard"));
                QCOMPARE(index.data(SessionRoles::Meta).toString(), QString("main"));
                QCOMPARE(index.data(SessionRoles::MetaIcon).toString(), QString("git-branch"));
                QCOMPARE(index.data(SessionRoles::WorkingSince).toDouble(), session.turnStarted);
                QCOMPARE(index.data(SessionRoles::Children).toString(), QString("2"));
                if (poll == 0) { workingRow = row; row->setFocus(); }
                else { QCOMPARE(row, workingRow.data()); QVERIFY(row->hasFocus()); }
            } else QCOMPARE(index.data(SessionRoles::Meta).toString(), QString());
        }
    }
    const auto directory = qEnvironmentVariable("HGS_DASHBOARD_PREVIEW");
    if (!directory.isEmpty()) { QDir().mkpath(directory); QVERIFY(window.grab().save(directory + "/integrated-" + theme + ".png")); QVERIFY(workingRow->grab().save(directory + "/card-" + theme + ".png")); }
    QVERIFY(workingRow);
    // Even the subagent-count area navigates; it never expands a dashboard row.
    QTest::mouseClick(workingRow, Qt::LeftButton, Qt::NoModifier, QPoint(workingRow->width() - 30, workingRow->height() - 20));
    auto *list = window.findChild<SessionList *>("sessionList");
    QVERIFY(list->isVisible());
    QVERIFY(list->currentItem()->data(SessionRoles::Identity).toString().contains("codex/hgs/dashboard"));

}

// Renders the Tasks tab beside design/task-tracker: the prototype's projects,
// sessions and tasks, each stage in both themes, hovered rows and the dialogs.
void TestSessionsWindow::tasksPreview()
{
    const auto directory = qEnvironmentVariable("HGS_PREVIEW_DIR");
    if (directory.isEmpty()) QSKIP("Set HGS_PREVIEW_DIR to render the tasks preview");
    QDir().mkpath(directory);
    SessionOrganization org; const auto orchard = org.createGroup("orchard"), lumen = org.createGroup("lumen");
    for (const auto *name : {"claude/orchard/api", "codex/orchard/web", "claude/orchard/docs"}) org.moveSession(org.observe("arch", name, ""), orchard);
    for (const auto *name : {"claude/lumen/main", "codex/lumen/mobile"}) org.moveSession(org.observe("arch", name, ""), lumen);
    setJsonSetting("workspace/organization", QJsonDocument(org.toJson()).toJson(QJsonDocument::Compact));
    const auto session = [](const QString &name, const QString &activity, const QString &phase, const QString &prompt, const QString &model, const QString &effort) {
        SessionInfo info; info.name = name; info.cmd = name.section('/', 0, 0); info.project = name.section('/', 1, 1); info.tag = name.section('/', 2);
        info.tracked = true; info.resumable = true; info.activity = activity; info.phase = phase; info.prompt = prompt; info.model = model; info.effort = effort;
        if (phase == "tool") info.currentTool = "Bash";
        return info;
    };
    BoxState arch; arch.host = "arch"; arch.ok = true; arch.metrics = QJsonObject{{"platform", "linux"}};
    arch.sessions = {session("claude/orchard/api", "busy", "tool", "T-bt41xh: Add rate limiting to POST /v1/orders", "opus-5-5", "high"),
                     session("codex/orchard/web", "busy", "input", "T-vg42sm: The checkout e2e test fails in CI now and then", "gpt-6.1-sol", "high"),
                     session("claude/orchard/docs", "idle", "idle", "Collect questions for partners about their API limits", "sonnet-5-5", "medium"),
                     session("claude/lumen/main", "busy", "tool", "The image upload test sometimes fails in parallel runs", "opus-5-5", "xhigh"),
                     session("codex/lumen/mobile", "idle", "idle", "Polish the onboarding screens", "gpt-6.1-sol", "high")};
    FleetState fleet; fleet.setLocal(arch, QDateTime::currentMSecsSinceEpoch());
    // The prototype's orchard tasks in its order; IDs keep its numbers.
    const qint64 now = QDateTime::currentMSecsSinceEpoch(), minute = 60000;
    const auto ran = [](const QString &name) { return FakeTasksCli::pinned(name); };
    const auto agent = [](const QString &name) { return QJsonObject{{"kind", "agent"}, {"session", name}}; };
    QJsonArray tasks;
    const auto add = [&](const QString &id, const QString &stage, const QString &text, QJsonObject extra = {}) {
        auto task = FakeTasksCli::task(int(tasks.size()) + 1, stage, extra); task["uuid"] = "uuid-" + id; task["id"] = id; task["ref"] = "orchard/" + id;
        task["text"] = text; task["rank"] = QString(QChar(char16_t(u'a' + tasks.size()))); task["created_at"] = double(now - 90 * minute);
        if (!extra.contains("stage_at")) task["stage_at"] = double(now - 60 * minute);
        QJsonArray log{QJsonObject{{"actor", extra.value("author").toObject().isEmpty() ? QJsonObject{{"kind", "person"}} : extra.value("author")},
                                   {"at", double(now - 90 * minute)}, {"kind", "created"}, {"session", QJsonValue()}}};
        if (extra.contains("session")) log << QJsonObject{{"actor", QJsonObject{{"kind", "person"}}}, {"at", task["stage_at"]}, {"kind", "sent"}, {"session", extra.value("session")}};
        task["log"] = log; tasks << task;
    };
    add("T-ka38pd", "done", "Add a /healthz endpoint: check the database and Redis, answer 200 or 503 with the reason.",
        {{"session", ran("claude/orchard/api")}, {"summary", "/healthz checks the database and Redis; tests are green."}});
    add("T-mc39fw", "review", "Make a multi-stage Dockerfile for web; the image should get much smaller.",
        {{"session", ran("codex/orchard/web")}, {"summary", "Image 1.2 GB → 310 MB."}});
    add("T-qe40rn", "done", "Remove the N+1 in GET /v1/orders and add a test for the number of database queries.",
        {{"session", ran("claude/orchard/api")}, {"summary", "select_related and a query-count test."}});
    add("T-bt41xh", "active", "Add rate limiting to POST /v1/orders: at most 30 requests per minute per API key. The limit comes from the config; answer 429 with Retry-After. Cover it with tests.",
        {{"session", ran("claude/orchard/api")}, {"stage_at", double(now - 13 * minute)}});
    add("T-vg42sm", "active", "The checkout e2e test fails in CI now and then. Find the cause and make it stable without disabling it.",
        {{"session", ran("codex/orchard/web")}, {"stage_at", double(now - 34 * minute)}});
    add("T-dn44wk", "todo", "Move the sessions table to UUID keys. The migration must not cause downtime: new column, backfill, switch, drop the old one.",
        {{"target", ran("claude/orchard/api")}});
    add("T-hy43ce", "todo", "Add an Export CSV button to the admin orders page: current filters, all pages, UTF-8 with BOM for Excel.");
    add("T-zu46lo", "todo", "Update Next.js to 15.4 and fix the type errors. The build and e2e must be green.");
    add("T-pr45ja", "todo", "Document in the README how to run the local environment: docker-compose, seeds, stripe-mock, ports.",
        {{"target", ran("claude/orchard/api")}, {"author", agent("claude/orchard/api")}});
    add("T-fw47tb", "backlog", "Add a dark theme to the dashboard with a switch in the user profile.");
    add("T-co48yv", "backlog", "Find out why /search takes 2–3 s on a copy of the production database. Measure first, then propose options.");
    add("T-ji49mr", "backlog", "Replace moment.js with date-fns across web without changing date formats.");
    add("T-sx50ge", "backlog", "Collect questions for partners about their API limits and draft an email.",
        {{"target", ran("claude/orchard/docs")}, {"author", agent("claude/orchard/docs")}});
    QFile original(script()); QVERIFY(original.open(QIODevice::ReadOnly));
    const QJsonObject settings{{"task_template", "[Zerus task {ref}]\nWritten by {author}.\n{prompt}\n{history}\nWhen you finish, or find that it cannot be done, report with\n"
                                                 "{tools}.task_report: one to three sentences on what you changed, how you\nchecked it, and what is left undone or why it cannot be done."},
                               {"cancel_template", "[Zerus task {ref} cancelled]\nThe person cancelled this task. Stop working on it. Do not continue and do\n"
                                                   "not revert anything; list what you changed and wait for the person."},
                               {"agents_other", true}, {"agent_limit", 0}};
    const auto json = [](const QJsonValue &value) {
        return FakeCli::pythonString(QString::fromUtf8(value.isArray() ? QJsonDocument(value.toArray()).toJson(QJsonDocument::Compact) : QJsonDocument(value.toObject()).toJson(QJsonDocument::Compact)));
    };
    const QByteArray answer = "elif c=='task':\n T=json.loads(" + json(tasks) + ")\n P={'id':a[a.index('--project')+1] if '--project' in a else '','name':'orchard'}\n"
        " if a[1]=='get':print(json.dumps({'project':P,'task':[t for t in T if t['uuid']==a[2]][0]}))\n"
        " elif a[1]=='settings':print(json.dumps({'project':P,'settings':json.loads(" + json(settings) + ")}))\n"
        " else:print(json.dumps({'project':P,'tasks':T,'damaged':[]}))\n";
    const QString fixture = m_dir->filePath("tasks-preview-hgs");
    QVERIFY(FakeCli::install(fixture, original.readAll().replace("if False:pass\n", "if False:pass\n" + answer)));
    SessionsWindow window(FakeCli::program(fixture)); keepDetailsPollsOut(window); window.resize(1240, 800); window.setFleet(fleet); window.show();
    window.showSession({}, "claude/orchard/api");
    auto *tabs = window.findChild<QTabWidget *>("sessionDetailTabs"); auto *view = window.findChild<TasksView *>(); QVERIFY(tabs && view);
    tabs->setCurrentWidget(view); QTRY_VERIFY(view->findChild<QWidget *>("taskRow:T-mc39fw"));
    const auto stage = [&](const QString &name) { view->findChild<QAbstractButton *>("taskTile:" + name)->click(); QTest::qWait(80); };
    const auto hover = [&](const QString &id) {
        auto *row = view->findChild<QWidget *>("taskRow:" + id); QVERIFY(row);
        QEnterEvent enter(QPointF(20, 10), QPointF(20, 10), QPointF(20, 10)); QApplication::sendEvent(row, &enter); QTest::qWait(40);
    };
    const auto save = [&](QWidget *widget, const QString &name) { QVERIFY(widget->grab().save(directory + "/" + name + ".png")); };
    auto *themeChoice = window.findChild<QComboBox *>("workspaceTheme"); QVERIFY(themeChoice);
    for (const auto &theme : {QStringLiteral("dark"), QStringLiteral("light")}) {
        themeChoice->setCurrentIndex(themeChoice->findData(theme)); QTest::qWait(100);
        for (const auto &name : {"review", "active", "todo", "backlog", "done"}) { stage(name); save(&window, QStringLiteral("real-%1-%2").arg(name, theme)); }
    }
    themeChoice->setCurrentIndex(themeChoice->findData("dark")); QTest::qWait(100);
    // Activity beside it: the Tasks tab keeps its column.
    tabs->setCurrentIndex(0); QTest::qWait(80); save(&window, "real-activity"); tabs->setCurrentWidget(view); QTest::qWait(80);
    stage("todo"); hover("T-dn44wk"); save(&window, "real-todo-hover");
    view->findChild<QToolButton *>("taskOnlySession")->click(); QTest::qWait(80); save(&window, "real-todo-only");
    view->findChild<QToolButton *>("taskOnlySession")->click(); QTest::qWait(80);
    stage("active"); hover("T-bt41xh"); save(&window, "real-active-hover");
    view->findChild<QWidget *>("taskRow:T-bt41xh")->findChild<QToolButton *>("taskAction:cancel")->click();
    QDialog *dialog = nullptr; QTRY_VERIFY((dialog = window.findChild<QDialog *>("taskCancelDialog")) && dialog->isVisible());
    QTest::qWait(60); save(dialog, "real-cancel"); dialog->reject(); QTest::qWait(40);
    stage("todo"); QTest::mouseClick(view->findChild<QWidget *>("taskRow:T-dn44wk"), Qt::LeftButton, {}, QPoint(300, 30));
    QTRY_VERIFY((dialog = window.findChild<QDialog *>("taskDialog")) && dialog->isVisible());
    QTest::qWait(60); save(dialog, "real-task-settings"); dialog->reject(); QTest::qWait(40);
    view->findChild<QToolButton *>("taskSettings")->click();
    QTRY_VERIFY((dialog = window.findChild<QDialog *>("taskSettingsDialog")) && dialog->isVisible());
    QTest::qWait(60); save(dialog, "real-project-settings"); dialog->reject();
}

QTEST_MAIN(TestSessionsWindow)
#include "test_sessionswindow_settings1.moc"
