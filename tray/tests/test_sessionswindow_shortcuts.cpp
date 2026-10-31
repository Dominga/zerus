// U62 (#712): Keyboard shortcuts, chords and Go to, from test_sessionswindow.
#include "SessionsWindowFixture.h"

class TestSessionsWindow : public SessionsWindowFixture {
    Q_OBJECT
private slots:
    // Keyboard commands; see docs/superpowers/specs/2026-10-15-keyboard-shortcuts-design.md.
    void shortcutsSwitchSections();
    void shortcutsMoveBetweenSessions();
    void shortcutsFindSessionsNeedingAttention();
    void shortcutsCycleSessionTabs();
    void terminalChordReturnsToActivity();
    void shortcutsInModalDialogsStayLocal();
    void zoomChordsChangeContentScale();
    void reboundChordsApplyAtOnce();
    void projectListKeepsItsReorderKeys();
    void focusChordsCycleSessionAreas();
    void goToOpensAndActivatesItems();
    void cheatSheetOpensKeyboardSettings();
    void renameChordWorksOnTheSessionsPage();
    void attentionChordsStayInSearchResults();
    void goToFollowsTheSessionList();
};

void TestSessionsWindow::shortcutsSwitchSections()
{
    SessionsWindow window(program()); window.setFleet(fleet()); window.show(); window.activateWindow();
    QVERIFY(QTest::qWaitForWindowActive(&window));
    auto *pages = workspacePages(window); QVERIFY(pages); QCOMPARE(pages->currentIndex(), 0);
    // Rail buttons in page order: Sessions, Projects, Machines, Overview, Accounts, Settings.
    const QList<QPushButton *> rail{railButton(window, "sessions"), railButton(window, "projects"), railButton(window, "machines"),
        window.findChild<QPushButton *>("brandMark"), railButton(window, "accounts"), window.findChild<QPushButton *>("workspaceSettings")};
    for (auto *button : rail) QVERIFY(button);
    QPushButton *attention = nullptr;
    for (auto *button : window.findChildren<QPushButton *>("sessionFilter")) if (button->property("filter") == "attention") attention = button;
    QVERIFY(attention); attention->click(); QVERIFY(attention->isChecked());
    const QList<QPair<QString, int>> chords{{"go.projects", 1}, {"go.machines", 2}, {"go.accounts", 4}, {"go.overview", 3},
        {"go.settings", 5}, {"go.sessions", 0}, {"go.sessions", 0}};
    for (const auto &chord : chords) {
        pressCommand(&window, chord.first);
        QCOMPARE(pages->currentIndex(), chord.second);
        for (int page = 0; page < rail.size(); ++page) QCOMPARE(rail[page]->isChecked(), page == chord.second);
    }
    // Unlike a second click on the rail button, the chord keeps the filter.
    QVERIFY(attention->isChecked());
}

