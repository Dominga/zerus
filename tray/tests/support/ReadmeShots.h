#pragma once

#include <QJsonArray>
#include <QJsonObject>
#include <QString>

#include "FleetState.h"
#include "HgsClient.h"

// WhiteWh fork: the synthetic workspace of the README pictures: the Orbit
// platform project on a Windows workstation, a MacBook and two Linux servers, an
// agent team, accounts and the conversations that the fake hgs answers.
namespace ReadmeShots {
inline QJsonObject event(int seq, double at, const QString &type, const QString &detail, const QString &tool = {})
{
    return {{"seq", seq}, {"at", at}, {"type", type}, {"detail", detail}, {"tool", tool}};
}

inline SessionInfo session(const QString &cmd, const QString &project, const QString &tag, const QString &model,
                    const QString &cwd, const QString &branch, const QString &prompt, double now)
{
    SessionInfo s;
    s.name = cmd + '/' + project + '/' + tag; s.cmd = cmd; s.project = project; s.tag = tag;
    s.tracked = true; s.resumable = true; s.model = model; s.effort = "high";
    s.cwd = cwd; s.gitBranch = branch; s.prompt = prompt;
    s.runId = "run-" + tag; s.conversationId = "conversation-" + tag;
    s.runtimeState = "live"; s.processState = "running"; s.activity = "idle"; s.phase = "idle";
    s.created = qint64(now) - 3 * 3600; s.lastEventAt = now - 240;
    return s;
}

inline void working(SessionInfo &s, const QString &tool, const QString &detail, int seconds, double now)
{
    s.activity = "busy"; s.phase = "tool"; s.currentTool = tool; s.toolDetail = detail;
    s.turnStarted = now - seconds; s.lastEventAt = now - 3;
}

inline QJsonObject metrics(const QString &platform, double cpu, int cores, double load, double used, double total, double now)
{
    const double gib = 1024. * 1024. * 1024.;
    QJsonObject m{{"platform", platform}, {"cpu_percent", cpu}, {"cpu_count", cores}, {"cpu_interval_seconds", 2.0},
                  {"memory_used_bytes", used * gib}, {"memory_total_bytes", total * gib}, {"sampled_at", now - 4}};
    if (load >= 0) m["load_1"] = load;   // Windows reports no load average
    return m;
}

// The New team picture needs its coordinator in an existing Git folder; it and
// the Team designer's session join only then.
inline FleetState fleet(double now, const QString &coordinatorFolder = {})
{
    BoxState ws; ws.host = "workstation"; ws.ok = true; ws.metrics = metrics("windows", 34, 16, -1, 27.6, 64, now);
    auto lead = session("claude", "orbit", "checkout", "claude-opus-4-6", "D:\\work\\orbit", "feat/checkout-v2",
                        "Lead the Checkout v2 team.", now);
    working(lead, "Bash", "pnpm test checkout/address", 48, now);
    auto backend = session("dsh", "orbit", "checkout-api", "deepseek-v4-pro", "D:\\work\\orbit-team\\backend", "team/checkout-v2/backend",
                           "#2 Saved cards API", now);
    backend.gitWorktree = true; backend.gitCommonDir = "D:\\work\\orbit\\.git";
    working(backend, "Bash", "cargo test -p payments saved_cards", 263, now);
    auto release = session("claude", "orbit", "release", "claude-sonnet-4-6", coordinatorFolder.isEmpty() ? "D:\\work\\orbit" : coordinatorFolder,
                           "main", "Draft the 2.4 release notes from the merged changes.", now);
    auto search = session("codex", "orbit", "search", "gpt-6.1-sol", "D:\\work\\orbit", "feat/search-index",
                          "Index product names for checkout search.", now);
    working(search, "Edit", "web/search/index.ts", 95, now);
    ws.sessions = {lead, backend, search, release};
    if (!coordinatorFolder.isEmpty()) {
        auto designer = session("claude", "team-designer", "team-designer", "claude-opus-4-6", "D:\\work\\team-designer", {},
                                "Help me write the goal and the team protocol for a new team.", now);
        ws.sessions.append(designer);
    }

    BoxState mac; mac.host = "macbook"; mac.ok = true; mac.metrics = metrics("macos", 22, 12, 2.41, 18.2, 36, now);
    auto frontend = session("codex", "orbit", "checkout-ui", "gpt-6.1-sol", "/Users/alice/work/orbit-team/frontend", "team/checkout-v2/frontend",
                            "#3 Card form UI", now);
    frontend.gitWorktree = true; frontend.gitCommonDir = "/Users/alice/work/orbit/.git";
    working(frontend, "Edit", "web/checkout/CardForm.tsx", 131, now);
    auto ios = session("claude", "orbit", "ios-shell", "claude-sonnet-4-6", "/Users/alice/work/orbit", "feat/ios-wallet",
                       "Add Apple Pay to the iOS checkout shell.", now);
    ios.unreadReply = true; ios.replyId = "reply-ios"; ios.replyAt = now - 540; ios.lastEventAt = now - 540;
    mac.sessions = {frontend, ios};

    BoxState b1; b1.host = "build-01"; b1.ok = true; b1.metrics = metrics("linux", 58, 32, 9.84, 41.0, 128, now);
    auto tester = session("kimi", "orbit", "checkout-tests", "kimi-k2.5", "/srv/work/orbit-team/tester", "team/checkout-v2/tester",
                          "Waiting for #2 Saved cards API", now);
    tester.gitWorktree = true; tester.gitCommonDir = "/srv/work/orbit/.git";
    auto docs = session("opencode", "orbit", "api-docs", "qwen3-coder", "/srv/work/orbit", "docs/saved-cards",
                        "Document the saved cards endpoints.", now);
    working(docs, "Write", "docs/api/saved-cards.md", 72, now);
    auto terraform = session("codex", "infra", "staging", "gpt-6.1-sol", "/srv/work/infra", "main",
                             "Plan the staging database upgrade.", now);
    b1.sessions = {tester, docs, terraform};

    BoxState b2; b2.host = "build-02"; b2.ok = true; b2.metrics = metrics("linux", 71, 32, 18.6, 77.5, 128, now);
    auto review = session("dsh", "orbit", "review", "deepseek-v4-pro", "/srv/work/orbit", "release/2.4",
                          "Review the payments changes before tonight's release.", now);
    review.activity = "waiting"; review.phase = "input"; review.attentionId = "dsh-review-scope"; review.lastEventAt = now - 35;
    auto refactor = session("codex", "orbit", "api-refactor", "gpt-6.1-sol", "/srv/work/orbit", "feat/api-v3",
                            "Move the order handlers to the v3 router.", now);
    working(refactor, "Bash", "cargo test --workspace", 388, now);
    auto migrations = session("claude", "orbit", "migrations", "claude-sonnet-4-6", "/srv/work/orbit", "feat/ledger-index",
                              "Add the ledger index migration.", now);
    migrations.state = "paused";
    b2.sessions = {review, refactor, migrations};

    FleetState state; const auto ms = qint64(now * 1000);
    state.setLocal(ws, ms); state.setPeer(mac, ms); state.setPeer(b1, ms); state.setPeer(b2, ms);
    return state;
}

inline QJsonObject organization()
{
    const auto folder = [](const QString &id, const QString &machine, const QString &path, const QString &name) {
        return QJsonObject{{"id", id}, {"machine", machine}, {"path", path}, {"name", name}};
    };
    const QStringList orbit{"workstation\nclaude/orbit/checkout", "workstation\ncodex/orbit/search", "build-02\ndsh/orbit/review",
        "build-02\ncodex/orbit/api-refactor", "macbook\nclaude/orbit/ios-shell", "build-01\nopencode/orbit/api-docs",
        "workstation\nclaude/orbit/release", "build-02\nclaude/orbit/migrations", "workstation\ndsh/orbit/checkout-api",
        "macbook\ncodex/orbit/checkout-ui", "build-01\nkimi/orbit/checkout-tests"};
    const QJsonArray projects{
        QJsonObject{{"id", "orbit"}, {"name", "Orbit platform"}, {"color", "#64b5f6"}, {"sessions", QJsonArray::fromStringList(orbit)},
            {"folders", QJsonArray{folder("orbit-ws", "workstation", "D:/work/orbit", "API and web app"),
                                   folder("orbit-docs", "workstation", "D:/work/orbit-docs", "Developer docs"),
                                   folder("orbit-mac", "macbook", "/Users/alice/work/orbit", "macOS and iOS clients"),
                                   folder("orbit-b1", "build-01", "/srv/work/orbit", "Integration tests"),
                                   folder("orbit-b2", "build-02", "/srv/work/orbit", "Release builds")}}},
        QJsonObject{{"id", "infra"}, {"name", "Infrastructure"}, {"color", "#ffb74d"},
            {"sessions", QJsonArray{"build-01\ncodex/infra/staging"}},
            {"folders", QJsonArray{folder("infra-b1", "build-01", "/srv/work/infra", "Staging")}}},
        QJsonObject{{"id", "research"}, {"name", "Docs & research"}, {"color", "#b39ddb"},
            {"folders", QJsonArray{folder("research-mac", "macbook", "/Users/alice/notes", "Notes")}}},
        QJsonObject{{"id", "ungrouped"}, {"name", "General"}, {"color", "#8d9baa"},
            {"sessions", QJsonArray{"workstation\nclaude/team-designer/team-designer"}}}};
    return {{"version", 2}, {"default_project", "orbit"}, {"projects", projects},
            {"imported_machines", QJsonArray{"workstation", "macbook", "build-01", "build-02"}}};
}

inline QJsonObject team(double now)
{
    const auto member = [](const QString &id, const QString &role, const QString &agent, const QString &model, const QString &host, const QString &name) {
        QJsonObject m{{"id", id}, {"role", role}, {"agent", agent}, {"model", model}, {"state", "active"},
                      {"session", QJsonObject{{"host", host}, {"name", name}}}};
        if (!host.isEmpty()) m["computer"] = host;
        return m;
    };
    const auto deps = [](std::initializer_list<int> ids) { QJsonArray a; for (int id : ids) a.append(QJsonObject{{"task", id}}); return a; };
    const auto attempt = [now](const QString &id, const QString &by, const QString &state, int ago, const QString &report = {}) {
        QJsonObject a{{"n", 1}, {"id", id}, {"member", by}, {"state", state}, {"sent_at", now - ago}};
        if (!report.isEmpty()) a["report"] = QJsonObject{{"text", report}, {"at", now - ago + 600}};
        return a;
    };
    const QJsonArray tasks{
        QJsonObject{{"id", 1}, {"title", "Checkout plan"}, {"assignee", "lead"}, {"status", "accepted"},
                    {"attempts", QJsonArray{attempt("a1", "lead", "reported", 3300, "Plan approved by the owner")}}},
        QJsonObject{{"id", 2}, {"title", "Saved cards API"}, {"assignee", "backend"}, {"status", "in_progress"}, {"deps", deps({1})},
                    {"criteria", QJsonArray{"List, add and remove cards", "Card numbers never stored"}},
                    {"attempts", QJsonArray{attempt("a2", "backend", "delivered", 2400)}}},
        QJsonObject{{"id", 3}, {"title", "Card form UI"}, {"assignee", "frontend"}, {"status", "in_progress"}, {"deps", deps({1})},
                    {"attempts", QJsonArray{attempt("a3", "frontend", "delivered", 1500)}}},
        QJsonObject{{"id", 4}, {"title", "Address validation"}, {"assignee", "frontend"}, {"status", "submitted"}, {"deps", deps({1})},
                    {"criteria", QJsonArray{"Postal codes checked per country", "Keyboard order kept"}},
                    {"attempts", QJsonArray{attempt("a4", "frontend", "reported", 2700, "Validation and messages are in web/checkout/address")}}},
        QJsonObject{{"id", 5}, {"title", "Contract tests"}, {"assignee", "tester"}, {"status", "pending"}, {"deps", deps({2})}},
        QJsonObject{{"id", 6}, {"title", "Checkout E2E"}, {"assignee", "tester"}, {"status", "pending"}, {"deps", deps({3, 5})}},
        QJsonObject{{"id", 7}, {"title", "Release notes"}, {"assignee", "lead"}, {"status", "pending"},
                    {"deps", QJsonArray{QJsonObject{{"task", 6}}, QJsonObject{{"task", 4}, {"kind", "soft"}}}}}};
    return {{"id", "team-checkout"}, {"name", "Checkout v2"}, {"status", "running"}, {"project", "Orbit platform"},
            {"folder", "D:\\work\\orbit"}, {"lead", "lead"}, {"revision", 9}, {"created", now - 3600}, {"updated", now - 60},
            {"settings", QJsonObject{}},
            {"members", QJsonArray{member("lead", "Lead", "claude", "claude-opus-4-6", {}, "claude/orbit/checkout"),
                                   member("backend", "Backend", "dsh", "deepseek-v4-pro", {}, "dsh/orbit/checkout-api"),
                                   member("frontend", "Frontend", "codex", "gpt-6.1-sol", "macbook", "codex/orbit/checkout-ui"),
                                   member("tester", "Tester", "kimi", "kimi-k2.5", "build-01", "kimi/orbit/checkout-tests")}},
            {"tasks", tasks},
            {"layers", QJsonObject{{"1", 0}, {"2", 1}, {"3", 1}, {"4", 1}, {"5", 2}, {"6", 3}, {"7", 4}}},
            {"cycle", QJsonArray{}}, {"ready", QJsonArray{}}, {"validation", QJsonObject{{"errors", QJsonArray{}}, {"warnings", QJsonArray{}}}}};
}

inline QJsonObject feed(double now)
{
    QJsonArray items;
    const auto add = [&](int seq, int ago, const QString &kind, const QJsonObject &detail) {
        items.prepend(QJsonObject{{"seq", seq}, {"at", now - ago}, {"kind", kind}, {"detail", detail}});
    };
    add(1, 3600, "created", {{"by", "Owner"}});
    add(2, 3400, "approved", {{"by", "Owner"}});
    add(3, 3300, "accepted", {{"by", "Lead"}, {"task", 1}});
    add(4, 2400, "handed_out", {{"to", "Backend"}, {"task", 2}, {"attempt", 1}});
    add(5, 2700, "handed_out", {{"to", "Frontend"}, {"task", 4}, {"attempt", 1}});
    add(6, 2100, "reported", {{"by", "Frontend"}, {"task", 4}, {"attempt", 1}});
    add(7, 1500, "handed_out", {{"to", "Frontend"}, {"task", 3}, {"attempt", 1}});
    return {{"items", items}, {"next_cursor", QJsonValue()}};
}

inline QJsonObject catalog()
{
    const auto role = [](const QString &id, const QString &name, const QString &agent, const QString &model, const QString &effort,
                         const QString &folder, const QString &prompt) {
        return QJsonObject{{"id", id}, {"name", name}, {"agent", agent}, {"model", model}, {"effort", effort}, {"folder", folder}, {"prompt", prompt}};
    };
    const auto seat = [](const QString &id) { return QJsonObject{{"role", id}, {"count", 1}}; };
    const auto step = [](const QString &title, const QString &id, QJsonArray deps) { return QJsonObject{{"title", title}, {"role", id}, {"deps", deps}}; };
    const QJsonArray roles{
        role("lead", "Lead", "claude", "claude-opus-4-6", "high", "shared", "You lead the team: plan, hand out and review."),
        role("backend", "Backend", "dsh", "deepseek-v4-pro", "high", "worktree", "Build and test the server side."),
        role("frontend", "Frontend", "codex", "gpt-6.1-sol", "medium", "worktree", "Build the interface."),
        role("tester", "Tester", "kimi", "kimi-k2.5", "medium", "worktree", "Write and run the tests."),
        role("reviewer", "Reviewer", "claude", "claude-sonnet-4-6", "high", "read_only", "Review changes; edit nothing.")};
    const QJsonArray templates{
        QJsonObject{{"id", "feature"}, {"name", "Feature delivery"}, {"description", "API, interface and tests in two waves"},
                    {"lead_protocol", "Plan two waves: the API and the form first, the tests after."},
                    {"roster", QJsonArray{seat("backend"), seat("frontend"), seat("tester")}},
                    {"tasks", QJsonArray{step("Plan", "lead", {}), step("API", "backend", {0}), step("Interface", "frontend", {0}), step("Tests", "tester", {1, 2})}},
                    {"planning", "template"}, {"approval", "review"}, {"max_members", 6}},
        QJsonObject{{"id", "bugfix"}, {"name", "Bug hunt"}, {"description", "Reproduce, fix and prove it with a test"},
                    {"lead_protocol", "Reproduce first; accept a fix only with a failing test that now passes."},
                    {"roster", QJsonArray{seat("backend"), seat("tester")}}, {"tasks", QJsonArray{}}, {"planning", "lead"}, {"approval", "review"}, {"max_members", 4}},
        QJsonObject{{"id", "review"}, {"name", "Review pass"}, {"description", "Two read-only reviewers before a release"},
                    {"lead_protocol", "Split the diff by area; merge the findings into one list."},
                    {"roster", QJsonArray{seat("reviewer"), QJsonObject{{"role", "reviewer"}, {"count", 1}}}}, {"tasks", QJsonArray{}},
                    {"planning", "lead"}, {"approval", "auto"}, {"max_members", 4}}};
    return {{"builtin", false}, {"catalog", QJsonObject{{"version", 4}, {"roles", roles}, {"templates", templates}, {"routing", QJsonArray{}}}},
            {"draft", QJsonObject{{"base", 4}, {"proposals", QJsonArray{}}}}};
}

inline QJsonObject accounts(double now)
{
    const auto profile = [](const QString &id, const QString &provider, const QString &label) {
        return QJsonObject{{"id", id}, {"provider", provider}, {"label", label}, {"home", "/profiles/" + id}, {"native", false},
                           {"installed", true}, {"is_default", true}};
    };
    const auto claude = profile("orbit-claude", "claude", "Orbit engineering"), codex = profile("orbit-codex", "codex", "Orbit Codex");
    const auto kimi = profile("orbit-kimi", "kimi", "Kimi research"), dsh = profile("orbit-deepseek", "dsh", "DeepSeek API");
    const auto local = profile("local-models", "opencode", "Local models");
    const auto window = [now](int minutes, double used, int resetIn) {
        return QJsonObject{{"window_minutes", minutes}, {"used_percent", used}, {"resets_at", now + resetIn}};
    };
    const auto usage = [now](const QString &email, const QString &account, const QString &plan, const QJsonArray &windows) {
        return QJsonObject{{"status", "ok"}, {"checked_at", now - 30}, {"windows", windows},
                           {"identity", QJsonObject{{"email", email}, {"account_id", account}, {"plan", plan}}}};
    };
    return {{"profiles", QJsonObject{{"", QJsonArray{claude, codex, dsh}}, {"macbook", QJsonArray{claude, codex}},
                                     {"build-01", QJsonArray{codex, kimi, local}}, {"build-02", QJsonArray{claude, codex, dsh}}}},
            {"usage", QJsonObject{
                {"orbit-claude", usage("eng@example.com", "acct-claude", "Team", {window(300, 34, 6200), window(10080, 18, 302400)})},
                {"orbit-codex", usage("ci@example.com", "acct-codex", "Pro", {window(300, 62, 4100), window(10080, 41, 190000)})},
                {"orbit-kimi", usage("research@example.com", "acct-kimi", "Moderato", {window(10080, 27, 400000)})},
                {"orbit-deepseek", QJsonObject{{"status", "ok"}, {"checked_at", now - 30},
                    {"balances", QJsonArray{QJsonObject{{"kind", "Topped up"}, {"balance", "18.40"}, {"currency", "USD"}}}},
                    {"identity", QJsonObject{{"account_id", "acct-deepseek"}, {"plan", "Pay as you go"}}}}},
                {"local-models", QJsonObject{{"status", "configured"}, {"checked_at", now - 30},
                    {"identity", QJsonObject{{"auth_method", "Ollama"}, {"plan", "qwen3-coder"}}}}}}}};
}

inline QJsonObject details(const SessionInfo &s, const QJsonArray &events)
{
    return {{"tracked", true}, {"run_id", s.runId}, {"conversation_id", s.conversationId}, {"runtime_state", "live"},
            {"process_state", "running"}, {"activity", s.activity}, {"phase", s.phase}, {"current_tool", s.currentTool},
            {"tool_detail", s.toolDetail}, {"events", events}, {"cursor", events.size()}};
}

// The new team's goal and protocol, as the Team designer drafts them.
constexpr char goal[] = "Customers can list, add and remove saved cards at checkout. Card numbers stay with the payment provider.\n"
                         "Done when the API, the card form and the contract tests pass on staging.";
constexpr char protocol[] = "Two waves: the saved cards API and the card form first, the tests after.\n"
                            "Accept a report only against its criteria; send unclear work back with one concrete request.\n"
                            "Start the second wave when both first-wave tasks are accepted.";

// What `hgs inspect` answers, keyed by "<host>\n<name>" (the local host is empty).
inline QJsonObject conversations(const FleetState &state, double now)
{
    QJsonObject result;
    const auto all = [&](const QString &host, const BoxState &box) {
        for (const auto &s : box.sessions)
            result[host + '\n' + s.name] = details(s, {event(1, now - 1800, "UserPromptSubmit", s.prompt),
                                                       event(2, now - 1700, "Stop", "Started on it; I will report back here.")});
    };
    all({}, state.local());
    for (const auto &peer : state.peerNames()) all(peer, *state.peer(peer));
    const auto find = [&](const QString &host, const QString &name) {
        const auto &box = host.isEmpty() ? state.local() : *state.peer(host);
        for (const auto &s : box.sessions) if (s.name == name) return s;
        return SessionInfo{};
    };
    auto lead = details(find({}, "claude/orbit/checkout"), {
        event(1, now - 3560, "UserPromptSubmit", "Lead the Checkout v2 team: saved cards in the API and the checkout form, covered by "
                                                "contract tests. Plan the tasks, then hand them out."),
        event(2, now - 3500, "PostToolUse", "docs/checkout/architecture.md", "Read"),
        event(3, now - 3420, "Stop", "Plan sent for approval: 7 tasks in 5 layers. #2 and #3 can start together; the tests wait for the API."),
        event(4, now - 2100, "UserPromptSubmit", "[Zerus team Checkout v2] Report from Frontend · #4 Address validation, attempt 1. "
                                                "Check it against the criteria."),
        event(5, now - 2060, "PostToolUse", "web/checkout/address/validate.ts", "Read"),
        event(6, now - 60, "Stop", "Reviewing **#4 Address validation** from Frontend against its criteria:\n\n"
                                   "- Postal codes are checked per country, with the message under the field\n"
                                   "- The form keeps its keyboard order: street, city, postal code, country\n"
                                   "- The public `/v1/checkout` contract is unchanged\n\n"
                                   "Running the checks before I accept it:\n\n"
                                   "```bash\npnpm test checkout/address\ncargo test -p payments contract\n```\n\n"
                                   "#2 Saved cards API and #3 Card form UI are still in progress."),
        event(7, now - 48, "PreToolUse", "pnpm test checkout/address", "Bash")});
    lead["task_lists"] = QJsonObject{{"main", QJsonObject{{"run_id", "run-checkout"}, {"items", QJsonArray{
        QJsonObject{{"title", "Plan Checkout v2 with the team"}, {"status", "completed"}},
        QJsonObject{{"title", "Hand out the first wave"}, {"status", "completed"}},
        QJsonObject{{"title", "Review #4 Address validation"}, {"status", "in_progress"}},
        QJsonObject{{"title", "Review #2 and #3 reports"}, {"status", "pending"}},
        QJsonObject{{"title", "Write the release notes"}, {"status", "pending"}}}}}}};
    result["\nclaude/orbit/checkout"] = lead;

    auto review = details(find("build-02", "dsh/orbit/review"), {
        event(1, now - 900, "UserPromptSubmit", "Review the payments changes on release/2.4 before tonight's release."),
        event(2, now - 860, "PostToolUse", "git diff --stat origin/main...release/2.4", "Bash"),
        event(3, now - 820, "AgentMessage", "14 files changed since main:\n\n"
                                            "- `services/payments`: 9 files, the saved cards API and two migrations\n"
                                            "- `web/checkout`: 3 files, the card form\n"
                                            "- `docs/api`: 2 files\n\n"
                                            "The migrations touch the `ledger` table, so I would start there.")});
    review["attention_id"] = "dsh-review-scope";
    review["pending_questions"] = QJsonArray{QJsonObject{
        {"source", "dsh_native"}, {"answer_transport", "native_dsh"}, {"question_id", "dsh-review-scope"}, {"question_hash", QString(64, 'c')},
        {"run_id", "run-review"}, {"conversation_id", "conversation-review"}, {"can_answer", true},
        {"questions", QJsonArray{QJsonObject{{"id", "scope"}, {"header", "Review scope"}, {"question", "Which changes should I review first?"},
            {"multi_select", false}, {"allow_other", true}, {"options", QJsonArray{
                QJsonObject{{"id", "payments"}, {"label", "Payments and migrations (Recommended)"},
                            {"description", "The saved cards API and the two ledger migrations: 9 files."}},
                QJsonObject{{"id", "all"}, {"label", "Everything on release/2.4"}, {"description", "All 14 files, with the card form and the API docs."}}}}}}}}};
    result["build-02\ndsh/orbit/review"] = review;

    const auto designer = find({}, "claude/team-designer/team-designer");
    if (!designer.name.isEmpty())
        result["\nclaude/team-designer/team-designer"] = details(designer, {
            event(1, now - 400, "UserPromptSubmit", "New team for saved payment methods in checkout. Members: Backend (DeepSeek), "
                                                   "Frontend (Codex), Tester (Kimi). Help me with the goal and the protocol."),
            event(2, now - 340, "Stop", QStringLiteral("Here is a draft you can use:\n\n**GOAL:**\n%1\n\n**PROTOCOL:**\n%2")
                                            .arg(QString::fromLatin1(goal), QString::fromLatin1(protocol)))});
    return result;
}

// The fake hgs: answers from <script>.json, which the test writes.
constexpr char program[] = R"PY(#!/usr/bin/env python3
import json,sys
d=json.load(open(__file__+'.json',encoding='utf-8'))
a=sys.argv[1:]
host=''
if a and a[0].startswith('@'):host=a[0][1:];a=a[1:]
c=a[0] if a else ''
def out(v):sys.stdout.write(json.dumps(v))
profiles=d['accounts']['profiles'].get(host,[])
def usage(p):
 u=dict(d['accounts']['usage'].get(p['id'],{'status':'unavailable'}))
 u.update(id=p['id'],provider=p['provider'],home=p['home'])
 return u
if c=='inspect':
 name=a[1] if len(a)>1 else ''
 out({} if '--agent' in a else d['inspect'].get(host+'\n'+name,{'tracked':True,'events':[],'cursor':0}))
elif c=='account' and a[1:2]==['ls']:out({'profiles':profiles})
elif c=='account' and a[1:2]==['inspect']:
 if '--session' in a:
  provider=a[a.index('--session')+1].split('/')[0]
  found=[p for p in profiles if p['provider']==provider]
  u=usage(found[0]) if found else {'status':'unavailable','provider':provider}
  for k in ('id','home'):u.pop(k,None)
  out(u)
 else:
  found=[p for p in profiles if p['id']==a[2]]
  out(usage(found[0]) if found else {'status':'unavailable','id':a[2]})
elif c=='team':
 op=a[1] if len(a)>1 else ''
 if op=='list':
  t=dict(d['team'])
  for k in ('layers','cycle','ready','validation'):t.pop(k,None)
  out({'items':[t]})
 elif op=='status':out(d['team'])
 elif op=='events':out(d['feed'])
 elif op=='catalog' and a[2:3]==['get']:out(d['catalog'])
 else:
  # A change is kept for the test to read (U57: decide-proposal); #754:
  # `.proposalrefuse` refuses a decision with its words.
  open(__file__+'.calls','a',encoding='utf-8').write(json.dumps({'args':a,'input':json.loads(sys.stdin.read() or '{}')})+'\n')
  import os
  if op=='decide-proposal' and os.path.exists(__file__+'.proposalrefuse'):print(open(__file__+'.proposalrefuse').read(),file=sys.stderr);sys.exit(1)
  out({'status':'accepted' if op=='decide-proposal' else 'ok'})
elif c=='model-server':out(d.get('model_servers',{'servers':[]}) if a[1:2]==['list'] else {'status':'error','error':'refused','detail':'not in this fixture'})
elif c=='git-status' and 'git' in d:out(d['git'].get(host+'\n'+a[a.index('--path')+1],{'state':'not_repo'}))
elif c=='ls':out({'host':host or 'workstation','ok':True,'sessions':[]})
elif c=='swarm':out({'schema':1,'organization':d['organization'],'node_id':'node-workstation','versions':{},'written':{},'conflicts':[],
 'peers':{'macbook':{},'build-01':{},'build-02':{}}})
elif c=='project' and a[1:2]==['ls']:out([])
else:out({})
)PY";
}