void TestSessionsWindow::shortcutsMoveBetweenSessions()
{
    auto box = fleet().local(); auto first = box.sessions[0];
    first.activity = "idle"; first.phase = "idle"; first.currentTool.clear();
    first.subagents = {{"agent-a", QJsonObject{{"name", "Review"}, {"state", "working"}}}};
    box.sessions = {first, box.sessions[1]};
    FleetState state; state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionOrganization org; const auto release = org.createGroup("Release"); org.createGroup("Research");
    for (const auto &name : {QStringLiteral("codex/hgs/dashboard"), QStringLiteral("kimi/docs/research")}) org.moveSession(org.observe("arch", name, {}), release);
    setJsonSetting("workspace/organization", QJsonDocument(org.toJson()).toJson(QJsonDocument::Compact));
    QSettings().setValue("workspace/expandedSessions", QStringList{QStringLiteral("\ncodex/hgs/dashboard")});
    SessionsWindow window(program()); window.setFleet(state); window.show(); window.activateWindow();
    QVERIFY(QTest::qWaitForWindowActive(&window));
    window.showSession({}, "codex/hgs/dashboard");
    auto *list = window.findChild<SessionList *>("sessionList"); QVERIFY(list); QCOMPARE(list->count(), 4);
    QVERIFY(list->item(0)->data(SessionRoles::Header).toBool());
    QCOMPARE(list->item(2)->data(SessionRoles::ChildId).toString(), QString("agent-a"));
    QCOMPARE(list->currentRow(), 1);
    auto *composer = window.findChild<MessageComposer *>("messageComposer"); setComposerText(composer, "Draft for the dashboard");
    // From the message field the chords switch sessions; each session keeps its own draft.
    composer->editor()->setFocus(); QTRY_VERIFY(composer->editor()->hasFocus());
    pressCommand(&window, "session.next");
    QCOMPARE(currentSessionKey(list), QString("\nkimi/docs/research")); QCOMPARE(composer->sessionKey(), QString("\nkimi/docs/research"));
    QTRY_VERIFY(!composer->isDraftLoading(composer->sessionKey()));
    QCOMPARE(composer->editor()->toPlainText(), QString()); QVERIFY(composer->editor()->hasFocus());
    pressCommand(&window, "session.previous");
    QCOMPARE(currentSessionKey(list), QString("\ncodex/hgs/dashboard"));
    QTRY_COMPARE(composer->editor()->toPlainText(), QString("Draft for the dashboard")); QVERIFY(composer->editor()->hasFocus());
    list->setFocus(); QTRY_VERIFY(list->hasFocus());
    pressCommand(&window, "session.next");
    QCOMPARE(currentSessionKey(list), QString("\nkimi/docs/research"));
    pressCommand(&window, "session.next"); // The last session stays: no wrap.
    QCOMPARE(currentSessionKey(list), QString("\nkimi/docs/research"));
    pressCommand(&window, "session.previous", 1); // The second chord: Ctrl+PgUp, or Command-Option-Up on macOS.
    QCOMPARE(currentSessionKey(list), QString("\ncodex/hgs/dashboard")); QCOMPARE(list->currentRow(), 1);
    pressCommand(&window, "session.previous"); // The project header is no session.
    QCOMPARE(list->currentRow(), 1);
    QTRY_COMPARE(composer->editor()->toPlainText(), QString("Draft for the dashboard"));
    // From another page the chord shows Sessions and moves from the selection.
    window.findChild<QPushButton *>("brandMark")->click(); QVERIFY(!list->isVisible());
    pressCommand(&window, "session.next", 1);
    QVERIFY(list->isVisible()); QCOMPARE(currentSessionKey(list), QString("\nkimi/docs/research"));
}

void TestSessionsWindow::shortcutsFindSessionsNeedingAttention()
{
    auto box = fleet().local(); const auto base = box.sessions[1]; box.sessions.clear();
    for (const auto &[name, phase] : QList<QPair<QString, QString>>{{"codex/alpha/one", "approval"}, {"codex/beta/two", "idle"}, {"codex/gamma/three", "input"}}) {
        auto session = base; session.name = name; session.cmd = "codex"; session.project = name.section('/', 1, 1); session.tag = name.section('/', 2);
        session.phase = phase; box.sessions << session;
    }
    FleetState state; state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program()); window.setFleet(state); window.show(); window.activateWindow();
    QVERIFY(QTest::qWaitForWindowActive(&window));
    window.showSession({}, "codex/gamma/three");
    auto *list = window.findChild<SessionList *>("sessionList"); list->setFocus(); QTRY_VERIFY(list->hasFocus());
    pressCommand(&window, "session.nextAttention"); // Wraps to the first.
    QCOMPARE(currentSessionKey(list), QString("\ncodex/alpha/one"));
    pressCommand(&window, "session.nextAttention"); // Skips the idle session.
    QCOMPARE(currentSessionKey(list), QString("\ncodex/gamma/three"));
    pressCommand(&window, "session.previousAttention");
    QCOMPARE(currentSessionKey(list), QString("\ncodex/alpha/one"));
    pressCommand(&window, "session.previousAttention"); // Wraps to the last.
    QCOMPARE(currentSessionKey(list), QString("\ncodex/gamma/three"));
    for (auto &session : box.sessions) session.phase = "idle";
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    pressCommand(&window, "session.nextAttention");
    QCOMPARE(currentSessionKey(list), QString("\ncodex/gamma/three"));
    QCOMPARE(window.findChild<QLabel *>("notice")->text(), QString("No sessions need attention"));
}

void TestSessionsWindow::shortcutsCycleSessionTabs()
{
    for (const bool processes : {false, true}) {
        QSettings().setValue("processes/enabled", processes);
        SessionsWindow window(program()); window.setFleet(fleet()); window.show(); window.activateWindow();
        QVERIFY(QTest::qWaitForWindowActive(&window));
        window.showSession({}, "codex/hgs/dashboard");
        auto *tabs = window.findChild<QTabWidget *>("sessionDetailTabs"); auto *terminal = window.findChild<TerminalView *>();
        auto *processView = window.findChild<ProcessView *>(); QVERIFY(tabs); QVERIFY(terminal); QVERIFY(processView);
        QVERIFY(!tabs->isTabVisible(tabs->indexOf(window.findChild<QWidget *>("nativeUiView"))));
        QCOMPARE(tabs->isTabVisible(tabs->indexOf(processView)), processes);
        QCOMPARE(tabs->indexOf(window.findChild<TasksView *>()), -1); // The board lives in the side panel.
        const int taskView=tabs->indexOf(window.findChild<QWidget *>("taskDetailsPane")); QVERIFY(taskView>=0);
        QVERIFY(!tabs->isTabVisible(taskView));
        QList<QWidget *> order{tabs->widget(0), terminal}; if (processes) order << processView;
        auto *composer = window.findChild<MessageComposer *>("messageComposer");
        composer->editor()->setFocus(); QTRY_VERIFY(composer->editor()->hasFocus());
        for (int step = 1; step <= order.size(); ++step) {
            pressCommand(&window, "tab.next");
            QCOMPARE(tabs->currentWidget(), order[step % order.size()]); QVERIFY(focusedIn(tabs->currentWidget()));
        }
        for (int step = order.size() - 1; step >= 0; --step) {
            pressCommand(&window, "tab.previous");
            QCOMPARE(tabs->currentWidget(), order[step]); QVERIFY(focusedIn(tabs->currentWidget()));
        }
    }
}

void TestSessionsWindow::terminalChordReturnsToActivity()
{
    // A Control+Shift+letter chord stays with the agent even where a command uses it.
    auto &registry = ShortcutRegistry::instance();
    const QKeyCombination shiftLetter(physicalControl() | Qt::ShiftModifier, Qt::Key_K);
    registry.setKeys("go.overview", registry.keys("go.overview") << QKeySequence(shiftLetter));
    SessionsWindow window(program()); window.setFleet(fleet()); window.show(); window.activateWindow();
    QVERIFY(QTest::qWaitForWindowActive(&window));
    window.showSession({}, "codex/hgs/dashboard");
    auto *tabs = window.findChild<QTabWidget *>("sessionDetailTabs"); auto *terminal = window.findChild<TerminalView *>();
    auto *composer = window.findChild<MessageComposer *>("messageComposer"); auto *list = window.findChild<SessionList *>("sessionList");
    auto *pages = workspacePages(window); auto *screen = terminal->terminalScreen();
    tabs->setCurrentWidget(terminal); screen->setFocus(); QTRY_VERIFY(screen->hasFocus());
    QSignalSpy output(screen, &TerminalScreen::output);
    // A chord the window takes from Terminal leaves the keys after it to the agent.
    pressCommand(&window, "zoom.reset");
    QCOMPARE(window.findChild<QLabel *>("notice")->text(), QString("Content scale 100%"));
    QCOMPARE(tabs->currentWidget(), terminal); QVERIFY(screen->hasFocus()); QCOMPARE(output.size(), 0);
    // Keys an agent needs stay in Terminal, bound or not: Control with letters
    // (Ctrl+R also refreshes on Linux and Windows) and with Shift and letters
    // other than copy and paste, Alt chords, function keys, Escape and Enter.
    const QList<QKeyCombination> agentKeys{QKeyCombination(physicalControl(), Qt::Key_C), QKeyCombination(physicalControl(), Qt::Key_R), shiftLetter,
        QKeyCombination(Qt::AltModifier, Qt::Key_Up), Qt::Key_F2, Qt::Key_Escape, Qt::Key_Return};
    for (const auto &key : agentKeys) {
        const auto before = output.size();
        pressChord(&window, key.key(), key.keyboardModifiers());
        QVERIFY2(output.size() > before, qPrintable(QKeySequence(key).toString()));
        QCOMPARE(tabs->currentWidget(), terminal); QCOMPARE(currentSessionKey(list), QString("\ncodex/hgs/dashboard"));
        QCOMPARE(pages->currentIndex(), 0); QVERIFY(!window.findChild<QDialog *>("renameSessionDialog"));
    }
    pressCommand(&window, "tab.terminal");
    QCOMPARE(tabs->currentIndex(), 0); QTRY_VERIFY(composer->editor()->hasFocus());
    pressCommand(&window, "tab.terminal");
    QCOMPARE(tabs->currentWidget(), terminal); QTRY_VERIFY(screen->hasFocus());
    pressCommand(&window, "go.overview");
    QCOMPARE(pages->currentIndex(), 3);
}

void TestSessionsWindow::shortcutsInModalDialogsStayLocal()
{
    SessionsWindow window(program()); window.setFleet(fleet()); window.show(); window.activateWindow();
    QVERIFY(QTest::qWaitForWindowActive(&window));
    auto *pages = workspacePages(window); window.findChild<QPushButton *>("brandMark")->click(); QCOMPARE(pages->currentIndex(), 3);
    {
        QDialog dialog(&window); dialog.setModal(true); auto *field = new QLineEdit(&dialog);
        dialog.show(); dialog.activateWindow(); QVERIFY(QTest::qWaitForWindowActive(&dialog));
        field->setFocus(); QTRY_VERIFY(field->hasFocus());
        pressCommand(&dialog, "go.sessions");
        pressCommand(&dialog, "go.settings");
        QCOMPARE(pages->currentIndex(), 3);
    }
    window.activateWindow(); QVERIFY(QTest::qWaitForWindowActive(&window));
    pressCommand(&window, "go.sessions"); QCOMPARE(pages->currentIndex(), 0);
}

void TestSessionsWindow::zoomChordsChangeContentScale()
{
    SessionsWindow window(program()); window.setFleet(fleet()); window.show(); window.activateWindow();
    QVERIFY(QTest::qWaitForWindowActive(&window));
    window.showSession({}, "codex/hgs/dashboard");
    auto *composer = window.findChild<MessageComposer *>("messageComposer"); composer->editor()->setFocus(); QTRY_VERIFY(composer->editor()->hasFocus());
    auto *slider = window.findChild<QSlider *>("workspaceContentScale"); auto *value = window.findChild<QLabel *>("workspaceContentScaleValue");
    auto *notice = window.findChild<QLabel *>("notice"); QVERIFY(slider); QVERIFY(value);
    const auto editorPixels = [composer] { return QFontInfo(composer->editor()->font()).pixelSize(); };
    QCOMPARE(editorPixels(), 13);
    pressCommand(&window, "zoom.in");
    QCOMPARE(Settings::value("workspace/contentScale").toDouble(), 1.1);
    QCOMPARE(slider->value(), 22); QCOMPARE(value->text(), QString("110%")); QCOMPARE(notice->text(), QString("Content scale 110%"));
    QTRY_COMPARE(editorPixels(), 14);
    pressCommand(&window, "zoom.reset");
    QCOMPARE(Settings::value("workspace/contentScale").toDouble(), 1.0);
    QCOMPARE(slider->value(), 20); QCOMPARE(value->text(), QString("100%")); QTRY_COMPARE(editorPixels(), 13);
    for (int i = 0; i < 4; ++i) pressCommand(&window, "zoom.out");
    QCOMPARE(Settings::value("workspace/contentScale").toDouble(), 0.75); QCOMPARE(notice->text(), QString("Content scale 75%"));
}

void TestSessionsWindow::reboundChordsApplyAtOnce()
{
    SessionsWindow window(program()); window.setFleet(fleet()); window.show(); window.activateWindow();
    QVERIFY(QTest::qWaitForWindowActive(&window));
    window.showSession({}, "codex/hgs/dashboard");
    const auto native = [](const char *chord) { return QKeySequence::fromString(chord, QKeySequence::PortableText).toString(QKeySequence::NativeText); };
    auto *projects = railButton(window, "projects"); QVERIFY(projects);
    QCOMPARE(projects->toolTip(), QString("Projects (%1)").arg(native("Ctrl+3")));
    QCOMPARE(window.findChild<QPushButton *>("newSession")->toolTip(), QString("New session (%1)").arg(native("Ctrl+N")));
    auto *inspector = window.findChild<QPushButton *>("toggleInspector"); QVERIFY(inspector->toolTip().endsWith(" (" + native("Ctrl+I") + ")"));
    pressCommand(&window, "panel.toggle"); QVERIFY(inspector->isChecked());
    QCOMPARE(inspector->toolTip(), QString("Collapse side panel (%1)").arg(native("Ctrl+I")));
    QCOMPARE(inspector->accessibleName(), QString("Collapse side panel"));
    auto *tabs = window.findChild<QTabWidget *>("sessionDetailTabs"); auto *terminal = window.findChild<TerminalView *>();
    tabs->setCurrentWidget(terminal); terminal->terminalScreen()->setFocus(); QTRY_VERIFY(terminal->terminalScreen()->hasFocus());
    const auto previousTab = ShortcutRegistry::instance().keys("tab.next").value(0); QVERIFY(!previousTab.isEmpty());
    const auto portable = [](const char *chord) { return QKeySequence::fromString(chord, QKeySequence::PortableText); };
    ShortcutRegistry::instance().setKeys("tab.next", {portable("Ctrl+9")});
    ShortcutRegistry::instance().setKeys("go.projects", {portable("Ctrl+8")});
    QCOMPARE(window.findChild<QAction *>("shortcut:tab.next")->shortcuts(), QList<QKeySequence>{portable("Ctrl+9")});
    QCOMPARE(projects->toolTip(), QString("Projects (%1)").arg(native("Ctrl+8")));
    pressSequence(&window, previousTab); // No longer bound: Terminal keeps it.
    QCOMPARE(tabs->currentWidget(), terminal);
    pressCommand(&window, "tab.next");
    QCOMPARE(tabs->currentWidget(), tabs->widget(0));
}

void TestSessionsWindow::projectListKeepsItsReorderKeys()
{
    SessionOrganization projects; projects.createGroup("infra"); projects.createGroup("sample-app");
    QSettings().setValue("workspace/hideEmptyProjects", false);
    setJsonSetting("workspace/organization", QJsonDocument(projects.toJson()).toJson(QJsonDocument::Compact));
    const auto stored = [] { return SessionOrganization(QJsonDocument::fromJson(jsonSetting("workspace/organization")).object()); };
    SessionsWindow window(program()); window.setFleet(fleet()); window.show(); window.activateWindow();
    QVERIFY(QTest::qWaitForWindowActive(&window));
    window.showProjects();
    auto *pages = workspacePages(window); auto *page = window.findChild<ProjectsDialog *>();
    auto *list = page->findChild<QListWidget *>("logicalProjects"); QVERIFY(list); QVERIFY(list->count() > 2);
    list->setCurrentRow(1); list->setFocus(); QTRY_VERIFY(list->hasFocus());
    const auto moved = stored().groups()[1].id;
    // Alt+Up and Alt+Down are the list's own keys, although Next and Previous session use them elsewhere.
    QCOMPARE(ShortcutRegistry::instance().keys("session.next").value(0), QKeySequence(Qt::ALT | Qt::Key_Down));
    pressChord(&window, Qt::Key_Up, Qt::AltModifier);
    QCOMPARE(stored().groups()[0].id, moved); QCOMPARE(pages->currentIndex(), 1);
    list->setCurrentRow(list->count() - 1);
    pressChord(&window, Qt::Key_Down, Qt::AltModifier); // The last project stays last.
    QCOMPARE(pages->currentIndex(), 1);
    auto *other = page->findChild<QCheckBox *>("allSwarmProjects"); other->setFocus(); QTRY_VERIFY(other->hasFocus());
    pressCommand(&window, "session.next");
    QCOMPARE(pages->currentIndex(), 0);
}

void TestSessionsWindow::focusChordsCycleSessionAreas()
{
    SessionsWindow window(program()); window.setFleet(fleet()); window.show(); window.activateWindow();
    QVERIFY(QTest::qWaitForWindowActive(&window));
    window.showSession({}, "codex/hgs/dashboard");
    auto *list = window.findChild<SessionList *>("sessionList"); auto *activity = window.findChild<ActivityView *>("mainActivity");
    auto *composer = window.findChild<MessageComposer *>("messageComposer"); auto *panel = window.findChild<QFrame *>("sessionInspectorPanel");
    pressCommand(&window, "panel.toggle"); QTRY_VERIFY(panel->isVisible());
    list->setFocus(); QTRY_VERIFY(list->hasFocus());
    // Session list, the current view, the message field, the side panel, and around.
    const QList<const QWidget *> areas{list, activity->browser(), composer->editor(), panel};
    for (int step = 1; step <= areas.size(); ++step) {
        pressCommand(&window, "focus.next");
        QVERIFY2(focusedIn(areas[step % areas.size()]), qPrintable(QString::number(step)));
    }
    for (int step = areas.size() - 1; step >= 0; --step) {
        pressCommand(&window, "focus.previous");
        QVERIFY2(focusedIn(areas[step]), qPrintable(QString::number(step)));
    }
    // Other pages have no such areas.
    window.findChild<QPushButton *>("brandMark")->click(); auto *focus = QApplication::focusWidget();
    pressCommand(&window, "focus.next"); QCOMPARE(QApplication::focusWidget(), focus);
}

void TestSessionsWindow::goToOpensAndActivatesItems()
{
    SessionsWindow window(program()); window.setFleet(fleet()); window.show(); window.activateWindow();
    QVERIFY(QTest::qWaitForWindowActive(&window));
    auto *pages = workspacePages(window); QVERIFY(pages);
    auto *sessions = window.findChild<SessionList *>(); QVERIFY(sessions);
    pressCommand(&window, "go.anywhere");
    auto *goTo = window.findChild<GoToDialog *>("goToDialog"); QVERIFY(goTo);
    QTRY_VERIFY(goTo->isVisible());
    auto *results = goTo->findChild<QListWidget *>("goToResults"); QVERIFY(results);
    QStringList titles; QList<GoToItem> items;
    for (int row = 0; row < results->count(); ++row) {
        const auto item = results->item(row)->data(Qt::UserRole).value<GoToItem>();
        items << item; titles << item.title;
    }
    QVERIFY(titles.contains("hgs / dashboard"));
    QVERIFY(titles.contains("infra / review"));
    QVERIFY(titles.contains("Next session"));
    QVERIFY(titles.contains("Projects"));
    QVERIFY(titles.contains("mac"));
    // Message-field commands and Go to itself do not apply from the popup.
    QVERIFY(!titles.contains("Send message"));
    QVERIFY(!titles.contains("Go to…"));
    const auto find = [&](GoToItem::Kind kind, const QString &title) {
        for (const auto &item : items) if (item.kind == kind && item.title == title) return item;
        return GoToItem{};
    };
    goTo->hide();
    // Hiding the popup directly does not hand activation back on every
    // platform plugin (macOS offscreen); window chords need the active window.
    window.activateWindow(); QVERIFY(QTest::qWaitForWindowActive(&window));
    // A session on another computer opens and becomes the selection.
    const auto review = find(GoToItem::Session, "infra / review"); QCOMPARE(review.id, QString("mac\nclaude/infra/review"));
    emit goTo->activated(review);
    QCOMPARE(pages->currentIndex(), 0);
    QCOMPARE(currentSessionKey(sessions), review.id);
    // A section and a command run like their chords.
    emit goTo->activated(find(GoToItem::Section, "Projects"));
    QCOMPARE(pages->currentIndex(), 1);
    emit goTo->activated(find(GoToItem::Section, "Sessions"));
    auto *toggle = window.findChild<QPushButton *>("toggleInspector"); QVERIFY(toggle);
    const bool panel = toggle->isChecked();
    emit goTo->activated(find(GoToItem::Command, "Show or hide the side panel"));
    QCOMPARE(toggle->isChecked(), !panel);
    // A project without sessions opens Projects.
    GoToItem empty; empty.kind = GoToItem::Project; empty.id = "no-such-project";
    emit goTo->activated(empty);
    QCOMPARE(pages->currentIndex(), 1);
    // Recently opened sessions come first with an empty query.
    window.activateWindow(); QVERIFY(QTest::qWaitForWindowActive(&window));
    pressCommand(&window, "go.sessions");
    pressCommand(&window, "go.anywhere");
    QTRY_VERIFY(goTo->isVisible());
    QVERIFY(results->count() > 0);
    QCOMPARE(results->item(0)->data(Qt::UserRole).value<GoToItem>().id, review.id);
    // Go to's own chord closes the popup again.
    pressSequence(goTo->findChild<QLineEdit *>("goToQuery"), ShortcutRegistry::instance().keys("go.anywhere").value(0), false);
    QTRY_VERIFY(!goTo->isVisible());
}

void TestSessionsWindow::cheatSheetOpensKeyboardSettings()
{
    SessionsWindow window(program()); window.setFleet(fleet()); window.show(); window.activateWindow();
    QVERIFY(QTest::qWaitForWindowActive(&window));
    pressCommand(&window, "help.shortcuts");
    auto *help = window.findChild<ShortcutsDialog *>("shortcutsDialog"); QVERIFY(help);
    QTRY_VERIFY(help->isVisible());
    auto *customize = help->findChild<QPushButton *>("shortcutsCustomize"); QVERIFY(customize);
    customize->click();
    QVERIFY(!help->isVisible());
    auto *pages = workspacePages(window); QVERIFY(pages); QCOMPARE(pages->currentIndex(), 5);
    auto *sections = window.findChild<QListWidget *>("settingsSections"); QVERIFY(sections);
    QCOMPARE(sections->currentItem()->text(), QString("Keyboard"));
}

void TestSessionsWindow::renameChordWorksOnTheSessionsPage()
{
    SessionsWindow window(program()); window.setFleet(fleet()); window.show(); window.activateWindow();
    QVERIFY(QTest::qWaitForWindowActive(&window));
    window.showSession({}, "codex/hgs/dashboard");
    int dialogs = 0;
    QTimer close; close.setInterval(10);
    connect(&close, &QTimer::timeout, &window, [&] {
        auto *dialog = window.findChild<QDialog *>("renameSessionDialog");
        if (dialog && dialog->isVisible()) { ++dialogs; dialog->reject(); }
    });
    close.start();
    // Like the tab and focus commands, renaming belongs to the Sessions page.
    auto *pages = workspacePages(window); window.findChild<QPushButton *>("brandMark")->click(); QCOMPARE(pages->currentIndex(), 3);
    pressCommand(&window, "session.rename"); QTest::qWait(100);
    QCOMPARE(dialogs, 0); QCOMPARE(pages->currentIndex(), 3);
    pressCommand(&window, "go.sessions");
    pressCommand(&window, "session.rename");
    QTRY_COMPARE(dialogs, 1);
}

void TestSessionsWindow::attentionChordsStayInSearchResults()
{
    auto box = fleet().local(); const auto base = box.sessions[1]; box.sessions.clear();
    for (const auto &[name, phase] : QList<QPair<QString, QString>>{{"codex/alpha/one", "approval"}, {"claude/delta/four", "approval"},
             {"codex/beta/two", "idle"}, {"codex/gamma/three", "input"}}) {
        auto session = base; session.name = name; session.cmd = name.section('/', 0, 0); session.project = name.section('/', 1, 1);
        session.tag = name.section('/', 2); session.phase = phase; box.sessions << session;
    }
    FleetState state; state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program()); window.setFleet(state); window.show(); window.activateWindow();
    QVERIFY(QTest::qWaitForWindowActive(&window));
    window.showSession({}, "codex/gamma/three");
    // The Claude session needs attention too, but the search results do not show it.
    window.findChild<QLineEdit *>("search")->setText("codex");
    auto *results = window.findChild<QListWidget *>("searchResults"); QTRY_VERIFY(results->isVisible()); QCOMPARE(results->count(), 3);
    const auto name = [](const QListWidgetItem *item) { return item ? item->data(Qt::UserRole).toString().section('\n', 1, 1) : QString(); };
    for (int row = 0; row < results->count(); ++row) if (name(results->item(row)) == "codex/gamma/three") results->setCurrentRow(row);
    QCOMPARE(name(results->currentItem()), QString("codex/gamma/three"));
    results->setFocus(); QTRY_VERIFY(results->hasFocus());
    auto *composer = window.findChild<MessageComposer *>("messageComposer");
    pressCommand(&window, "session.nextAttention"); // Wraps to the first result.
    QCOMPARE(name(results->currentItem()), QString("codex/alpha/one")); QVERIFY(results->isVisible());
    QCOMPARE(composer->sessionKey(), QString("\ncodex/alpha/one"));
    pressCommand(&window, "session.nextAttention"); // Skips the idle session.
    QCOMPARE(name(results->currentItem()), QString("codex/gamma/three"));
    pressCommand(&window, "session.previousAttention");
    QCOMPARE(name(results->currentItem()), QString("codex/alpha/one"));
    pressCommand(&window, "session.previousAttention"); // Wraps to the last.
    QCOMPARE(name(results->currentItem()), QString("codex/gamma/three")); QVERIFY(results->isVisible());
    QCOMPARE(composer->sessionKey(), QString("\ncodex/gamma/three"));
    // Results without such a session say so.
    window.findChild<QLineEdit *>("search")->setText("beta");
    QTRY_COMPARE(results->count(), 1);
    pressCommand(&window, "session.nextAttention");
    QCOMPARE(notice(window), QString("No sessions need attention"));
}

void TestSessionsWindow::goToFollowsTheSessionList()
{
    // Projects order the session list unlike the sessions' keys.
    SessionOrganization org; const auto first = org.createGroup("First"), second = org.createGroup("Second");
    const auto empty = org.createGroup("Empty project");
    org.moveSession(org.observe("mac", "claude/infra/review", {}), first);
    org.moveSession(org.observe("arch", "codex/website/navigation", {}), first);
    org.moveSession(org.observe("arch", "codex/hgs/dashboard", {}), second);
    org.moveSession(org.observe("arch", "kimi/docs/research", {}), second);
    setJsonSetting("workspace/organization", QJsonDocument(org.toJson()).toJson(QJsonDocument::Compact));
    QSettings().setValue("workspace/hideEmptyProjects", false);
    SessionsWindow window(program()); window.setFleet(fleet()); window.show(); window.activateWindow();
    QVERIFY(QTest::qWaitForWindowActive(&window));
    auto *list = window.findChild<SessionList *>("sessionList");
    QStringList listed;
    for (int row = 0; row < list->count(); ++row) {
        const auto *item = list->item(row);
        if (!item->data(SessionRoles::Header).toBool() && item->data(SessionRoles::ChildId).toString().isEmpty())
            listed << item->data(SessionRoles::Key).toString();
    }
    pressCommand(&window, "go.anywhere");
    auto *goTo = window.findChild<GoToDialog *>("goToDialog"); QVERIFY(goTo); QTRY_VERIFY(goTo->isVisible());
    auto *results = goTo->findChild<QListWidget *>("goToResults"); auto *query = goTo->findChild<QLineEdit *>("goToQuery");
    QStringList others; GoToItem project;
    for (int row = 0; row < results->count(); ++row) {
        const auto item = results->item(row)->data(Qt::UserRole).value<GoToItem>();
        if (item.kind == GoToItem::Session && item.recency < 0) others << item.id;
        if (item.kind == GoToItem::Project && item.id == empty) project = item;
    }
    // Without recency, sessions follow the session list; those it does not show come last.
    QStringList expected; for (const auto &key : listed) if (others.contains(key)) expected << key;
    QVERIFY(expected.size() >= 3);
    auto sorted = expected; std::sort(sorted.begin(), sorted.end()); QVERIFY(expected != sorted);
    QCOMPARE(others.mid(0, expected.size()), expected);
    // A leading ">" lists sections and commands.
    query->setText(">"); QVERIFY(results->count() > 0);
    bool section = false;
    for (int row = 0; row < results->count(); ++row) {
        const auto kind = results->item(row)->data(Qt::UserRole).value<GoToItem>().kind;
        QVERIFY(kind == GoToItem::Section || kind == GoToItem::Command); section |= kind == GoToItem::Section;
    }
    QVERIFY(section);
    query->setText(">proj"); QCOMPARE(results->item(0)->data(Qt::UserRole).value<GoToItem>().title, QString("Projects"));
    // A project without sessions opens Projects with that project selected.
    QCOMPARE(project.title, QString("Empty project"));
    goTo->hide(); emit goTo->activated(project);
    QCOMPARE(workspacePages(window)->currentIndex(), 1);
    auto *projects = window.findChild<ProjectsDialog *>()->findChild<QListWidget *>("logicalProjects"); QVERIFY(projects->currentItem());
    QCOMPARE(projects->currentItem()->data(Qt::UserRole).toString(), empty);
    QVERIFY(projects->currentRow() > 0);
}

QTEST_MAIN(TestSessionsWindow)
#include "test_sessionswindow_shortcuts.moc"
