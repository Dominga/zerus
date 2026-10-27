#include "HgsClient.h"
#include <QtMath>
#include "DownloadCache.h"
#include "ChildProcess.h"
#include "JsonParse.h"
#include "ProcessRunner.h"
#include "LocalFile.h"
#include "MachinePlatform.h"
#include "SessionNotifications.h"
#include "SettingsJson.h"

#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QJsonValue>
#include <QFileInfo>
#include <QFile>
#include <QMimeDatabase>
#include <QProcess>
#include <QProcessEnvironment>
#include <QStandardPaths>
#include <QTimer>
#include <QRegularExpression>
#include <QSettings>
#include <QDateTime>
#include <QPointer>
#include <QDataStream>
#include <QCryptographicHash>
#include <QUuid>
#include <memory>

#include <algorithm>
#include <cmath>
#include <functional>
#include <limits>
#include <utility>

namespace {

// A JSON object reply parsed on the preparation thread. error is empty when the
// reply was a JSON object within limit bytes (0: any size).
struct ObjectReply {
    QJsonObject object;
    QString error;
    bool ok() const { return error.isEmpty(); }
};
ObjectReply objectReply(const QByteArray &bytes, qsizetype limit = 0)
{
    ObjectReply reply;
    if (limit > 0 && bytes.size() > limit) { reply.error = QStringLiteral("reply exceeds %1 bytes").arg(limit); return reply; }
    reply.object = JsonParse::object(bytes, &reply.error);
    return reply;
}
const auto parseObject = [](const QByteArray &bytes) { return objectReply(bytes); };
// The reply as an object, or an empty object for anything else.
const auto parseJsonObject = [](const QByteArray &bytes) { return JsonParse::object(bytes); };
// A finished child's result with its standard output parsed as an object.
struct ParsedResult {
    ProcessRunner::Result result;
    ObjectReply reply;
};
ParsedResult parsedResult(ProcessRunner::Result result, qsizetype limit = 0)
{
    ParsedResult parsed; parsed.reply = objectReply(result.standardOutput, limit); parsed.result = std::move(result);
    return parsed;
}


// The executable a start would use: a path as given, otherwise a PATH lookup
// (which honors PATHEXT on Windows, where C:\Zerus\hgs.exe is a path as well).
QString resolvedProgram(const QString &program)
{
    bool path = program.contains(QLatin1Char('/'));
#ifdef Q_OS_WIN
    path = path || program.contains(QLatin1Char('\\')) || QFileInfo(program).isAbsolute();
#endif
    return path ? program : QStandardPaths::findExecutable(program);
}

ProcessRunner::Request processRequest(const QString &program, const QStringList &arguments)
{
    ProcessRunner::Request request;
    // GUI actions are human input even when this GUI was launched from an
    // agent's shell tool. Never inherit that tool's attribution identity.
    auto environment = QProcessEnvironment::systemEnvironment();
    environment.remove("HGS_SESSION");environment.remove("HGS_RUN_ID");
    request.environment=environment;
#ifdef Q_OS_MACOS
    // Qt encodes argv with QFile::encodeName(), which decomposes Unicode on
    // macOS. Session names are byte identities, not filesystem names: changing
    // NFC to NFD can target another session or make an existing one disappear.
    // Environment values use UTF-8 without normalization. The ASCII-only sh
    // trampoline expands those values as quoted data, removes its private
    // variables, and execs the CLI with the original argv and process identity.
    QStringList values{program}; values.append(arguments);
    QStringList variables;
    QString script = QStringLiteral("set --");
    for (qsizetype i = 0; i < values.size(); ++i) {
        const QString variable = QStringLiteral("_HGS_ZERUS_ARG_%1").arg(i);
        variables.append(variable); environment.insert(variable, values[i]);
        script += QStringLiteral(" \"$%1\"").arg(variable);
    }
    script += QStringLiteral("; unset %1; exec \"$@\"").arg(variables.join(' '));
    request.program = QStringLiteral("/bin/sh");
    request.arguments = {QStringLiteral("-c"), script};
    request.environment = environment;
#else
    request.program = program;
    request.arguments = arguments;
#endif
    return request;
}

// Direct QProcess children (machine setup, transfers, launches) start on the GUI
// thread while the runner may be creating a request's pipes (ChildProcess.h).
void configureProcess(QProcess *process, const QString &program, const QStringList &arguments)
{
    const auto request = processRequest(program, arguments);
    process->setProgram(request.program); process->setArguments(request.arguments);
    if (request.environment) process->setProcessEnvironment(*request.environment);
    ChildProcess::inheritStdioOnly(*process);
}

// null -> QString(), не пустая строка "": вызывающий код различает их через isNull().
QString nullableString(const QJsonValue &v)
{
    if (v.isNull() || v.isUndefined())
        return QString();
    return v.toString();
}

SessionInfo sessionFromJson(const QJsonObject &obj)
{
    SessionInfo s;
    s.recovery = obj.value("recovery").toObject();
    s.name = obj.value(QStringLiteral("name")).toString();
    s.cmd = obj.value(QStringLiteral("cmd")).toString();
    s.project = nullableString(obj.value(QStringLiteral("project")));
    s.tag = nullableString(obj.value(QStringLiteral("tag")));
    s.attached = obj.value(QStringLiteral("attached")).toInt();
    s.created = static_cast<qint64>(obj.value(QStringLiteral("created")).toDouble());
    s.conversationId = obj.value("conversation_id").toString();
    s.replyId = obj.value("reply_id").toString(); s.replyAt = obj.value("reply_at").toDouble();
    s.replyScheduled = obj.value("reply_scheduled").toBool();
    const auto schedules = obj.value("schedules").toObject();
    s.scheduledJobs = schedules.value("count").toInt(-1);
    s.recurringSchedules = schedules.value("recurring").toInt(-1);
    s.nextScheduledAt = schedules.value("next_at").toDouble();
    s.coordinationWait = obj.value("coordination_wait").toObject();
    const auto work = obj.value("process_work").toObject();
    s.backgroundProcesses = work.value("count").toInt(-1);
    s.backgroundStarted = work.value("started_at").toDouble();
    s.backgroundObservedAt = work.value("sampled_at").toDouble();
    s.backgroundCommand = work.value("command").toString();
    s.runId = obj.value(QStringLiteral("run_id")).toString();
    s.worktreeOwnerId = obj.value("worktree_owner_id").toString();
    s.notificationPreferences = obj.value("notification_preferences").toObject();
    s.sharedNotifications = SessionNotifications::valid(s.notificationPreferences, s.conversationId);
    if (s.sharedNotifications) s.muted = s.notificationPreferences.value("muted").toBool();
    else s.notificationPreferences = {};
    s.launchId = obj.value(QStringLiteral("launch_id")).toString();
    s.attentionId = obj.value(QStringLiteral("attention_id")).toString();
    s.sharedRead = obj.contains(QStringLiteral("attention_signature"));
    s.attentionSignature = obj.value(QStringLiteral("attention_signature")).toString();
    const auto read = obj.value(QStringLiteral("read")).toObject();
    if (!s.conversationId.isEmpty() && read.value("conversation_id").toString() == s.conversationId) {
        s.readReplyId = read.value("reply_id").toString();
        s.readAttentionSignature = read.value("attention_signature").toString();
    }
    // Strict types: anything but a whole non-negative count is unknown, never 0.
    s.optionalQuestionsReported = obj.contains(QStringLiteral("optional_questions"));
    const auto optional = obj.value(QStringLiteral("optional_questions"));
    if (optional.isObject()) {
        const auto summary = optional.toObject();
        const auto count = summary.value(QStringLiteral("count"));
        const double value = count.isDouble() ? count.toDouble() : -1;
        if (value >= 0 && value <= 1e9 && value == std::floor(value)) {
            s.optionalQuestions = int(value);
            for (const auto &id : summary.value(QStringLiteral("ids")).toArray())
                if (id.isString() && !id.toString().isEmpty() && !s.optionalQuestionIds.contains(id.toString()))
                    s.optionalQuestionIds << id.toString();
            const auto latest = summary.value(QStringLiteral("latest_at"));
            s.optionalQuestionsLatestAt = latest.isDouble() && latest.toDouble() > 0 ? latest.toDouble() : 0;
        }
    }
    s.state = obj.value(QStringLiteral("state")).toString(QStringLiteral("running"));
    s.archiveId = s.state == QLatin1String("archived") ? obj.value(QStringLiteral("archive_id")).toString() : QString();
    s.archivedAt = obj.value(QStringLiteral("archived_at")).toDouble();
    s.resumable = obj.value(QStringLiteral("resumable")).toBool();
    s.terminalReported = obj.value("terminal_supported").isBool();
    s.terminalAttachReported = obj.value("terminal_attach_supported").isBool();
    s.terminalAttachSupported = obj.value("terminal_attach_supported").toBool();
    s.terminalAttachReason = obj.value("terminal_attach_reason").toString();
    s.terminalSupported = obj.value("terminal_supported").toBool(s.cmd != "dsh" && s.cmd != "opencode");
    s.bulkLifecycleSupported = obj.value("bulk_lifecycle_supported").toBool(s.cmd != "opencode");
    s.terminalReason = obj.value("terminal_reason").toString();
    s.actionsReported = obj.value("allowed_actions").isArray();
    for (const auto &action : obj.value("allowed_actions").toArray()) if (action.isString()) s.allowedActions.insert(action.toString());
    s.actionReasons = obj.value("action_reasons").toObject();
    s.activity = obj.value(QStringLiteral("activity")).toString();
    s.tracked = obj.value(QStringLiteral("tracked")).toBool();
    s.accountId = obj.value("account_id").toString(); s.accountHome = obj.value("account_home").toString();
    s.phase = obj.value(QStringLiteral("phase")).toString();
    s.processState = obj.value(QStringLiteral("process_state")).toString();
    s.conversationState = obj.value(QStringLiteral("conversation_state")).toString();
    s.runtimeState = obj.value(QStringLiteral("runtime_state")).toString();
    s.model = obj.value(QStringLiteral("model")).toString();
    s.goal = obj.value(QStringLiteral("goal")).toObject();
    s.effort = obj.value(QStringLiteral("effort")).toString();
    s.prompt = obj.value(QStringLiteral("prompt")).toString();
    s.currentTool = obj.value(QStringLiteral("current_tool")).toString();
    s.toolDetail = obj.value(QStringLiteral("tool_detail")).toString();
    s.lastEventAt = obj.value(QStringLiteral("last_event_at")).toDouble();
    s.attentionStarted = obj.value("attention_started").toDouble();
    s.turnStarted = obj.value(QStringLiteral("turn_started")).toDouble();
    s.compactionStarted = obj.value(QStringLiteral("compaction_started")).toDouble();
    s.subagentCount = obj.value(QStringLiteral("subagent_count")).toInt();
    s.cwd = obj.value("cwd").toString(); s.cwdSource = obj.value("cwd_source").toString();
    s.canonicalCwd=obj.value("cwd_canonical").toString();s.gitCommonDir=obj.value("git_common_dir").toString();
    s.gitRoot = obj.value("git_root").toString(); s.gitBranch = obj.value("git_branch").toString();
    s.gitWorktree = obj.value("git_worktree").toBool(); s.gitDetached = obj.value("git_detached").toBool();
    s.gitWorktreeName = obj.value("git_worktree_name").toString(); s.gitMetadataState = obj.value("git_metadata_state").toString();
    s.activitySummary = obj.value("activity_summary").toString(); s.activityDetail = obj.value("activity_detail").toString();
    s.providerError = obj.value("provider_error").toObject();
    s.providerStatusAt = obj.value("provider_status_at").toDouble();
    s.subagentSource = obj.value("subagent_source").toString();
    s.subagentCountsComplete = obj.value("subagent_counts_complete").toBool();
    s.subagentActiveCount = obj.value("subagent_active_count").toInt(s.subagentCount);
    s.subagentCompletedCount = obj.value("subagent_completed_count").toInt();
    s.subagentTotalCount = obj.value("subagent_total_count").toInt(-1);
    s.subagentPreviews = obj.value("subagent_previews").toArray();
    s.subagents = obj.value("subagents").toObject();
    const QJsonArray clientsArr = obj.value(QStringLiteral("clients")).toArray();
    s.clients.reserve(clientsArr.size());
    for (const QJsonValue &c : clientsArr)
        s.clients << c.toString();
    return s;
}

// Общий разбор одного JSON-объекта "бокса" (используется и parseBox, и parseFleet —
// не дублировать поля).
BoxState boxFromObject(const QJsonObject &obj)
{
    BoxState box;
    box.archivePaging = obj.value("archive_paging").toBool();
    box.metrics = obj.value("metrics").toObject();
    box.host = obj.value(QStringLiteral("host")).toString();
    box.ok = obj.value(QStringLiteral("ok")).toBool();
    box.error = obj.value(QStringLiteral("error")).toString();
    const auto errorDetail = obj.value(QStringLiteral("error_detail")).toString();
    if (!errorDetail.isEmpty()) box.error = errorDetail;
    box.localSessions = obj.value(QStringLiteral("local_sessions")).toBool(true);
    box.localHost = obj.value(QStringLiteral("local_host")).toObject();
    for (const auto &value : obj.value(QStringLiteral("ended_launches")).toArray()) {
        const auto launch = value.toObject(); const auto id = launch.value(QStringLiteral("launch_id")).toString();
        if (!id.isEmpty()) box.endedLaunches.insert(id, launch);
    }
    box.peersKnown = obj.value("peers").isArray();
    for (const auto &peer : obj.value("peers").toArray()) if (peer.isString()) box.peers.append(peer.toString());

    const QJsonObject projectsObj = obj.value(QStringLiteral("projects")).toObject();
    for (auto it = projectsObj.constBegin(); it != projectsObj.constEnd(); ++it)
        box.projects.insert(it.key(), it.value().toString());

    const QJsonArray sessionsArr = obj.value(QStringLiteral("sessions")).toArray();
    box.sessions.reserve(sessionsArr.size());
    for (const QJsonValue &sv : sessionsArr)
        box.sessions << sessionFromJson(sv.toObject());

    return box;
}

// Разбор одного элемента `hgs project ls --json`. src в самом JSON -- строка
// ("ansible"|"local"), не булево: ансибл-происхождение читаем сравнением, а не
// доверяем CLI прислать готовый bool -- формат остаётся человекочитаемым снаружи
// (то же самое видно и в `hgs project ls --json | python3 -m json.tool`).
ProjectInfo projectFromJson(const QJsonObject &obj)
{
    ProjectInfo p;
    p.name = obj.value(QStringLiteral("name")).toString();
    p.dir = obj.value(QStringLiteral("dir")).toString();
    p.fromAnsible = obj.value(QStringLiteral("src")).toString() == QLatin1String("ansible");
    p.exists = obj.value(QStringLiteral("exists")).toBool();
    return p;
}

} // namespace

HgsClient::HgsClient(QString hgsPath, QObject *parent)
    : QObject(parent)
    // Пустой путь значит "искать в PATH" — так и QProcess::start понимает голое имя без '/'.
    , m_hgs(hgsPath.isEmpty() ? QStringLiteral("hgs") : std::move(hgsPath))
{
}

quint64 HgsClient::requestRecovery(const QString &host, const QString &operation, const QJsonObject &data)
{
    const auto request = ++m_recoveryRequest;
    QStringList args{"recovery", operation}; if (!host.isEmpty()) args.prepend('@' + host);
    auto process = processRequest(m_hgs, args); process.timeoutMs = host.isEmpty() ? 10000 : 25000;
    process.input = operation != "get" ? QJsonDocument(data).toJson(QJsonDocument::Compact) : QByteArray();
    ProcessRunner::run(std::move(process), this, [](ProcessRunner::Result result) { return parsedResult(std::move(result)); },
                       [this, request, host, operation](const ParsedResult &parsed) {
        const auto &result = parsed.result;
        const auto fail = [this, request](const QString &error) { emit recoveryFinished(request, false, {}, error); };
        if (result.outcome == ProcessRunner::Result::TimedOut) return fail(tr("Recovery request timed out. Reload its state before trying again."));
        if (result.outcome == ProcessRunner::Result::FailedToStart) return fail(result.errorString);
        if (result.exitCode || result.exitStatus != QProcess::NormalExit) {
            const auto error = QString::fromUtf8(result.standardError).trimmed().left(2000);
            return fail(error.isEmpty() ? tr("Could not update recovery. Reload its state.") : error);
        }
        if (!parsed.reply.ok()) return fail(tr("Invalid recovery response"));
        emit recoveryFinished(request, true, parsed.reply.object, {}); if (operation != "get") emit sessionWriteDone(host);
    });
    return request;
}

HgsClient::~HgsClient()
{
    // Worker launches (ProcessRunner) are killed when this object is destroyed.
    // Direct QProcess children remain for interactive transfers: QProcess's
    // destructor waits and can emit finished(). Disconnect while the callback's
    // captured maps and owner are still alive, before QObject deletes children.
    for (auto *proc : findChildren<QProcess *>(QString(), Qt::FindDirectChildrenOnly)) {
        for (auto *timer : proc->findChildren<QTimer *>()) timer->stop();
        proc->disconnect(); proc->blockSignals(true);
        if (proc->state() != QProcess::NotRunning) { proc->kill(); proc->waitForFinished(1000); }
    }
}

BoxState HgsClient::box(const QJsonObject &object) { return boxFromObject(object); }

QList<SessionInfo> HgsClient::parseSessions(const QJsonArray &rows) {
    QJsonObject box{{"host","archive"},{"ok",true},{"sessions",rows}};
    return boxFromObject(box).sessions;
}

BoxState HgsClient::parseBox(const QByteArray &json, QString *error)
{
    if (error)
        error->clear();

    QString parseError;
    const QJsonObject object = JsonParse::object(json, &parseError);
    if (!parseError.isEmpty()) {
        if (error) *error = parseError;
        return BoxState{};
    }

    return boxFromObject(object);
}

QList<BoxState> HgsClient::parseFleet(const QByteArray &json, QString *error)
{
    if (error)
        error->clear();

    QString parseError;
    const QJsonArray arr = JsonParse::array(json, &parseError);
    if (!parseError.isEmpty()) {
        if (error) *error = parseError;
        return {};
    }

    QList<BoxState> boxes;
    boxes.reserve(arr.size());
    for (const QJsonValue &v : arr)
        boxes << boxFromObject(v.toObject());
    return boxes;
}

QList<ProjectInfo> HgsClient::parseProjects(const QByteArray &json, QString *error)
{
    if (error)
        error->clear();

    QString parseError;
    const QJsonArray arr = JsonParse::array(json, &parseError);
    if (!parseError.isEmpty()) {
        if (error) *error = parseError;
        return {};
    }

    QList<ProjectInfo> projects;
    projects.reserve(arr.size());
    for (const QJsonValue &v : arr)
        projects << projectFromJson(v.toObject());
    return projects;
}

namespace {

// Общий каркас запуска hgs: считает вывод только по finished (частичные readyRead
// не парсим — printf в hgs пишет одним куском, но QProcess вправе доставить его частями),
// бьёт по таймауту и всегда сообщает, какой бинарь и с какими аргументами не удался.
// Polls start hgs about once a second, so fork/exec runs on ProcessRunner's worker
// instead of the GUI thread. ProcessRunner also reports exactly once: the finished()
// that follows a timeout kill() must not reach onFailed() again (--selftest leaves
// main() on the first failure).
// Why a call failed, or empty when it succeeded.
QString failureDetail(const QString &hgs, const ProcessRunner::Result &result)
{
    if (result.outcome == ProcessRunner::Result::TimedOut)
        return QStringLiteral("timed out");
    if (result.outcome == ProcessRunner::Result::FailedToStart)
        return QStringLiteral("cannot start '%1': %2").arg(hgs, result.errorString);
    // hgs всегда возвращает 0 для "спящего пира" — ненулевой код или CrashExit
    // означает, что сломан сам вызов, а не то, что застали пира офлайн.
    if (result.exitStatus != QProcess::NormalExit || result.exitCode != 0) {
        const QString detail = QString::fromLocal8Bit(result.standardError).trimmed().left(4000);
        return detail.isEmpty() ? QStringLiteral("exit code %1").arg(result.exitCode) : detail;
    }
    return {};
}

void runHgsDirect(const QString &hgs, const QStringList &args, int timeoutMs, QObject *parent,
            const std::function<void(const QByteArray &)> &onFinished,
            const std::function<void(const QString &, const QString &)> &onFailed,
            const QByteArray &input = {})
{
    auto request = processRequest(hgs, args); request.timeoutMs = timeoutMs;
    if (!input.isEmpty()) request.input = input;
    ProcessRunner::run(std::move(request), parent, [hgs, args, onFinished, onFailed](const ProcessRunner::Result &result) {
        if (const auto detail = failureDetail(hgs, result); !detail.isEmpty())
            return onFailed(hgs + QLatin1Char(' ') + args.join(QLatin1Char(' ')), detail);
        onFinished(result.standardOutput);
    });
}

// A successful call's output becomes parse(output) on the preparation thread
// (ProcessRunner.h); done(value) or onFailed then runs on parent's thread.
template <typename Parse, typename Done>
void runHgsParsedDirect(const QString &hgs, const QStringList &args, int timeoutMs, QObject *parent, Parse parse, Done done,
                        const std::function<void(const QString &, const QString &)> &onFailed, const QByteArray &input = {})
{
    using Value = std::decay_t<std::invoke_result_t<Parse &, const QByteArray &>>;
    struct Reply { QString detail; Value value{}; };
    auto request = processRequest(hgs, args); request.timeoutMs = timeoutMs;
    if (!input.isEmpty()) request.input = input;
    ProcessRunner::run(std::move(request), parent,
        [hgs, parse = std::move(parse)](ProcessRunner::Result result) mutable {
            Reply reply; reply.detail = failureDetail(hgs, result);
            if (reply.detail.isEmpty()) reply.value = parse(result.standardOutput);
            return reply;
        },
        [hgs, args, done = std::move(done), onFailed](Reply reply) mutable {
            if (!reply.detail.isEmpty()) return onFailed(hgs + QLatin1Char(' ') + args.join(QLatin1Char(' ')), reply.detail);
            done(std::move(reply.value));
        });
}

void guardMachine(QStringList &args,const QString &expected)
{
    if(expected.isEmpty())return;
    const int at=!args.isEmpty()&&args.first().startsWith('@')?1:0;
    args.insert(at,"--expect-machine");args.insert(at+1,expected);
}
void runHgs(const QString &hgs,const QStringList &args,int timeoutMs,QObject *parent,
            const std::function<void(const QByteArray &)> &done,
            const std::function<void(const QString &,const QString &)> &failed,const QByteArray &input={})
{
    const int at=!args.isEmpty()&&args.first().startsWith('@')?1:0;
    if(args.value(at)!="--expect-machine"){runHgsDirect(hgs,args,timeoutMs,parent,done,failed,input);return;}
    QStringList help{"--help"};if(at)help.prepend(args.first());
    runHgsDirect(hgs,help,15000,parent,[=](const QByteArray &out){
        if(!out.contains("guarded-create-v1")){failed({},QObject::tr("The selected CLI cannot verify this creation's computer. Update hgs before continuing."));return;}
        runHgsDirect(hgs,args,timeoutMs,parent,done,failed,input);
    },failed);
}

// runHgs() for a JSON reply: parse runs on the preparation thread, done on
// parent's thread. Every hgs reply the client parses goes through here.
template <typename Parse, typename Done>
void runHgsParsed(const QString &hgs, const QStringList &args, int timeoutMs, QObject *parent, Parse parse, Done done,
                  const std::function<void(const QString &, const QString &)> &failed, const QByteArray &input = {})
{
    const int at = !args.isEmpty() && args.first().startsWith('@') ? 1 : 0;
    if (args.value(at) != "--expect-machine") { runHgsParsedDirect(hgs, args, timeoutMs, parent, std::move(parse), std::move(done), failed, input); return; }
    QStringList help{"--help"}; if (at) help.prepend(args.first());
    runHgsDirect(hgs, help, 15000, parent, [=](const QByteArray &out) {
        if (!out.contains("guarded-create-v1")) { failed({}, QObject::tr("The selected CLI cannot verify this creation's computer. Update hgs before continuing.")); return; }
        runHgsParsedDirect(hgs, args, timeoutMs, parent, parse, done, failed, input);
    }, failed);
}


} // namespace

void HgsClient::cancelArchivePages(){const auto jobs=std::exchange(m_archiveJobs,{});qDeleteAll(jobs);}
quint64 HgsClient::requestArchivePage(const QString &host,const QJsonObject &query) {
    delete m_archiveJobs.take(host); // Cancels only our read-only CLI helper.
    auto *owner=new QObject(this);m_archiveJobs[host]=owner;
    const quint64 request=++m_archiveRequest;
    QStringList args;if(!host.isEmpty())args<<'@'+host;args<<"archives"<<"--json";
    const auto finish=[this,owner,request,host](const QJsonObject &data,const QString &error){m_archiveJobs.remove(host);owner->deleteLater();emit archivePageReady(request,host,data,error);};
    struct Page { QJsonObject data; QString error; };
    runHgsParsed(m_hgs,args,30000,owner,[query](const QByteArray &out){
        QString parseError;const auto data=JsonParse::object(out,&parseError);QString error;
        const auto rows=data["sessions"].toArray();
        if(!parseError.isEmpty()||data["version"].toInt()!=1||!data["sessions"].isArray()||rows.size()>50||!data["next_cursor"].isString()||data["next_cursor"].toString().size()>1024||!data["total"].isDouble()||data["total"].toDouble()<rows.size()
            ||data["from"]!=query["from"]||data["until"]!=query["until"]||data["query"]!=query["query"]||rows.isEmpty()&&!data["next_cursor"].toString().isEmpty()||!query["cursor"].toString().isEmpty()&&query["cursor"]==data["next_cursor"])error=HgsClient::tr("Invalid archive page. Update the computer CLI and refresh.");
        if(data.contains("previous_cursor")||data.contains("position")) {
            const auto previous=data["previous_cursor"];const auto position=data["position"].toDouble(-1);
            if(!previous.isString()||previous.toString().size()>1024||!data["position"].isDouble()||position<0||position!=qFloor(position)||position+rows.size()>data["total"].toDouble())error=HgsClient::tr("Invalid archive window. Refresh the archive.");
        }
        for(const auto &value:rows){const auto row=value.toObject();const auto id=row["archive_id"].toString();if(row["state"]!="archived"||row["name"].toString().isEmpty()||QUuid(id).toString(QUuid::WithoutBraces)!=id||!row["archived_at"].isDouble()||row["archived_at"].toDouble()<query["from"].toDouble()||row["archived_at"].toDouble()>=query["until"].toDouble())error=HgsClient::tr("Invalid archived session identity. Refresh the archive.");}
        return Page{data,error};
    },[finish](const Page &page){finish(page.data,page.error);},[finish](const QString &,const QString &error){finish({},error);},QJsonDocument(query).toJson(QJsonDocument::Compact));
    return request;
}

void HgsClient::requestVersion()
{
    if (m_versionInFlight) return;
    m_versionInFlight=true;
    runHgs(m_hgs,{QStringLiteral("--version")},3000,this,
        [this](const QByteArray &out){
            m_versionInFlight=false;
            static const QRegularExpression format(QStringLiteral("^hgs ([0-9]+\\.[0-9]+\\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\\+[0-9A-Za-z.-]+)?)$"));
            const auto match=format.match(QString::fromUtf8(out).trimmed());
            emit versionReady(out.size()<=128 && match.hasMatch()?match.captured(1):QString{});
        },[this](const QString &,const QString &){m_versionInFlight=false;emit versionReady({});});
}

void HgsClient::requestSessionReference(const QString &host, const QString &name, const QString &run,
                                       const QString &conversation, const QString &archive, const QString &agent)
{
    QStringList args{"reference", name, "--json", "--run", run, "--conversation", conversation};
    if (!host.isEmpty()) args.prepend('@' + host);
    if (!archive.isEmpty()) args << "--archive" << archive;
    if (!agent.isEmpty()) args << "--agent" << agent;
    runHgsParsed(m_hgs,args,host.isEmpty()?15000:25000,this,parseObject,[this](const ObjectReply &data){
        const auto uri=data.object.value("uri").toString();
        if(!data.ok() || !uri.startsWith("zerus://session?") || uri.size()>8192)
            emit sessionReferenceFailed(tr("Could not read a session reference. Update hgs on this computer."));
        else emit sessionReferenceReady(uri);
    },[this](const QString &,const QString &error){emit sessionReferenceFailed(error);});
}

quint64 HgsClient::requestAccounts(const QString &host, const QStringList &arguments, const QByteArray &input)
{
    const quint64 request = ++m_accountRequest;
    QStringList args;
    if (!host.isEmpty()) args << QLatin1Char('@') + host;
    args << QStringLiteral("account"); args.append(arguments);
    // Start waits up to 20 seconds for OpenCode's registration on that computer.
    runHgsParsed(m_hgs, args, arguments.value(0) == "set-key" ? 90000 : arguments.value(0) == "copy" || arguments.value(0) == "start" ? 60000 : 25000, this,
        parseObject, [this, request, host](const ObjectReply &data) {
            if (!data.ok()) emit accountsFailed(request, host, tr("Invalid account catalog response"));
            else emit accountsReady(request, host, data.object);
        }, [this, request, host](const QString &, const QString &error) { emit accountsFailed(request, host, error); }, input);
    return request;
}

quint64 HgsClient::requestOpenCodeModels(const QString &host, const QString &location)
{
    const quint64 request = ++m_accountRequest;
    QStringList args;
    if (!host.isEmpty()) args << QLatin1Char('@') + host;
    args << QStringLiteral("account") << QStringLiteral("models") << QStringLiteral("native-opencode")
         << QStringLiteral("--location") << location << QStringLiteral("--json");
    struct Catalog { bool tooLarge = false; ObjectReply reply; };
    runHgsParsed(m_hgs, args, 30000, this,
        [](const QByteArray &bytes) {
            // hgs keeps the catalog within 4 MiB; a larger answer is not one.
            Catalog catalog; catalog.tooLarge = bytes.size() > 5 * 1024 * 1024;
            if (!catalog.tooLarge) catalog.reply = objectReply(bytes);
            return catalog;
        },
        [this, request, host, location](const Catalog &catalog) {
            if (catalog.tooLarge) {
                emit openCodeModelsFailed(request, host, location, tr("The OpenCode model catalog is too large"));
                return;
            }
            if (!catalog.reply.ok()) emit openCodeModelsFailed(request, host, location, tr("Invalid OpenCode model catalog response"));
            else emit openCodeModelsReady(request, host, location, catalog.reply.object);
        }, [this, request, host, location](const QString &, const QString &error) { emit openCodeModelsFailed(request, host, location, error); });
    return request;
}

quint64 HgsClient::requestOpenCodeFavorite(const QString &host, const QString &value, bool favorite)
{
    const quint64 request = ++m_accountRequest;
    // hgs answers a repeated identity from its receipt, so every change gets its own.
    const auto identity = QUuid::createUuid().toString(QUuid::WithoutBraces);
    QStringList args;
    if (!host.isEmpty()) args << QLatin1Char('@') + host;
    args << QStringLiteral("account") << QStringLiteral("favorite") << QStringLiteral("native-opencode")
         << QStringLiteral("--model") << value << QStringLiteral("--favorite") << (favorite ? QStringLiteral("on") : QStringLiteral("off"))
         << QStringLiteral("--request-id") << identity << QStringLiteral("--json");
    runHgsParsed(m_hgs, args, 25000, this, parseObject,
        [this, request, host, value, favorite, identity](const ObjectReply &data) {
            const auto &receipt = data.object;
            // The receipt must name this exact change.
            if (!data.ok() || receipt["status"] != "applied"
                || receipt["request_id"] != identity || receipt["model"] != value || receipt["favorite"] != favorite)
                emit openCodeFavoriteFailed(request, host, tr("Invalid favorite response"));
            else emit openCodeFavoriteReady(request, host, receipt);
        }, [this, request, host](const QString &, const QString &error) { emit openCodeFavoriteFailed(request, host, error); });
    return request;
}

quint64 HgsClient::requestMobile(const QString &host, const QStringList &arguments)
{
    const quint64 request = ++m_mobileRequest;
    QStringList help{QStringLiteral("--help")}, args{QStringLiteral("mobile")};
    args.append(arguments);
    if (!host.isEmpty()) { help.prepend(QLatin1Char('@') + host); args.prepend(QLatin1Char('@') + host); }
    const auto failed = [this, request, host](const QString &, const QString &error) {
        auto text = error; if (text.startsWith(QLatin1String("hgs: "))) text.remove(0, 5);
        emit mobileFailed(request, host, text);
    };
    // An older CLI would read "mobile" as an agent to launch.
    runHgs(m_hgs, help, host.isEmpty() ? 5000 : 15000, this, [this, request, host, args, failed](const QByteArray &out) {
        if (!out.contains("mobile phones|invite|revoke-phone")) {
            emit mobileFailed(request, host, tr("Update Zerus on %1 to manage phones from here.").arg(host.isEmpty() ? tr("this computer") : host));
            return;
        }
        runHgsParsed(m_hgs, args, 40000, this, parseObject, [this, request, host](const ObjectReply &data) {
            if (!data.ok()) emit mobileFailed(request, host, tr("Invalid phone management response"));
            else emit mobileReady(request, host, data.object);
        }, failed);
    }, failed);
    return request;
}

quint64 HgsClient::requestMachines(const QStringList &arguments)
{
    const quint64 request = ++m_machineRequest;
    QStringList args{QStringLiteral("machine")}; args.append(arguments);
    runHgsParsed(m_hgs, args, arguments.value(0) == "check" ? 25000 : 8000, this, parseObject,
        [this, request](const ObjectReply &data) {
            if (!data.ok()) emit machinesFailed(request, tr("Invalid machine settings response"));
            else emit machinesReady(request, data.object);
        }, [this, request](const QString &, const QString &error) { emit machinesFailed(request, error); });
    return request;
}

quint64 HgsClient::requestSwarm(const QStringList &arguments, const QJsonObject &input)
{
    const quint64 request = ++m_swarmRequest;
    QStringList args{QStringLiteral("swarm")}; args.append(arguments);
    const bool network = QStringList{"preview", "join", "sync", "bind"}.contains(arguments.value(0));
    runHgsParsed(m_hgs, args, network ? 180000 : 10000, this, parseObject,
        [this, request](const ObjectReply &data) {
            if (!data.ok()) emit swarmFailed(request, tr("Invalid project catalog response"));
            else emit swarmReady(request, data.object);
        }, [this, request](const QString &, const QString &error) { emit swarmFailed(request, error); },
        input.isEmpty() ? QByteArray() : QJsonDocument(input).toJson(QJsonDocument::Compact));
    return request;
}

int HgsClient::taskTimeoutMs(const QStringList &arguments) {
    const QString action = arguments.value(0);
    return action == QLatin1String("send") || action == QLatin1String("cancel") || action == QLatin1String("message") ? 90000 : 30000;
}

quint64 HgsClient::requestTask(const QStringList &arguments, const QString &input)
{
    const quint64 request = ++m_taskRequest;
    QStringList args{QStringLiteral("task")}; args.append(arguments);
    auto call = processRequest(m_hgs, args); call.timeoutMs = taskTimeoutMs(arguments);
    call.maximumOutputBytes = 36 * 1024 * 1024;
    // stdin always closes, even empty: a command that reads a text never waits on the GUI.
    call.input = input.toUtf8();
    ProcessRunner::run(std::move(call), this, [](ProcessRunner::Result result) {
        ParsedResult parsed;
        if (!result.outputLimitExceeded && result.outcome == ProcessRunner::Result::Finished && result.exitStatus == QProcess::NormalExit
            && result.exitCode == 0) parsed = parsedResult(std::move(result));
        else parsed.result = std::move(result);
        return parsed;
    }, [this, request](const ParsedResult &parsed) {
        const auto &result = parsed.result;
        if (result.outputLimitExceeded) return emit taskFailed(request, tr("Task response exceeds 36 MiB"));
        if (result.outcome == ProcessRunner::Result::TimedOut) return emit taskFailed(request, tr("hgs task timed out"));
        if (result.outcome == ProcessRunner::Result::FailedToStart)
            return emit taskFailed(request, tr("cannot start '%1': %2").arg(m_hgs, result.errorString));
        if (result.exitStatus != QProcess::NormalExit || result.exitCode != 0) {
            const QString detail = QString::fromLocal8Bit(result.standardError).trimmed().left(4000);
            return emit taskFailed(request, detail.isEmpty() ? tr("exit code %1").arg(result.exitCode) : detail);
        }
        if (!parsed.reply.ok()) emit taskFailed(request, tr("Invalid task response"));
        else emit taskReady(request, parsed.reply.object);
    });
    return request;
}

quint64 HgsClient::requestSessionHost(const QString &action)
{
    const quint64 request = ++m_sessionHostRequest;
    // install copies hgs.exe and a host image into the profile; status asks a
    // running host, which answers within five seconds or counts as unavailable.
    const int timeoutMs = action == QLatin1String("install") ? 60000 : action == QLatin1String("status") ? 15000 : 25000;
    runHgsParsed(m_hgs, {QStringLiteral("session-host"), action, QStringLiteral("--json")}, timeoutMs, this, parseObject,
        [this, request](const ObjectReply &data) {
            if (!data.ok()) emit sessionHostFailed(request, tr("Invalid session host response"));
            else emit sessionHostReady(request, data.object);
        }, [this, request](const QString &, const QString &error) { emit sessionHostFailed(request, error); });
    return request;
}

void HgsClient::setPresence(double idleSeconds, bool locked)
{
    if (m_presenceInFlight) { m_pendingPresence = qMakePair(idleSeconds, locked); m_presencePending = true; return; }
    m_presenceInFlight = true;
    const auto input = QJsonDocument(QJsonObject{{"idle_seconds", qMax(0.0, idleSeconds)}, {"locked", locked}}).toJson(QJsonDocument::Compact);
    const auto finished = [this](const QString &error) {
        m_presenceInFlight = false;
        emit presenceFinished(error);
        if (std::exchange(m_presencePending, false)) setPresence(m_pendingPresence.first, m_pendingPresence.second);
    };
    runHgs(m_hgs, {QStringLiteral("presence"), QStringLiteral("set"), QStringLiteral("--json")}, 5000, this,
        [finished](const QByteArray &) { finished({}); },
        [finished](const QString &, const QString &detail) { finished(detail); }, input);
}

void HgsClient::setupMachine(const QString &alias, const QString &source)
{
    if (m_machineSetupInFlight) return;
    m_machineSetupInFlight = true;
    auto *proc = new QProcess(this); QStringList args{"machine", "setup", alias};
    if (!source.isEmpty()) args << "--source" << source;
    configureProcess(proc, m_hgs, args); proc->setProcessChannelMode(QProcess::MergedChannels);
    connect(proc, &QProcess::readyReadStandardOutput, this, [this, proc]() {
        emit machineSetupOutput(QString::fromUtf8(proc->readAllStandardOutput()));
    });
    connect(proc, &QProcess::errorOccurred, this, [this, proc](QProcess::ProcessError error) {
        if (error != QProcess::FailedToStart) return;
        m_machineSetupInFlight = false; emit machineSetupFinished(false, proc->errorString()); proc->deleteLater();
    });
    connect(proc, qOverload<int, QProcess::ExitStatus>(&QProcess::finished), this, [this, proc](int code, QProcess::ExitStatus status) {
        emit machineSetupOutput(QString::fromUtf8(proc->readAllStandardOutput()));
        m_machineSetupInFlight = false;
        const bool ok = code == 0 && status == QProcess::NormalExit;
        emit machineSetupFinished(ok, ok ? QString() : status == QProcess::CrashExit ? tr("Setup was interrupted. See the log above.") : tr("Setup exited with code %1. See the log above.").arg(code));
        proc->deleteLater();
    });
    proc->start();
}

void HgsClient::requestLocal()
{
    if (m_localInFlight) return;
    m_localInFlight = true;
    emit stateReadStarted({});
    // Large native histories can take longer than five seconds. Keep one poll
    // in flight and allow it to finish instead of continually killing/restarting it.
    struct Listing { BoxState box; QString error; };
    runHgsParsed(m_hgs, {QStringLiteral("ls"), QStringLiteral("--json"), QStringLiteral("--local"), QStringLiteral("--no-archives")},
           15000, this,
           [](const QByteArray &out) { Listing listing; listing.box = parseBox(out, &listing.error); return listing; },
           [this](const Listing &listing) {
               m_localInFlight = false;
               const QString &err = listing.error;
               const BoxState &box = listing.box;
               if (!err.isEmpty()) {
                   emit stateReadFailed(QString(), err);
                   emit failed(m_hgs + QStringLiteral(" ls --json --local"), err);
                   return;
               }
               emit localReady(box);
           },
           [this](const QString &what, const QString &detail) { m_localInFlight = false; emit stateReadFailed(QString(), detail); emit failed(what, detail); });
}

void HgsClient::requestPeer(const QString &peer)
{
    // Не копим запросы к одному и тому же пиру: пока предыдущий "@peer ls --json" не
    // завершился (спящий ноут держит ssh-рукопожатие все 15с таймаута), повторные вызовы
    // -- тик меню, минутный фон, "Refresh" -- просто игнорируются. Иначе десять открытий
    // меню подряд = десять параллельных ssh к боксу, который не отвечает ни на один.
    if (m_peerInFlight.contains(peer))
        return;
    m_peerInFlight.insert(peer);
    emit stateReadStarted(peer);

    // ssh-рукопожатие может занять больше времени, чем локальный вызов — у самого hgs
    // уже есть ConnectTimeout=3, но обвязка (auth, motd) укладывается не всегда в 5с.
    struct Listing { BoxState box; QString error; };
    runHgsParsed(m_hgs, {QLatin1Char('@') + peer, QStringLiteral("ls"), QStringLiteral("--json"), QStringLiteral("--no-archives")},
           15000, this,
           [peer](const QByteArray &out) {
               Listing listing; listing.box = parseBox(out, &listing.error);
               // The CLI forwards the peer's own listing, whose host is the name
               // that machine gives itself (HGS_SELF or its host name). A machine
               // saved under another alias must still be found, polled and
               // addressed by that alias.
               if (listing.error.isEmpty()) listing.box.reportedHost = std::exchange(listing.box.host, peer);
               return listing;
           },
           [this, peer](const Listing &listing) {
               // Снятие метки здесь и в onFailed ниже -- ровно те же два колбэка, которыми
               // runHgs() гарантированно завершает вызов РОВНО один раз (см. reported в
               // runHgs). Без этого упавший или зависший по таймауту пир навсегда
               // застревал бы в m_peerInFlight, и его строка в меню замерла бы на
               // "polling…" до перезапуска трея.
               m_peerInFlight.remove(peer);
               const QString &err = listing.error;
               const BoxState &box = listing.box;
               if (!err.isEmpty()) {
                   emit stateReadFailed(peer, err);
                   emit failed(m_hgs + QStringLiteral(" ls --json"), err);
                   return;
               }
               emit peerReady(box);
           },
           [this, peer](const QString &what, const QString &detail) {
               m_peerInFlight.remove(peer);
               emit stateReadFailed(peer, detail);
               emit failed(what, detail);
           });
}

QSet<QString> HgsClient::pollingHosts() const
{
    auto hosts = m_peerInFlight;
    if (m_localInFlight) hosts.insert(QString());
    return hosts;
}

quint64 HgsClient::requestSearch(const QString &host, const QString &query)
{
    const auto id = ++m_searchRequest;
    QStringList args{"search", "--query", query, "--limit", "100"};
    if (!host.isEmpty()) args.prepend('@' + host);
    runHgsParsed(m_hgs, args, host.isEmpty() ? 12000 : 20000, this, parseObject,
        [this, id, host](const ObjectReply &doc) {
            if (!doc.ok() || !doc.object.value("results").isArray())
                emit searchFailed(id, host, tr("Invalid search response; update hgs on this machine."));
            else emit searchReady(id, host, doc.object);
        }, [this, id, host](const QString &, const QString &error) { emit searchFailed(id, host, error); });
    return id;
}

quint64 HgsClient::requestProcess(const QString &host, const QString &name, const QString &processId, const QString &run,
                                const QString &conversation, const QString &archive, const QString &generation, bool stop)
{
    static quint64 counter=0;
    const auto request=++counter;
    QStringList args{"processes",name,stop?"--stop":"--output",processId,"--run",run,"--conversation",conversation};
    if(!archive.isEmpty())args<<"--archive"<<archive;
    if(!generation.isEmpty())args<<"--generation"<<generation;
    if(!host.isEmpty())args.prepend('@'+host);
    runHgsParsed(m_hgs,args,45000,this,parseObject,[this,request](const ObjectReply &doc){
        if(!doc.ok())emit processFinished(request,{},tr("Invalid process response"));
        else emit processFinished(request,doc.object,{});
    },[this,request](const QString &,const QString &error){emit processFinished(request,{},error);});
    return request;
}

bool HgsClient::requestInspection(const QString &host, const QString &name, qint64 after, const QString &archiveId, bool includeProcesses)
{
    const QString key = host + QLatin1Char('\n') + name + QLatin1Char('\n') + archiveId;
    if (m_inspectionsInFlight.contains(key))
        return false;
    m_inspectionsInFlight.insert(key);
    emit inspectionLoadingChanged(host, name, archiveId, {}, true);
    QStringList args{QStringLiteral("inspect"), name, QStringLiteral("--after"), QString::number(after)};
    if (!includeProcesses) args << QStringLiteral("--skip-processes");
    if (!archiveId.isEmpty())
        args << QStringLiteral("--archive") << archiveId;
    if (!host.isEmpty())
        args.prepend(QLatin1Char('@') + host);
    runHgsParsed(m_hgs, args, host.isEmpty() ? 5000 : 15000, this, parseObject,
           [this, host, name, key, archiveId](const ObjectReply &doc) {
               m_inspectionsInFlight.remove(key);
               emit inspectionLoadingChanged(host, name, archiveId, {}, false);
               if (!doc.ok()) {
                   emit inspectionFailed(host, name, tr("Invalid activity response"), archiveId);
                   return;
               }
               emit inspectionReady(host, name, doc.object, archiveId);
           }, [this, host, name, key, archiveId](const QString &, const QString &detail) {
               m_inspectionsInFlight.remove(key);
               emit inspectionLoadingChanged(host, name, archiveId, {}, false);
               emit inspectionFailed(host, name, detail, archiveId);
           });
    return true;
}

quint64 HgsClient::downloadFile(const QString &host, const QString &path, const QString &destination)
{
    const auto id = ++m_fileDownloadRequest;
    transferFile(id, host, path, destination);
    return id;
}

quint64 HgsClient::findCachedFile(const QString &host, const QString &path)
{
    const auto id = ++m_fileDownloadRequest;
    const auto root = DownloadCache::directory();
    DownloadCache::async<DownloadCache::Result>(this, [root, host, path] { return DownloadCache::lookup(root, {host, path}); },
        [this, id](const DownloadCache::Result &result) { emit fileCacheChecked(id, result.path, result.error); });
    return id;
}

quint64 HgsClient::downloadFileToCache(const QString &host, const QString &path, bool force)
{
    const auto id = ++m_fileDownloadRequest;
    const auto root = DownloadCache::directory();
    QByteArray key;
    QDataStream stream(&key, QIODevice::WriteOnly); stream.setVersion(QDataStream::Qt_6_0);
    stream << root << host << path;
    auto active = m_cachedDownloads.find(key);
    if (active != m_cachedDownloads.end()) {
        active->requests.append(id); active->force = active->force || force;
        return id;
    }
    m_cachedDownloads.insert(key, CachedDownload{{id}, force});
    if (force) prepareCachedDownload(key, root, host, path);
    else DownloadCache::async<DownloadCache::Result>(this, [root, host, path] { return DownloadCache::lookup(root, {host, path}); },
        [this, key, root, host, path](const DownloadCache::Result &result) {
            const auto current = m_cachedDownloads.constFind(key);
            if (current == m_cachedDownloads.cend()) return;
            if (!result.error.isEmpty()) { finishCachedDownload(key, {}, result.error); return; }
            if (!result.path.isEmpty() && !current->force) { finishCachedDownload(key, result.path, {}); return; }
            prepareCachedDownload(key, root, host, path);
        });
    return id;
}

void HgsClient::prepareCachedDownload(const QByteArray &key, const QString &root, const QString &host, const QString &path)
{
    DownloadCache::async<DownloadCache::Prepared>(this, [root] { return DownloadCache::prepare(root); },
        [this, key, host, path](const DownloadCache::Prepared &prepared) {
            const auto active = m_cachedDownloads.constFind(key);
            if (active == m_cachedDownloads.cend()) return;
            if (!prepared.error.isEmpty()) { finishCachedDownload(key, {}, prepared.error); return; }
            transferFile(active->requests.first(), host, path, prepared.pending->path, prepared.pending, key);
        });
}

void HgsClient::finishCachedDownload(const QByteArray &key, const QString &path, const QString &error, const QString &warning)
{
    const auto active = m_cachedDownloads.take(key);
    const QPointer<HgsClient> alive(this);
    for (const auto id : active.requests) {
        if (!error.isEmpty()) emit fileDownloadFailed(id, error);
        else emit fileDownloaded(id, path);
        // A direct result slot can close the window and destroy its client.
        if (!alive) return;
    }
    if (!warning.isEmpty()) {
        emit downloadCacheWarning(warning);
        if (!alive) return;
    }
    if (error.isEmpty() && !active.requests.isEmpty()) scheduleDownloadCacheTrim();
}

quint64 HgsClient::requestApprovalLog(const QString &operation, const QJsonObject &data)
{
    const auto id = ++m_approvalLogRequest;
    const auto submit = [this, id, operation, data] {
        auto request = processRequest(m_hgs, {"approval-log", operation}); request.timeoutMs = 10000;
        request.input = QJsonDocument(data).toJson(QJsonDocument::Compact);
        ProcessRunner::run(std::move(request), this, [](ProcessRunner::Result result) { return parsedResult(std::move(result), 256 * 1024); },
                           [this, id, operation](const ParsedResult &parsed) {
            const auto &reply = parsed.result;
            QString error;
            const auto result = parsed.reply.object;
            if (reply.outcome != ProcessRunner::Result::Finished || reply.exitStatus != QProcess::NormalExit || reply.exitCode)
                error = QString::fromUtf8(reply.standardError).trimmed().left(2000);
            else if (reply.standardOutput.size() > 256 * 1024 || result.isEmpty()) error = tr("Invalid automatic approval journal response.");
            if (error.isEmpty() && (reply.outcome != ProcessRunner::Result::Finished || reply.exitStatus != QProcess::NormalExit || reply.exitCode))
                error = tr("The automatic approval journal is unavailable. No new approval was authorized.");
            emit approvalLogFinished(id, operation, result, error);
        });
    };
    // Unknown CLI words can launch an agent on retained versions. Probe the
    // configured local executable before ever using this additive command.
    if (m_approvalLogSupported) submit();
    else runHgs(m_hgs, {"--help"}, 5000, this, [this, id, operation, submit](const QByteArray &help) {
        if (!help.contains("hgs approval-log begin|finish|list|detail")) {
            emit approvalLogFinished(id, operation, {}, tr("Update the local hgs CLI to use automatic approvals and its journal.")); return;
        }
        m_approvalLogSupported = true; submit();
    }, [this, id, operation](const QString &, const QString &error) { emit approvalLogFinished(id, operation, {}, error); });
    return id;
}

void HgsClient::transferFile(quint64 id, const QString &host, const QString &path, const QString &destination,
                             const std::shared_ptr<DownloadCache::Pending> &pending, const QByteArray &cacheKey)
{
    const auto failed = [this, id, cacheKey](const QString &error) {
        if (cacheKey.isEmpty()) emit fileDownloadFailed(id, error);
        else finishCachedDownload(cacheKey, {}, error);
    };
    enum class Transfer { Saved, InvalidResponse, Unconfirmed };
    runHgsParsed(m_hgs, {'@' + host, "download", "--path", path, "--output", destination}, 135000, this,
        [host, path, destination](const QByteArray &out) {
            if (out.size() > 65536) return Transfer::InvalidResponse;
            const auto object = JsonParse::object(out);
            const QFileInfo saved(destination);
            if (object.value("host").toString() != host || object.value("path").toString() != path
                || object.value("local_path").toString() != destination || !object.value("bytes").isDouble()
                || !LocalFile::readable(saved) || object.value("bytes").toDouble() != LocalFile::size(saved)) return Transfer::Unconfirmed;
            return Transfer::Saved;
        },
        [this, id, host, path, destination, pending, cacheKey, failed](Transfer transfer) {
            if (transfer == Transfer::InvalidResponse) { failed(tr("Invalid file transfer response.")); return; }
            if (transfer == Transfer::Unconfirmed) { failed(tr("The file transfer did not confirm the saved local file.")); return; }
            if (!pending) { emit fileDownloaded(id, destination); return; }
            DownloadCache::async<DownloadCache::Result>(this,
                [pending, host, path] { return DownloadCache::publish(pending, path, -1, {host, path}); },
                [this, cacheKey](const DownloadCache::Result &result) {
                    finishCachedDownload(cacheKey, result.path, result.error, result.warning);
                });
        }, [pending, failed](const QString &, const QString &error) {
            failed(error.startsWith("hgs: ") ? error.mid(5) : error);
        });
}

void HgsClient::scheduleDownloadCacheTrim()
{
    if (!m_downloadCacheTimer) {
        m_downloadCacheTimer = new QTimer(this); m_downloadCacheTimer->setSingleShot(true);
        connect(m_downloadCacheTimer, &QTimer::timeout, this, &HgsClient::trimDownloadCache);
    }
    m_downloadCacheTimer->start(DownloadCache::OpeningGraceMs + 5000);
    QTimer::singleShot(0, this, &HgsClient::trimDownloadCache);
}

void HgsClient::trimDownloadCache()
{
    const auto limit = DownloadCache::limitBytes();
    if (!limit || m_downloadCacheTrimBusy) return;
    m_downloadCacheTrimBusy = true;
    const auto root = DownloadCache::directory();
    DownloadCache::async<DownloadCache::Usage>(this, [root, limit] { return DownloadCache::trim(root, limit); },
        [this](const DownloadCache::Usage &result) {
            m_downloadCacheTrimBusy = false;
            if (!result.error.isEmpty()) emit downloadCacheWarning(result.error);
        });
}

quint64 HgsClient::requestAttachment(const QString &host, const QString &name, const QJsonObject &file,
                                     const QString &conversation, const QString &archive, const QString &agent)
{
    const auto id=++m_attachmentRequest;
    QStringList args{"attachment",name,"--request",file.value("request_id").toString(),"--index",QString::number(file.value("index").toInt(-1)),"--conversation",conversation};
    if(!archive.isEmpty())args<<"--archive"<<archive;
    if(!agent.isEmpty())args<<"--agent"<<agent;
    if(!host.isEmpty())args.prepend('@'+host);
    struct Attachment { bool tooLarge=false, valid=false; QJsonObject metadata; QByteArray bytes; };
    runHgsParsed(m_hgs,args,host.isEmpty()?10000:45000,this,[file](const QByteArray &out){
        Attachment attachment;
        if(out.size()>MaximumAttachmentBytes*4/3+8192){attachment.tooLarge=true;return attachment;}
        const auto object=JsonParse::object(out);
        const auto encoded=object.value("data_base64").toString().toLatin1();
        const auto decoded=QByteArray::fromBase64Encoding(encoded,QByteArray::AbortOnBase64DecodingErrors);
        if(object.value("request_id")!=file.value("request_id")
            || object.value("index")!=file.value("index") || !decoded || decoded.decoded.isEmpty() || decoded.decoded.size()>MaximumAttachmentBytes)
            return attachment;
        attachment.valid=true;attachment.metadata=object;attachment.metadata.remove("data_base64");attachment.bytes=decoded.decoded;
        return attachment;
    },[this,id](const Attachment &attachment){
        if(attachment.tooLarge){emit attachmentFailed(id,tr("Attachment response is too large."));return;}
        if(!attachment.valid){emit attachmentFailed(id,tr("Invalid attachment response. Try again."));return;}
        emit attachmentReady(id,attachment.metadata,attachment.bytes);
    },[this,id](const QString &,const QString &error){emit attachmentFailed(id,error);});
    return id;
}

void HgsClient::launchNativeSession(const QString &host, const QString &target, const QString &name, const QString &account, const QString &launchId)
{
    launchDetachedSession(host,"dsh",target,name,account,launchId);
}

void HgsClient::launchDetachedSession(const QString &host, const QString &agent, const QString &target, const QString &name, const QString &account, const QString &launchId, const QString &model, const QString &effort,const QString &expectedMachine)
{
    QStringList args{agent, target, "--new", "-n", name, "--launch-id", launchId};
    if (agent != "dsh" && agent != "opencode") args << "-d";
    if (!account.isEmpty()) args << "--account" << account;
    if (agent == "opencode" && !model.isEmpty()) args << "--model" << model;
    // hgs requires --model with --effort: effort is that model's variant.
    if (agent == "opencode" && !model.isEmpty() && !effort.isEmpty()) args << "--effort" << effort;
    if (!host.isEmpty()) args.prepend('@' + host);
    guardMachine(args,expectedMachine);
    runHgs(m_hgs, args, 90000, this,
        [this,launchId](const QByteArray &) { emit nativeSessionLaunched(launchId,true,{}); },
        [this,launchId](const QString &,const QString &error) { emit nativeSessionLaunched(launchId,false,error); });
}

void HgsClient::requestSubagentInspection(const QString &host, const QString &name, const QString &agentId, const QString &archiveId)
{
    const QString key = host + '\n' + name + '\n' + archiveId + '\n' + agentId;
    if (m_inspectionsInFlight.contains(key)) return;
    m_inspectionsInFlight.insert(key);
    emit inspectionLoadingChanged(host, name, archiveId, agentId, true);
    QStringList args{"inspect", name, "--agent", agentId, "--skip-processes"};
    if (!archiveId.isEmpty()) args << "--archive" << archiveId;
    if (!host.isEmpty()) args.prepend('@' + host);
    runHgsParsed(m_hgs, args, host.isEmpty() ? 5000 : 15000, this, parseObject,
        [this,host,name,agentId,archiveId,key](const ObjectReply &doc) {
            m_inspectionsInFlight.remove(key);
            emit inspectionLoadingChanged(host, name, archiveId, agentId, false);
            if (!doc.ok())
                emit subagentInspectionFailed(host,name,agentId,archiveId,tr("Invalid subagent activity response"));
            else emit subagentInspectionReady(host,name,agentId,archiveId,doc.object);
        }, [this,host,name,agentId,archiveId,key](const QString &,const QString &error) {
            m_inspectionsInFlight.remove(key);
            emit inspectionLoadingChanged(host, name, archiveId, agentId, false);
            emit subagentInspectionFailed(host,name,agentId,archiveId,error);
        });
}

void HgsClient::runWrite(const QString &op, const QStringList &args, int timeoutMs,
                         bool sessionWrite, const QString &host)
{
    auto request = processRequest(m_hgs, args); request.timeoutMs = timeoutMs;
    struct Write { ProcessRunner::Result result; QJsonObject cleanup; int cleanups = 0; };
    ProcessRunner::run(std::move(request), this, [](ProcessRunner::Result result) {
        Write write;
        for (const auto &line : result.standardError.split('\n')) if (line.startsWith("ZERUS_WORKTREE_CLEANUP ")) {
            write.cleanup = JsonParse::object(line.mid(23)); ++write.cleanups;
        }
        write.result = std::move(result);
        return write;
    }, [this, op, sessionWrite, host](const Write &write) {
        const auto &result = write.result;
        // Any QProcess error (failed start, crash) reports its own text, as the
        // errorOccurred() that precedes finished() did before.
        const bool timedOut = result.outcome == ProcessRunner::Result::TimedOut;
        const bool error = !timedOut && result.error != QProcess::UnknownError;
        const bool ok = !timedOut && !error && result.exitCode == 0 && result.exitStatus == QProcess::NormalExit;
        emit writeDone(op, ok, timedOut ? QStringLiteral("timed out") : error ? result.errorString
                                        : QString::fromLocal8Bit(result.standardError).trimmed());
        if (ok && (op.startsWith("archive ") || op.startsWith("terminate "))) {
            const auto &cleanup = write.cleanup; const int count = write.cleanups;
            if (count == 1 && !cleanup["path"].toString().isEmpty() && !cleanup["common_dir"].toString().isEmpty()) {
                if (cleanup["status"] == "removed") invalidateWorktreeCache(host, cleanup["common_dir"].toString());
                emit worktreeCleanupFinished(host, op, cleanup);
            }
        }
        // A failed batch can have completed some sessions. Refresh its own machine
        // even on failure, without mixing it up with concurrent project edits.
        if (sessionWrite)
            emit sessionWriteDone(host);
    });
}

void HgsClient::forkSession(const QString &host, const QString &name, const QString &tag, const QString &archive, const QString &run, const QString &conversation)
{
    QStringList args{"fork", name, "-n", tag, "-d"};
    if (!archive.isEmpty()) args << "--archive" << archive;
    if (!run.isEmpty()) args << "--expected-run-id" << run;
    if (!conversation.isEmpty()) args << "--expected-conversation-id" << conversation;
    if (!host.isEmpty()) args.prepend('@' + host);
    runWrite(tr("fork %1").arg(name), args, host.isEmpty() ? 60000 : 90000, true, host);
}

void HgsClient::terminateSession(const QString &host, const QString &name, const QString &run)
{
    QStringList args{"terminate",name};
    if(!run.isEmpty())args<<"--expected-run-id"<<run;
    if(!host.isEmpty())args.prepend('@'+host);
    runWrite(QStringLiteral("terminate %1").arg(name),args,host.isEmpty()?45000:60000,true,host);
}

void HgsClient::killSession(const QString &host, const QString &name)
{
    const QString op = QStringLiteral("kill %1").arg(name);
    if (host.isEmpty()) {
        // Локальный kill — это exec tmux kill-session, доли миллисекунд: тот же бюджет,
        // что у requestLocal()/requestProjects().
        runWrite(op, {QStringLiteral("kill"), name}, 5000, true, host);
        return;
    }
    // Пиру -- через ssh (см. dispatch в hgs: kill получает ConnectTimeout=3, но не
    // ServerAliveInterval), поэтому бюджет как у requestPeer(): спящий ноут держит
    // рукопожатие до 15с, а не подвисает навсегда.
    runWrite(op, {QLatin1Char('@') + host, QStringLiteral("kill"), name}, 15000, true, host);
}

void HgsClient::openNativeUi(const QString &host, const QString &name)
{
    QStringList args{"native-ui",name}; if (!host.isEmpty()) args.prepend('@'+host);
    runWrite(tr("open native DeepSeek UI"),args,host.isEmpty()?45000:60000,false,host);
}

quint64 HgsClient::requestNativeUi(const QString &host, const QString &name)
{
    const auto request = ++m_nativeUiRequest;
    QStringList args{"native-ui", name, "--json"}; if (!host.isEmpty()) args.prepend('@' + host);
    runHgsParsed(m_hgs, args, host.isEmpty() ? 45000 : 60000, this, parseObject,
        [this, request](const ObjectReply &doc) {
            if (!doc.ok() || !doc.object.value("url").isString()) {
                emit nativeUiFailed(request, tr("Invalid native UI connection response")); return;
            }
            emit nativeUiReady(request, doc.object);
        }, [this, request](const QString &, const QString &error) { emit nativeUiFailed(request, error); });
    return request;
}

void HgsClient::pauseSession(const QString &host, const QString &name)
{
    QStringList args{QStringLiteral("pause"), name};
    if (!host.isEmpty())
        args.prepend(QLatin1Char('@') + host);
    // CLI waits up to 10s for a graceful exit, plus history verification and SSH.
    runWrite(QStringLiteral("pause %1").arg(name), args, 45000, true, host);
}

void HgsClient::resumeSession(const QString &host, const QString &name)
{
    QStringList args{QStringLiteral("resume"), name, QStringLiteral("-d")};
    if (!host.isEmpty())
        args.prepend(QLatin1Char('@') + host);
    runWrite(QStringLiteral("resume %1").arg(name), args, 30000, true, host);
}

void HgsClient::startFreshSession(const QString &host, const QString &name, const QString &run)
{
    QStringList args{QStringLiteral("resume"), name, QStringLiteral("--fresh")};
    if (!run.isEmpty()) args << QStringLiteral("--expected-run-id") << run;
    args << QStringLiteral("-d");
    if (!host.isEmpty()) args.prepend(QLatin1Char('@') + host);
    runWrite(QStringLiteral("fresh %1").arg(name), args, 30000, true, host);
}

void HgsClient::archiveSession(const QString &host, const QString &name)
{
    QStringList args{QStringLiteral("archive"), name};
    if (!host.isEmpty()) args.prepend(QLatin1Char('@') + host);
    runWrite(QStringLiteral("archive %1").arg(name), args, host.isEmpty() ? 30000 : 45000, true, host);
}

void HgsClient::renameSession(const QString &host, const QString &name, const QString &newName, const QString &archiveId)
{
    QStringList args{QStringLiteral("rename"), name, newName};
    if (!archiveId.isEmpty()) args << QStringLiteral("--archive") << archiveId;
    if (!host.isEmpty()) args.prepend(QLatin1Char('@') + host);
    runWrite(QStringLiteral("rename %1").arg(name), args, host.isEmpty() ? 5000 : 15000, true, host);
}

void HgsClient::restoreArchive(const QString &host, const QString &name, const QString &archiveId)
{
    // Archive identity must never collapse to a generic resume of a reused name.
    if (archiveId.isEmpty()) {
        emit writeDone(QStringLiteral("restore %1").arg(name), false, tr("Missing archive identity"));
        return;
    }
    QStringList args{QStringLiteral("resume"), name, QStringLiteral("--archive"), archiveId, QStringLiteral("-d")};
    if (!host.isEmpty()) args.prepend(QLatin1Char('@') + host);
    runWrite(QStringLiteral("restore %1").arg(name), args, 30000, true, host);
}

void HgsClient::forgetArchive(const QString &host, const QString &name, const QString &archiveId)
{
    if (archiveId.isEmpty()) {
        emit writeDone(QStringLiteral("forget archive %1").arg(name), false, tr("Missing archive identity"));
        return;
    }
    QStringList args{QStringLiteral("kill"), name, QStringLiteral("--archive"), archiveId};
    if (!host.isEmpty()) args.prepend(QLatin1Char('@') + host);
    runWrite(QStringLiteral("forget archive %1").arg(name), args, host.isEmpty() ? 5000 : 15000, true, host);
}

void HgsClient::pauseAll(const QString &host, int count)
{
    QStringList args{QStringLiteral("pause"), QStringLiteral("--all")};
    if (!host.isEmpty())
        args.prepend(QLatin1Char('@') + host);
    // History verification and graceful exit are sequential for each session.
    runWrite(QStringLiteral("pause all on %1").arg(host.isEmpty() ? tr("this box") : host),
             args, 15000 + 30000 * qBound(1, count, 10000), true, host);
}

void HgsClient::resumeAll(const QString &host, int count)
{
    QStringList args{QStringLiteral("resume"), QStringLiteral("--all"), QStringLiteral("-d")};
    if (!host.isEmpty())
        args.prepend(QLatin1Char('@') + host);
    runWrite(QStringLiteral("resume all on %1").arg(host.isEmpty() ? tr("this box") : host),
             args, 15000 + 15000 * qBound(1, count, 10000), true, host);
}

void HgsClient::projectSet(const QString &name, const QString &dir, const QString &host)
{
    const QString op = QStringLiteral("save project %1").arg(name);
    // projects.local — файл "одна запись на строку" (name=dir), который читает и
    // Ansible-оверлей, и резолвер проекта, и это же меню. hgs сам меняет каталог через
    // `cd "$dir" && pwd`, что молча пропускает перевод строки внутри пути насквозь, а
    // project_write() пишет его в файл как есть — запись обрывается на первом \n и
    // остаток превращается в мусорную "фантомную" строку без имени. Это порча самой
    // записи, которую пользователь только что сохранил, а не абстрактная гигиена —
    // поэтому чек здесь, до запуска процесса, а не патч в hgs (вне tray/ в этой
    // задаче, да и остальную валидацию имени hgs и так делает сам, дублировать незачем).
    if (dir.contains(QLatin1Char('\n')) || dir.contains(QLatin1Char('\r'))) {
        // Сигналим через singleShot(0), а не emit прямо здесь: writeDone для всех
        // остальных исходов приходит асинхронно, через цикл событий (процесс всегда
        // запускается и завершается позже) -- если этот единственный синхронный путь
        // прилетит раньше app.exec(), QCoreApplication::exit() из обработчика
        // окажется вызван до того, как цикл вообще стартовал, и тихо потеряется
        // (проверено: без singleShot ранер зависает до собственного канареечного
        // таймаута). singleShot(0) держит контракт "всегда через loop" одинаковым
        // для всех трёх исходов.
        QTimer::singleShot(0, this, [this, op]() {
            emit writeDone(op, false,
                            QStringLiteral("the path has a newline in it — it would corrupt "
                                            "projects.local"));
        });
        return;
    }
    QStringList args{QStringLiteral("project"), QStringLiteral("set"), name, dir};
    if (!host.isEmpty()) args.prepend(QLatin1Char('@') + host);
    runWrite(op, args, host.isEmpty() ? 5000 : 15000);
}

void HgsClient::projectRemove(const QString &name, const QString &host)
{
    QStringList args{QStringLiteral("project"), QStringLiteral("rm"), name};
    if (!host.isEmpty()) args.prepend(QLatin1Char('@') + host);
    runWrite(QStringLiteral("remove project %1").arg(name), args, host.isEmpty() ? 5000 : 15000);
}

void HgsClient::requestProjects(const QString &host)
{
    QStringList args{QStringLiteral("project"), QStringLiteral("ls"), QStringLiteral("--json")};
    if (!host.isEmpty()) args.prepend(QLatin1Char('@') + host);
    struct Projects { QList<ProjectInfo> projects; QString error; };
    runHgsParsed(m_hgs, args, host.isEmpty() ? 5000 : 15000, this,
           [](const QByteArray &out) { Projects parsed; parsed.projects = parseProjects(out, &parsed.error); return parsed; },
           [this, host](const Projects &parsed) {
               const QString &err = parsed.error;
               const auto &projects = parsed.projects;
               if (!err.isEmpty()) { emit projectsFailed(host, err); return; }
               if (host.isEmpty()) emit projectsReady(projects);
               emit projectsForHostReady(host, projects);
           }, [this, host](const QString &, const QString &detail) { emit projectsFailed(host, detail); });
}

namespace {
// sizes holds each entry's reply size, measured when its reply was parsed,
// so trimming never serializes the cache on the GUI thread.
void trimWorktreeCache(QHash<QString,QJsonObject> &cache,QHash<QString,qsizetype> &sizes) {
    qsizetype bytes=0;for(auto i=cache.cbegin();i!=cache.cend();++i)bytes+=sizes.value(i.key());
    while(cache.size()>64||bytes>16*1024*1024) {
        auto oldest=cache.begin();
        for(auto i=cache.begin();i!=cache.end();++i)if(i.value()["received_at"].toDouble()<oldest.value()["received_at"].toDouble())oldest=i;
        bytes-=sizes.take(oldest.key());cache.erase(oldest);
    }
}

// The desktop's saved worktree catalogs. SettingsJson parses them once (the
// tray warms the key at startup); writers record what they stored.
const QString WorktreeSnapshotsKey = QStringLiteral("workspace/worktreeSnapshots");
constexpr qsizetype WorktreeSnapshotsLimit = 8 * 1024 * 1024;
void saveWorktreeSnapshots(QJsonObject snapshots)
{
    while(snapshots.size()>64||QJsonDocument(snapshots).toJson(QJsonDocument::Compact).size()>WorktreeSnapshotsLimit){QString oldest;double time=std::numeric_limits<double>::max();for(auto i=snapshots.begin();i!=snapshots.end();++i)if(i.value().toObject()["sampled_at"].toDouble()<time){oldest=i.key();time=i.value().toObject()["sampled_at"].toDouble();}snapshots.remove(oldest);}
    const QJsonDocument document(snapshots);
    const auto bytes=document.toJson(QJsonDocument::Compact);
    QSettings().setValue(WorktreeSnapshotsKey,bytes);
    SettingsJson::remember(WorktreeSnapshotsKey,bytes,document);
}
QString worktreeCacheKey(const QString &executable,const QString &host,const QString &path) {
    return QString::fromLatin1(QCryptographicHash::hash((executable+'\n'+host+'\n'+path).toUtf8(),QCryptographicHash::Sha256).toHex());
}
}
QJsonObject HgsClient::worktreeSnapshot(const QString &host,const QString &path) const
{
    return SettingsJson::object(WorktreeSnapshotsKey,WorktreeSnapshotsLimit).value(worktreeCacheKey(m_hgs,host,path)).toObject();
}
QJsonObject HgsClient::gitStatusSnapshot(const QString &host, const QString &path) const
{
    return m_gitStatusCache.value(host + '\n' + path);
}

void HgsClient::requestGitStatus(const QString &host, const QString &path)
{
    if (path.isEmpty()) return;
    const auto key = host + '\n' + path;
    const auto age = QDateTime::currentMSecsSinceEpoch() - m_gitStatusRequestedAt.value(key);
    if (m_gitStatusRequestedAt.contains(key) && age >= 0 && age < 15000) return;
    if (m_gitStatusInFlight.contains(key) || m_gitStatusQueue.contains({host, path})) return;
    // Bound both subprocess concurrency and old folder snapshots.
    if (m_gitStatusQueue.size() >= 64) return;
    m_gitStatusQueue.append({host, path}); pumpGitStatus();
}

void HgsClient::pumpGitStatus()
{
    while (m_gitStatusInFlight.size() < 2 && !m_gitStatusQueue.isEmpty()) {
        const auto folder = m_gitStatusQueue.takeFirst();
        const auto host = folder.first, path = folder.second, key = host + '\n' + path;
        m_gitStatusInFlight.insert(key);
        const auto finish = [this, host, path, key](QJsonObject data) {
            const auto received = QDateTime::currentMSecsSinceEpoch();
            if (!data.value("state").isString()) data = {{"state", "unavailable"}};
            data["received_at_ms"] = received;
            m_gitStatusCache.insert(key, data); m_gitStatusRequestedAt.insert(key, received);
            m_gitStatusInFlight.remove(key);
            while (m_gitStatusCache.size() > 128) {
                QString oldest; qint64 at = std::numeric_limits<qint64>::max();
                for (auto i = m_gitStatusRequestedAt.cbegin(); i != m_gitStatusRequestedAt.cend(); ++i)
                    if (!m_gitStatusInFlight.contains(i.key()) && i.value() < at) { oldest = i.key(); at = i.value(); }
                m_gitStatusCache.remove(oldest); m_gitStatusRequestedAt.remove(oldest);
            }
            emit gitStatusChanged(host, path); pumpGitStatus();
        };
        QStringList args{"git-status", "--path", path, "--json"};
        if (!host.isEmpty()) args.prepend('@' + host);
        runHgsParsed(m_hgs, args, host.isEmpty() ? 5000 : 10000, this,
            [](const QByteArray &out) { return out.size() <= 32768 ? JsonParse::object(out) : QJsonObject(); },
            [finish](const QJsonObject &data) { finish(data); },
            [finish](const QString &, const QString &) { finish({}); });
    }
}

quint64 HgsClient::requestWorktrees(const QString &host,const QString &path,bool refresh,const QString &expectedMachine)
{
    if(!expectedMachine.isEmpty()){
        const auto request=++m_worktreeRequest;QStringList args{"worktrees","--creation","--path",path,"--refresh","--json"};
        if(!host.isEmpty())args.prepend('@'+host);guardMachine(args,expectedMachine);
        runHgsParsed(m_hgs,args,15000,this,[](const QByteArray &out){return JsonParse::object(out);},[this,request,host,path](const QJsonObject &data){
            if(!data["worktrees"].isArray()||!data["state"].isString())emit worktreesFailed(request,host,path,tr("Invalid verified worktree catalog."));
            else emit worktreesReady(request,host,path,data);
        },[this,request,host,path](const QString &,const QString &error){emit worktreesFailed(request,host,path,error);});
        return request;
    }
    const quint64 request=++m_worktreeRequest;
    const auto cached=worktreeSnapshot(host,path);
    const auto age=QDateTime::currentSecsSinceEpoch()-qint64(cached.value("sampled_at").toDouble());
    const auto hostKey=QString::fromUtf8(QJsonDocument(QJsonArray{host}).toJson(QJsonDocument::Compact));
    const auto hostGeneration=m_worktreeCacheGenerations.value(hostKey);
    if(!refresh&&!cached.isEmpty()&&!cached.value("stale").toBool()&&age>=0&&age<30){
        QTimer::singleShot(0,this,[this,request,host,path,cached,hostKey,hostGeneration]{
            if(m_worktreeCacheGenerations.value(hostKey)!=hostGeneration)emit worktreesFailed(request,host,path,tr("Folder changed. Refresh its catalog."));
            else emit worktreesReady(request,host,path,cached);
        });return request;
    }
    const auto repository=host+'\n'+cached["common_dir"].toString();
    const auto generation=m_worktreeCacheGenerations.value(repository);
    const auto key=host+'\n'+path+'\n'+QString::number(generation)+':'+QString::number(hostGeneration);
    if(m_worktreesInFlight.contains(key)){m_worktreesInFlight[key].append({request,path});return request;}
    m_worktreesInFlight[key].append({request,path});
    QStringList args{"worktrees","--path",path,"--json"};if(refresh)args<<"--refresh";if(!host.isEmpty())args.prepend('@'+host);
    runHgsParsed(m_hgs,args,host.isEmpty()?6000:15000,this,[](const QByteArray &out){return out.size()<=2*1024*1024?JsonParse::object(out):QJsonObject();},
                 [this,host,key,repository,generation,hostKey,hostGeneration](QJsonObject data){
        const auto requests=m_worktreesInFlight.take(key);
        if(m_worktreeCacheGenerations.value(repository)!=generation||m_worktreeCacheGenerations.value(hostKey)!=hostGeneration) {
            for(const auto &r:requests)emit worktreesFailed(r.first,host,r.second,tr("Folder changed. Refresh its catalog."));return;
        }
        data["received_at"] = QDateTime::currentSecsSinceEpoch();
        if(!data.value("worktrees").isArray()||!data.value("state").isString()){
            for(const auto &r:requests)emit worktreesFailed(r.first,host,r.second,tr("Invalid worktree catalog response"));return;
        }
        auto snapshots=SettingsJson::object(WorktreeSnapshotsKey);
        auto store=[&](const QString &path,QJsonObject value){snapshots[worktreeCacheKey(m_hgs,host,path)]=value;};
        for(const auto &r:requests)store(r.second,data);
        if(data["state"]=="ok")for(const auto &v:data["worktrees"].toArray()){
            const auto root=v.toObject()["path"].toString();auto sibling=data;sibling["path"]=root;sibling["selected_root"]=root;store(root,sibling);
        }
        saveWorktreeSnapshots(snapshots);
        for(const auto &r:requests)emit worktreesReady(r.first,host,r.second,data);
    },[this,host,key](const QString &,const QString &detail){for(const auto &r:m_worktreesInFlight.take(key))emit worktreesFailed(r.first,host,r.second,detail);});
    return request;
}

quint64 HgsClient::createWorktree(const QString &host,const QString &path,const QString &commonDir,
    const QString &branch,const QString &base,const QString &destination,const QString &requestId,bool receipt,const QString &expectedMachine)
{
    const auto request=++m_worktreeRequest;
    const auto token=requestId.isEmpty()?QUuid::createUuid().toString(QUuid::WithoutBraces):requestId;
    QStringList args{"worktrees","create","--path",path,"--common-dir",commonDir,"--branch",branch,
        "--base",base,"--destination",destination,"--request-id",token,"--json"};
    if(receipt)args<<"--receipt";
    if(!host.isEmpty())args.prepend('@'+host);
    guardMachine(args,expectedMachine);
    runHgsParsed(m_hgs,args,330000,this,parseJsonObject,[this,request,token,branch,commonDir,host,receipt](const QJsonObject &data){
        invalidateWorktreeCache(host, commonDir);
        if(receipt&&data["request_id"]==token&&data["status"]!="created"){
            emit worktreeCreated(request,false,data,data["detail"].toString(tr("Worktree creation is not confirmed. Check its original result.")));return;
        }
        if(data["request_id"]!=token||data["status"]!="created"||data["branch"]!=branch||data["common_dir"]!=commonDir
            ||!QFileInfo(data["path"].toString()).isAbsolute()){
            emit worktreeCreated(request,false,{},tr("Creation could not be confirmed. Refresh the catalog and check the destination before retrying."));return;
        }
        emit worktreeCreated(request,true,data,{});
    },[this,request,destination,host,commonDir](const QString &,const QString &error){
        invalidateWorktreeCache(host, commonDir);
        emit worktreeCreated(request,false,{},error+tr("\nCheck %1 before retrying. Any created worktree is kept.").arg(destination));
    });
    return request;
}

quint64 HgsClient::worktreeReceipt(const QString &host,const QString &action,const QString &requestId,const QString &expectedMachine)
{
    const auto request=++m_worktreeRequest;
    QStringList args{"worktrees",action,"--request-id",requestId,"--json"};
    if(!host.isEmpty())args.prepend('@'+host);
    guardMachine(args,expectedMachine);
    runHgsParsed(m_hgs,args,15000,this,parseJsonObject,[this,request,requestId](const QJsonObject &result){
        const bool ok=result["schema"]==1&&result["request_id"]==requestId&&result["status"].isString();
        emit worktreeReceiptReady(request,ok,result,ok?QString():tr("Invalid worktree receipt."));
    },[this,request](const QString &,const QString &error){emit worktreeReceiptReady(request,false,{},error);});
    return request;
}

namespace {
QString cleanupError(const QString &host,const QString &detail) {
    // An older hgs prints the catalog usage for the new review/remove actions.
    if(detail.contains(QStringLiteral("usage: hgs worktrees --path")))
        return host.isEmpty()?HgsClient::tr("Update Zerus on this computer to clean up worktrees."):HgsClient::tr("Update Zerus on %1 to clean up worktrees.").arg(host);
    return detail;
}
}
quint64 HgsClient::reviewWorktrees(const QString &host,const QString &path,const QString &worktree)
{
    const auto request=++m_worktreeRequest;
    const auto repository=host+'\n'+path;const auto generation=m_worktreeCacheGenerations.value(repository);
    const auto key=host+'\n'+path+'\n'+worktree+'\n'+QString::number(generation);
    if(m_reviewsInFlight.contains(key)){m_reviewsInFlight[key].append(request);return request;}
    m_reviewsInFlight[key].append(request);
    QStringList args{"worktrees","review","--path",path,"--json"};if(!worktree.isEmpty())args<<"--worktree"<<worktree;if(!host.isEmpty())args.prepend('@'+host);
    struct Review { QJsonObject data; qsizetype bytes=0; };
    runHgsParsed(m_hgs,args,host.isEmpty()?30000:45000,this,[](const QByteArray &out){return Review{out.size()<=4*1024*1024?JsonParse::object(out):QJsonObject(),out.size()};},
                 [this,host,path,key,worktree,repository,generation](const Review &review){
        const auto &data=review.data;
        const auto requests=m_reviewsInFlight.take(key);
        if(m_worktreeCacheGenerations.value(repository)!=generation) {
            for(const auto id:requests)emit worktreeReviewFailed(id,host,path,tr("Folder changed. Refresh its cleanup review."));return;
        }
        if(!data.value("worktrees").isArray()||!data.value("state").isString()){for(const auto r:requests)emit worktreeReviewFailed(r,host,path,tr("Invalid worktree review response"));return;}
        if(data["state"]=="ok") {
            const auto repository = host+'\n'+data["common_dir"].toString();
            auto merged = worktree.isEmpty() ? data : m_worktreeReviews.value(repository);
            QJsonArray rows;
            if(!worktree.isEmpty()) for(const auto &row:merged["worktrees"].toArray()) if(row.toObject()["path"]!=worktree) rows.append(row);
            for(const auto &value:data["worktrees"].toArray()) {
                auto row=value.toObject(); if(!worktree.isEmpty()&&row["path"]!=worktree) continue;
                row["sampled_at"]=data["sampled_at"]; rows.append(row);
            }
            if(!worktree.isEmpty()) {
                auto targeted=merged["targeted_paths"].toArray();if(!targeted.contains(worktree))targeted.append(worktree);merged["targeted_paths"]=targeted;
            }
            merged["worktrees"]=rows; merged["sampled_at"]=data["sampled_at"];
            merged["received_at"]=QDateTime::currentSecsSinceEpoch();
            m_worktreeReviews.insert(repository,merged);
            m_worktreeReviewBytes[repository]=(worktree.isEmpty()?0:m_worktreeReviewBytes.value(repository))+review.bytes;
            trimWorktreeCache(m_worktreeReviews,m_worktreeReviewBytes);
        }
        for(const auto r:requests)emit worktreeReviewReady(r,host,path,data);
    },[this,host,path,key](const QString &,const QString &detail){for(const auto r:m_reviewsInFlight.take(key))emit worktreeReviewFailed(r,host,path,cleanupError(host,detail));});
    return request;
}
QJsonObject HgsClient::worktreeReview(const QString &host,const QString &commonDir,int maxAgeSeconds) const
{
    const auto data=m_worktreeReviews.value(host+'\n'+commonDir);
    const auto age=QDateTime::currentSecsSinceEpoch()-qint64(data.value("sampled_at").toDouble());
    if(age<0||age>=maxAgeSeconds)return {};
    auto result=data;QJsonArray rows;
    for(const auto &value:data["worktrees"].toArray()) {
        const auto row=value.toObject();const auto rowAge=QDateTime::currentSecsSinceEpoch()-qint64(row["sampled_at"].toDouble(data["sampled_at"].toDouble()));
        if(rowAge>=0&&rowAge<maxAgeSeconds)rows.append(row);
    }
    result["worktrees"]=rows;return result;
}
void HgsClient::invalidateWorktreeCache(const QString &host,const QString &commonDir)
{
    ++m_worktreeCacheGenerations[QString::fromUtf8(QJsonDocument(QJsonArray{host}).toJson(QJsonDocument::Compact))];
    ++m_worktreeCacheGenerations[host+'\n'+commonDir];
    m_worktreeReviews.remove(host+'\n'+commonDir); m_worktreeReviewBytes.remove(host+'\n'+commonDir);
    for(auto i=m_worktreeInspections.begin();i!=m_worktreeInspections.end();) {
        const auto scope=m_worktreeInspectionScopes.value(i.key());
        if(scope.value(0)==host&&scope.value(2)==commonDir) {m_worktreeInspectionScopes.remove(i.key());m_worktreeInspectionBytes.remove(i.key());i=m_worktreeInspections.erase(i);} else ++i;
    }
    auto snapshots=SettingsJson::object(WorktreeSnapshotsKey);
    for(auto i=snapshots.begin();i!=snapshots.end();++i) {
        auto value=i.value().toObject();
        // Keys include executable and host: another computer's identical path is unrelated.
        if(value["common_dir"]==commonDir&&i.key()==worktreeCacheKey(m_hgs,host,value["path"].toString())) {value["stale"]=true;i.value()=value;}
    }
    saveWorktreeSnapshots(snapshots);
    emit worktreeCacheInvalidated(host,commonDir);
}
void HgsClient::invalidateWorktreeInspection(const QString &host,const QString &path,const QString &commonDir)
{
    const auto target=QString::fromUtf8(QJsonDocument(QJsonArray{host,path,commonDir}).toJson(QJsonDocument::Compact));
    ++m_worktreeCacheGenerations[target];
    for(auto i=m_worktreeInspections.begin();i!=m_worktreeInspections.end();) {
        const auto entry=m_worktreeInspectionScopes.value(i.key());
        if(entry.value(0)==host&&entry.value(1)==path&&entry.value(2)==commonDir){m_worktreeInspectionScopes.remove(i.key());m_worktreeInspectionBytes.remove(i.key());i=m_worktreeInspections.erase(i);}else ++i;
    }
}
quint64 HgsClient::inspectWorktree(const QString &host,const QString &path,const QString &commonDir,const QString &file,int offset,bool refresh)
{
    const auto request=++m_worktreeRequest;
    const auto scope=QString::fromUtf8(QJsonDocument(QJsonArray{host,path,commonDir,file,offset}).toJson(QJsonDocument::Compact));
    const auto target=QString::fromUtf8(QJsonDocument(QJsonArray{host,path,commonDir}).toJson(QJsonDocument::Compact));
    if(refresh)invalidateWorktreeInspection(host,path,commonDir);
    const auto cached=m_worktreeInspections.value(scope);
    const auto age=QDateTime::currentSecsSinceEpoch()-qint64(cached["received_at"].toDouble());
    if(!refresh&&!cached.isEmpty()&&age>=0&&age<3600) {
        const auto revision=m_worktreeCacheGenerations.value(host+'\n'+commonDir), targetRevision=m_worktreeCacheGenerations.value(target);
        QTimer::singleShot(0,this,[this,request,host,path,file,cached,commonDir,revision,target,targetRevision]{
            if(revision!=m_worktreeCacheGenerations.value(host+'\n'+commonDir)||targetRevision!=m_worktreeCacheGenerations.value(target))
                emit worktreeInspectionFinished(request,host,path,file,{},tr("Folder data changed. Refresh to inspect it again."));
            else emit worktreeInspectionFinished(request,host,path,file,cached,{});
        });return request;
    }
    const auto repository=host+'\n'+commonDir;
    const auto generation=m_worktreeCacheGenerations.value(repository), targetGeneration=m_worktreeCacheGenerations.value(target);
    const auto flight=scope+QString::number(generation)+':'+QString::number(targetGeneration);
    if(m_worktreeInspectionsInFlight.contains(flight)){m_worktreeInspectionsInFlight[flight].append(request);return request;}
    m_worktreeInspectionsInFlight[flight].append(request);
    const auto finish=[this,scope,flight,repository,generation,target,targetGeneration,host,path,commonDir,file](QJsonObject data,QString error,qsizetype bytes) {
        if(m_worktreeCacheGenerations.value(repository)!=generation||m_worktreeCacheGenerations.value(target)!=targetGeneration){data={};error=tr("Folder data changed. Refresh to inspect it again.");}
        if(error.isEmpty()) {
            data["received_at"]=QDateTime::currentSecsSinceEpoch(); m_worktreeInspections[scope]=data;
            m_worktreeInspectionBytes[scope]=bytes; m_worktreeInspectionScopes[scope]=QStringList{host,path,commonDir};
            // File contents live only in bounded process memory, never in QSettings.
            trimWorktreeCache(m_worktreeInspections,m_worktreeInspectionBytes);
            for(auto i=m_worktreeInspectionScopes.begin();i!=m_worktreeInspectionScopes.end();)
                if(!m_worktreeInspections.contains(i.key()))i=m_worktreeInspectionScopes.erase(i);else ++i;
        }
        for(const auto id:m_worktreeInspectionsInFlight.take(flight))emit worktreeInspectionFinished(id,host,path,file,data,error);
    };
    QStringList args{"worktrees","inspect","--path",path,"--common-dir",commonDir,"--json"};
    if(!file.isEmpty())args<<"--file"<<file;if(offset>0)args<<"--offset"<<QString::number(offset);if(!host.isEmpty())args.prepend('@'+host);
    struct Inspection { QJsonObject data; bool verified=false; qsizetype bytes=0; };
    runHgsParsed(m_hgs,args,host.isEmpty()?30000:45000,this,[path,commonDir,file,offset](const QByteArray &out){
        Inspection inspection; inspection.bytes=out.size();
        inspection.data=out.size()<=4*1024*1024?JsonParse::object(out):QJsonObject();
        const auto &data=inspection.data;
        inspection.verified=!(data["state"]!="ok"||data["path"]!=path||data["common_dir"]!=commonDir
           ||(!file.isEmpty()&&data["file"].toObject()["path"]!=file)
           ||(offset>0&&data["file"].toObject()["offset"].toInt(-1)!=offset));
        return inspection;
    },[finish](const Inspection &inspection){
        if(!inspection.verified){finish({},tr("Worktree inspection could not be verified. Refresh and try again."),0);return;}
        finish(inspection.data,{},inspection.bytes);
    },[finish](const QString &,const QString &error){finish({},error.contains("usage: hgs worktrees")?tr("Update Zerus on this computer to inspect worktree changes."):error,0);});
    return request;
}

quint64 HgsClient::removeWorktree(const QString &host,const QString &path,const QString &commonDir,const QString &fingerprint,bool deleteBranch,const QString &ignoredFingerprint,const QString &discardFingerprint,const QString &processFingerprint,bool preserveArchiveRestore)
{
    const auto request=++m_worktreeRequest;const auto token=QUuid::createUuid().toString(QUuid::WithoutBraces);
    QStringList args{"worktrees","remove","--path",path,"--common-dir",commonDir,"--fingerprint",fingerprint,"--request-id",token,"--json"};
    if(deleteBranch)args<<"--delete-branch";if(!host.isEmpty())args.prepend('@'+host);
    if(!ignoredFingerprint.isEmpty())args<<"--ignored-fingerprint"<<ignoredFingerprint;
    if(!discardFingerprint.isEmpty())args<<"--discard-fingerprint"<<discardFingerprint;
    if(!processFingerprint.isEmpty())args<<"--process-fingerprint"<<processFingerprint;
    if(preserveArchiveRestore)args<<"--preserve-archive-restore";
    const auto forget=[this,host,commonDir]{invalidateWorktreeCache(host,commonDir);};
    auto process=processRequest(m_hgs,args); process.timeoutMs=330000; process.maximumOutputBytes=65536;
    ProcessRunner::run(std::move(process),this,[](ProcessRunner::Result result){return parsedResult(std::move(result));},
                       [this,request,token,path,commonDir,host,forget](const ParsedResult &parsed){
        const auto &reply=parsed.result;
        forget();
        const auto &data=parsed.reply.object;
        const bool finished=reply.outcome==ProcessRunner::Result::Finished && reply.exitStatus==QProcess::NormalExit;
        const bool identity=data["request_id"]==token && data["path"]==path;
        if(finished && reply.exitCode==0 && identity && (data["status"]=="removed" || data["status"]=="forgotten")) {
            emit worktreeRemoved(request,true,data,{});return;
        }
        const bool verified=finished && reply.exitCode!=0 && identity && data["common_dir"]==commonDir
            && data["operation"]=="worktree_remove" && data["outcome_version"]==1
            && (data["status"]=="kept" || data["status"]=="unconfirmed");
        const auto detail=verified?data["detail"].toString().left(4000):QString::fromLocal8Bit(reply.standardError).trimmed().left(4000);
        emit worktreeRemoved(request,false,verified?data:QJsonObject(),detail.isEmpty()
            ? tr("Removal could not be confirmed. Check the folder before deciding what to do next.") : cleanupError(host,detail));
    });
    return request;
}

quint64 HgsClient::requestCreationDirectory(const QString &host,const QString &path,const QString &expectedMachine)
{
    const auto request=++m_directoryRequest;QStringList help{"--help"};if(!host.isEmpty())help.prepend('@'+host);
    const auto execute=[this,request,host,path,expectedMachine](bool verified){
        if(!verified&&!expectedMachine.isEmpty()){emit directoriesFailed(request,host,tr("The original computer identity cannot be verified by this CLI."));return;}
        QStringList args{"dirs"};if(verified)args<<"--creation";args<<(path.isEmpty()?QStringLiteral("~"):path);
        if(!host.isEmpty())args.prepend('@'+host);guardMachine(args,expectedMachine);
        runHgsParsed(m_hgs,args,host.isEmpty()?10000:20000,this,parseJsonObject,[this,request,host,verified](const QJsonObject &data){
            if(!data["path"].isString()||!data["directories"].isArray()||(verified&&(!data["creation_identity_v1"].toBool()||QUuid(data["computer_id"].toString()).isNull())))
                emit directoriesFailed(request,host,tr("Invalid creation directory identity."));
            else emit directoriesReady(request,host,data);
        },[this,request,host](const QString &,const QString &error){emit directoriesFailed(request,host,error);});
    };
    runHgs(m_hgs,help,15000,this,[execute](const QByteArray &out){execute(out.contains("guarded-create-v1"));},
        [this,request,host](const QString &,const QString &error){emit directoriesFailed(request,host,error);});return request;
}

quint64 HgsClient::requestDirectories(const QString &host, const QString &path, bool hidden)
{
    const quint64 request = ++m_directoryRequest;
    QStringList args{QStringLiteral("dirs")};
    if (hidden) args << QStringLiteral("--hidden");
    args << (path.isEmpty() ? QStringLiteral("~") : path);
    if (!host.isEmpty()) args.prepend(QLatin1Char('@') + host);
    runHgsParsed(m_hgs, args, host.isEmpty() ? 5000 : 15000, this, parseObject,
           [this, host, request](const ObjectReply &doc) {
               if (!doc.ok() || !doc.object.value("path").isString()
                   || !doc.object.value("directories").isArray()) {
                   emit directoriesFailed(request, host, tr("Invalid directory response")); return;
               }
               emit directoriesReady(request, host, doc.object);
           }, [this, host, request](const QString &, const QString &detail) {
               emit directoriesFailed(request, host, detail);
           });
    return request;
}

quint64 HgsClient::createDirectory(const QString &host, const QString &path)
{
    const quint64 request = ++m_directoryRequest;
    const auto token = QUuid::createUuid().toString(QUuid::WithoutBraces);
    const auto style = host.isEmpty() ? MachinePlatform::build() : MachinePlatform::Style::Posix;
    auto requested = path;
    if (style == MachinePlatform::Style::Windows) requested.replace('\\', '/');
    const auto split = requested.lastIndexOf('/');
    const auto parentPath = split < 0 ? QStringLiteral(".") : split == 0 ? QStringLiteral("/") : requested.left(split);
    const auto leaf = requested.mid(split + 1);
    const auto mutate = [this, host, path, request, token, style, leaf](const QString &parent) {
        QStringList args{"dirs", "--create", path, "--request-id", token};
        if (!host.isEmpty()) args.prepend('@' + host);
        const auto expected = MachinePlatform::clean(parent + '/' + leaf, style);
        runHgsParsed(m_hgs, args, host.isEmpty() ? 5000 : 15000, this,
            parseJsonObject,[this, host, path, request, token, style, expected, parent](const QJsonObject &data){
                const auto confirmed = data.value("path").toString();
                if (data["status"] != "created" || data["request_id"] != token || data["requested_path"] != path
                    || !MachinePlatform::absolute(confirmed, style) || !MachinePlatform::same(confirmed, expected, style)
                    || !MachinePlatform::same(data.value("parent").toString(), parent, style)) {
                    emit directoryCreationFailed(request, host,
                        tr("Folder creation could not be confirmed. Open the path to check it before trying again."));
                    return;
                }
                emit directoryCreated(request, host, data);
            }, [this, host, request](const QString &, const QString &error) {
                emit directoryCreationFailed(request, host,
                    tr("%1\nOpen the path to check whether the folder exists before trying again.").arg(error));
            });
    };
    QStringList help{QStringLiteral("--help")};
    if (!host.isEmpty()) help.prepend('@' + host);
    // Probe the selected installed CLI, never fall through to an old agent launch.
    runHgs(m_hgs, help, host.isEmpty() ? 5000 : 15000, this,
        [this, host, request, parentPath, style, mutate](const QByteArray &out) {
            if (!out.contains("dirs --create <path>")) {
                emit directoryCreationFailed(request, host,
                    tr("Update hgs on %1 to create folders. You can still choose existing folders.")
                        .arg(host.isEmpty() ? tr("this computer") : host));
                return;
            }
            QStringList args{"dirs", parentPath};
            if (!host.isEmpty()) args.prepend('@' + host);
            // Pin the canonical parent before mkdir, retaining symlink aliases.
            runHgsParsed(m_hgs, args, host.isEmpty() ? 5000 : 15000, this,
                parseJsonObject,[this, host, request, style, mutate](const QJsonObject &data){
                    const auto parent = data.value("path").toString();
                    if (!MachinePlatform::absolute(parent, style) || !data.value("directories").isArray()) {
                        emit directoryCreationFailed(request, host,
                            tr("The computer did not confirm the parent folder. Open it before creating a folder."));
                        return;
                    }
                    mutate(parent);
                }, [this, host, request](const QString &, const QString &error) {
                    emit directoryCreationFailed(request, host, error);
                });
        }, [this, host, request](const QString &, const QString &error) {
            emit directoryCreationFailed(request, host, error);
        });
    return request;
}

quint64 HgsClient::requestAnswerQuestion(const QString &host, const QString &name,
                                        const QJsonObject &question, const QJsonArray &answers, const QString &durableRequestId)
{
    const quint64 request = ++m_answerRequest;
    const QString key = host + QLatin1Char('\n') + name;
    const QString questionId = question.value("question_id").toString();
    const QString questionHash = question.value("question_hash").toString();
    const QString runId = question.value("run_id").toString();
    const QString conversationId = question.value("conversation_id").toString();
    const bool asyncQuestion = question.value("source") == "codex_async" && question.value("optional").toBool();
    // Native agents ask for folder trust before SessionStart creates a conversation.
    // Only recognized native startup requests may omit it; the CLI checks the
    // exact run, process, folder and complete disclosure before sending keys.
    const auto trustProvider = question.value("source") == "claude_folder_trust" ? QStringLiteral("claude")
        : question.value("source") == "codex_folder_trust" ? QStringLiteral("codex") : QStringLiteral("kimi");
    const bool startupTrust = question.value("source") == trustProvider + "_folder_trust"
        && question.value("answer_transport") == trustProvider + "_tui" && questionHash.size() == 64
        && questionId == trustProvider + "-trust:" + questionHash;
    const bool startupPermission = question.value("source") == "claude_permission_mode"
        && question.value("answer_transport") == "claude_tui" && questionHash.size() == 64
        && questionId == "claude-permissions:" + questionHash;
    const bool startupHooks = question.value("source") == "codex_hooks_trust"
        && question.value("answer_transport") == "codex_tui" && questionHash.size() == 64
        && questionId == "codex-hooks-trust:" + questionHash;
    const bool startupUpdate = question.value("source") == "codex_update"
        && question.value("answer_transport") == "codex_tui" && questionHash.size() == 64
        && questionId == "codex-update:" + questionHash;
    auto reject = [this, request, host, name](const QString &detail) {
        QTimer::singleShot(0, this, [this, request, host, name, detail] {
            emit questionAnswerFailed(request, host, name, detail, false);
        });
    };
    if (m_answersInFlight.contains(key)) {
        reject(tr("An answer to this session is already being sent.")); return request;
    }
    const bool canSkip = question.value("optional").toBool() && question.value("can_skip").toBool()
        && question.value("source")=="codex_async" && !answers.isEmpty()
        && std::all_of(answers.begin(),answers.end(),[](const QJsonValue &value){return value.toObject().value("skip").toBool();});
    if (name.isEmpty() || questionId.isEmpty() || questionHash.isEmpty() || runId.isEmpty()
        || (conversationId.isEmpty() && !startupTrust && !startupPermission && !startupHooks && !startupUpdate) || answers.isEmpty() || (!question.value("can_answer").toBool() && !canSkip)) {
        reject(tr("This request is not ready to answer. Wait for an updated request or open Terminal.")); return request;
    }
    const QString program = resolvedProgram(m_hgs);
    const QFileInfo executable(program);
    if (program.isEmpty() || !executable.isFile() || !executable.isExecutable()) {
        reject(tr("Cannot start hgs: executable not found or not executable: %1").arg(m_hgs)); return request;
    }
    const QString requestId = durableRequestId.isEmpty() ? QUuid::createUuid().toString(QUuid::WithoutBraces) : durableRequestId;
    if (QUuid(requestId).isNull()) { reject(tr("The approval journal request identity is invalid.")); return request; }
    const QByteArray payload = QJsonDocument(QJsonObject{
        {"request_id", requestId}, {"question_id", questionId}, {"expected_question_hash", questionHash},
        {"expected_run_id", runId}, {"expected_conversation_id", conversationId}, {"answers", answers}
    }).toJson(QJsonDocument::Compact);
    if (payload.size() > MaximumMessageBytes) {
        reject(tr("The answers exceed 64 KiB.")); return request;
    }
    QStringList args{QStringLiteral("answer"), name, QStringLiteral("--json")};
    if (!host.isEmpty()) args.prepend(QLatin1Char('@') + host);
    m_answersInFlight.insert(key);
    auto job = processRequest(program, args); job.input = payload; job.timeoutMs = host.isEmpty() ? 30000 : 45000;
    ProcessRunner::run(std::move(job), this, [](ProcessRunner::Result result) { return parsedResult(std::move(result), 65536); },
        [this, request, host, name, key, requestId, questionId, questionHash, runId, conversationId, asyncQuestion, canSkip]
        (const ParsedResult &parsed) {
            const auto &result = parsed.result;
            m_answersInFlight.remove(key);
            const auto fail = [this, request, host, name](const QString &detail, bool uncertain) { emit questionAnswerFailed(request, host, name, detail, uncertain); };
            if (result.outcome == ProcessRunner::Result::FailedToStart) { fail(tr("Cannot start hgs: %1").arg(result.errorString), false); return; }
            if (result.outcome == ProcessRunner::Result::TimedOut) { fail(tr("Answer delivery timed out. Check Terminal before retrying."), true); return; }
            const QString errorText = QString::fromUtf8(result.standardError).trimmed().left(4000);
            if (result.exitCode != 0 || result.exitStatus != QProcess::NormalExit) {
                const bool uncertain = result.exitStatus != QProcess::NormalExit || result.exitCode != 1
                    || errorText.contains(QStringLiteral("delivery uncertain"), Qt::CaseInsensitive);
                fail(errorText.isEmpty() ? tr("Answer transfer failed (exit %1).").arg(result.exitCode) : errorText, uncertain); return;
            }
            if (result.standardOutput.size() > 65536) { fail(tr("The answer acknowledgement is too large. Check Terminal before retrying."), true); return; }
            const auto &receipt = parsed.reply.object;
            if (!parsed.reply.ok()
                || (receipt.value("status") != "answered" && !(asyncQuestion && !canSkip && receipt.value("status") == "submitted" && !receipt.value("skipped").toBool()))
                || receipt.value("request_id").toString() != requestId || receipt.value("name").toString() != name
                || receipt.value("question_id").toString() != questionId || receipt.value("question_hash").toString() != questionHash
                || receipt.value("run_id").toString() != runId || !receipt.value("conversation_id").isString()
                || receipt.value("conversation_id").toString() != conversationId) { fail(tr("The answer acknowledgement is invalid. Check Terminal before retrying."), true); return; }
            if (receipt.value("status") == "submitted") emit questionAnswerSubmitted(request, host, name, receipt);
            else emit questionAnswered(request, host, name, receipt);
            emit sessionWriteDone(host);
        });
    return request;
}

quint64 HgsClient::requestSendMessage(const QString &host, const QString &name, const QString &text,
                                     const QList<MessageAttachment> &attachments,
                                     const QString &expectedRunId, const QString &expectedConversationId, const QString &agentId, const QString &compactionId)
{
    const quint64 request = ++m_messageRequest;
    const QString key = host + QLatin1Char('\n') + name + '\n' + agentId;
    auto reject = [this, request, host, name](const QString &detail) {
        QTimer::singleShot(0, this, [this, request, host, name, detail]() {
            emit messageFailed(request, host, name, detail, false);
        });
    };
    if (m_messagesInFlight.contains(key)) {
        reject(tr("A message to this session is already being sent."));
        return request;
    }
    if (name.isEmpty() || expectedRunId.isEmpty()) {
        reject(tr("Refresh the session before sending a message."));
        return request;
    }
    // On macOS configureProcess starts a shell trampoline before exec'ing hgs.
    // A missing CLI would otherwise look like a delivery-uncertain exit127.
    // Detect definite startup failures before passing any message bytes; an
    // actual executable exiting127 must still remain uncertain.
    const QString program = resolvedProgram(m_hgs);
    const QFileInfo executable(program);
    if (program.isEmpty() || !executable.isFile() || !executable.isExecutable()) {
        reject(tr("Cannot start hgs: executable not found or not executable: %1").arg(m_hgs));
        return request;
    }
    if (text.toUtf8().size() > MaximumMessageBytes || attachments.size() > MaximumAttachments) {
        reject(tr("Use at most 64 KiB of text and 8 attachments."));
        return request;
    }
    qsizetype total = 0;
    QJsonArray files;
    for (const MessageAttachment &attachment : attachments) {
        if (attachment.data.isEmpty() || attachment.data.size() > MaximumAttachmentBytes) {
            reject(tr("Each attachment must be nonempty and at most 10 MiB."));
            return request;
        }
        total += attachment.data.size();
        if (total > MaximumAttachmentsBytes) {
            reject(tr("Attachments exceed 20 MiB in total."));
            return request;
        }
        QJsonObject file{{"name", attachment.name}, {"mime", attachment.mime},
            {"data_base64", QString::fromLatin1(attachment.data.toBase64())}};
        if (!attachment.reference.isEmpty()) file["reference"] = attachment.reference;
        files.append(file);
    }
    if (text.trimmed().isEmpty() && files.isEmpty()) {
        reject(tr("Write a message or attach a file first."));
        return request;
    }
    const QString messageId = QUuid::createUuid().toString(QUuid::WithoutBraces);
    QJsonObject message{
        {"request_id", messageId}, {"text", text}, {"attachments", files},
        {"expected_run_id", expectedRunId}, {"expected_conversation_id", expectedConversationId}
    };
    if (!agentId.isEmpty()) message["agent_id"] = agentId;
    if (!compactionId.isEmpty()) message["expected_compaction_id"] = compactionId;
    const auto payload = QJsonDocument(message).toJson(QJsonDocument::Compact);
    QStringList args{QStringLiteral("send"), name, QStringLiteral("--json")};
    if (!host.isEmpty()) args.prepend(QLatin1Char('@') + host);
    auto *process = new QProcess(this);
    configureProcess(process, program, args);
    auto *timer = new QTimer(process);
    timer->setSingleShot(true);
    auto reported = std::make_shared<bool>(false);
    auto fail = [this, request, host, name, key, reported](const QString &detail, bool uncertain) {
        if (*reported) return;
        *reported = true;
        m_messagesInFlight.remove(key);
        // A failed start can be reported inside start(), before the caller has
        // registered this request. Keep the claim synchronous, the result queued.
        QTimer::singleShot(0, this, [this, request, host, name, detail, uncertain] {
            emit messageFailed(request, host, name, detail, uncertain);
        });
    };
    const auto timeout = [process, fail]() {
        fail(tr("Delivery timed out. The message may have reached the agent; inspect Terminal before retrying."), true);
        process->kill();
    };
    connect(process, &QProcess::started, process, [process, timer, reported, payload, fail, timeout]() {
        if (*reported || timer->remainingTime() <= 0) {
            if (!*reported) timeout(); else process->kill();
            process->closeWriteChannel();
            return;
        }
        // The request and file bytes never appear in argv, a shell command, or a
        // clipboard. closeWriteChannel drains QProcess's pending bytes before EOF.
        if (process->write(payload) != payload.size()) {
            fail(tr("Message transfer failed. Inspect Terminal before retrying."), true);
            process->kill();
        }
        process->closeWriteChannel();
    });
    connect(timer, &QTimer::timeout, process, timeout);
    connect(process, &QProcess::errorOccurred, process,
        [process, timer, fail](QProcess::ProcessError error) {
            if (error == QProcess::FailedToStart) {
                timer->stop();
                fail(tr("Cannot start hgs: %1").arg(process->errorString()), false);
                process->deleteLater();
            }
        });
    connect(process, &QProcess::finished, process,
        [this, process, timer, fail, reported, request, host, name, key, messageId, expectedRunId, expectedConversationId, agentId]
        (int code, QProcess::ExitStatus status) {
            timer->stop();
            if (*reported) { process->deleteLater(); return; }
            const QString detail = QString::fromUtf8(process->readAllStandardError()).trimmed().left(4000);
            if (code != 0 || status != QProcess::NormalExit) {
                const bool uncertain = status != QProcess::NormalExit || code != 1
                    || detail.contains(QStringLiteral("delivery uncertain"), Qt::CaseInsensitive);
                fail(detail.isEmpty() ? tr("Message transfer failed (exit %1).").arg(code) : detail, uncertain);
            } else ProcessRunner::post(this, [bytes = process->readAllStandardOutput()] { return objectReply(bytes); },
                [this, fail, reported, request, host, name, key, messageId, expectedRunId, expectedConversationId, agentId](const ObjectReply &document) {
                const QJsonObject &receipt = document.object;
                if (!document.ok()
                    || receipt.value("status").toString() != QLatin1String("submitted")
                    || receipt.value("request_id").toString() != messageId
                    || receipt.value("agent_id").toString() != agentId
                    || receipt.value("name").toString() != name
                    || receipt.value("run_id").toString() != expectedRunId
                    || !receipt.value("conversation_id").isString()
                    || receipt.value("conversation_id").toString() != expectedConversationId
                    || (expectedConversationId.isEmpty() && !receipt.value("first_message").toBool())) {
                    fail(tr("Delivery acknowledgement is invalid. Inspect Terminal before retrying."), true);
                } else {
                    *reported = true;
                    m_messagesInFlight.remove(key);
                    emit messageSent(request, host, name, receipt);
                    emit sessionWriteDone(host);
                }
            });
            process->deleteLater();
        });
    m_messagesInFlight.insert(key);
    timer->start(host.isEmpty() ? 15000 : 45000);
    // Return the request ID before native startup can reenter the event loop.
    QTimer::singleShot(0, process, [process, timer, reported, timeout] {
        if (*reported) { process->deleteLater(); return; }
        if (timer->remainingTime() <= 0) { timeout(); process->deleteLater(); return; }
        process->start();
    });
    return request;
}

quint64 HgsClient::requestStageFiles(const QString &host, const QString &name, const QStringList &paths,
                                     const QString &run, const QString &conversation, qint64 untrackedCreated)
{
    QJsonArray files; qsizetype total = 0; QString error;
    if (paths.isEmpty() || paths.size() > MaximumAttachments) error = tr("Drop between 1 and 8 files.");
    for (const auto &path : paths) {
        QFile file(path); const QFileInfo info(path);
        if (!error.isEmpty()) break;
        const auto size = LocalFile::size(info);
        if (!LocalFile::readable(info) || size <= 0 || size > MaximumAttachmentBytes || !file.open(QIODevice::ReadOnly)) {
            error = tr("Each file must be readable, nonempty, and at most 10 MiB."); break;
        }
        const auto bytes = file.read(MaximumAttachmentBytes + 1); total += bytes.size();
        if (bytes.isEmpty() || bytes.size() > MaximumAttachmentBytes || total > MaximumAttachmentsBytes) {
            error = tr("Files exceed 20 MiB in total or changed during transfer."); break;
        }
        files.append(QJsonObject{{"name", info.fileName()}, {"mime", QMimeDatabase().mimeTypeForFile(info).name()},
            {"data_base64", QString::fromLatin1(bytes.toBase64())}});
    }
    if (!error.isEmpty()) {
        const auto id = ++m_sessionActionRequest;
        QTimer::singleShot(0, this, [this,id,error] { emit sessionActionFinished(id,false,{},error); }); return id;
    }
    QJsonObject data{{"expected_run_id",run},{"expected_conversation_id",conversation},{"attachments",files}};
    if (run.isEmpty() && conversation.isEmpty() && untrackedCreated >= 0) data["expected_created"] = untrackedCreated;
    return runSessionAction(host,name,"attachment",data);
}

quint64 HgsClient::requestCompactContext(const QString &host, const QString &name, const QString &run, const QString &conversation)
{
    return runSessionAction(host,name,"compact-context",{{"expected_run_id",run},{"expected_conversation_id",conversation}});
}

quint64 HgsClient::requestClearContext(const QString &host, const QString &name, const QString &run, const QString &conversation, bool dismissPrompt)
{
    QJsonObject data{{"expected_run_id",run},{"expected_conversation_id",conversation}};
    if (dismissPrompt) data["dismiss_prompt"] = true; // Older hgs rejects the unknown field.
    return runSessionAction(host,name,"clear-context",data);
}

quint64 HgsClient::requestInterrupt(const QString &host, const QString &name, const QString &run,
                                    const QString &conversation, double turnStarted)
{
    return runSessionAction(host,name,"interrupt",{{"expected_run_id",run},{"expected_conversation_id",conversation},{"expected_turn_started",turnStarted}});
}

quint64 HgsClient::requestRemoveQueued(const QString &host, const QString &name, const QString &run,
                                       const QString &conversation, const QString &messageId)
{
    // The exact native ID as hgs checks it: msg_ and at most 128 ASCII letters, digits, _ and -.
    static const QRegularExpression nativeId(QRegularExpression::anchoredPattern(QStringLiteral("msg_[A-Za-z0-9_-]{1,124}")));
    if (!nativeId.match(messageId).hasMatch()) {
        const auto id = ++m_sessionActionRequest;
        QTimer::singleShot(0, this, [this,id] { emit sessionActionFinished(id,false,{},tr("Choose the exact queued OpenCode message.")); });
        return id;
    }
    return runSessionAction(host,name,"remove_queued",{{"expected_run_id",run},{"expected_conversation_id",conversation},{"message_id",messageId}});
}

quint64 HgsClient::requestForgetBinding(const QString &host, const QString &name, const QString &run, const QString &conversation)
{
    return runSessionAction(host,name,"forget",{{"expected_run_id",run},{"expected_conversation_id",conversation}});
}

quint64 HgsClient::runSessionAction(const QString &host, const QString &name, const QString &operation, QJsonObject data)
{
    const auto request = ++m_sessionActionRequest;
    const auto requestId = QUuid::createUuid().toString(QUuid::WithoutBraces); data["request_id"] = requestId;
    // Scoped native actions go through the receipt-keeping `session-action`
    // command, which names the action in its request.
    const bool scoped = operation == "remove_queued" || operation == "forget";
    if (scoped) data["action"] = operation;
    QStringList args{scoped ? QStringLiteral("session-action") : operation,name}; if (operation == "attachment") args << "--stage";
    args << "--json"; if (!host.isEmpty()) args.prepend('@' + host);
    auto *process = new QProcess(this); configureProcess(process,m_hgs,args);
    auto *timer = new QTimer(process); timer->setSingleShot(true);
    auto reported = std::make_shared<bool>(false);
    // QProcess can fail inside start(), before the caller holds the request ID:
    // a failure is always reported afterwards.
    const auto fail = [this,request,reported](const QString &error) {
        if (*reported) return; *reported = true;
        QTimer::singleShot(0, this, [this,request,error] { emit sessionActionFinished(request,false,{},error); });
    };
    const auto payload = QJsonDocument(data).toJson(QJsonDocument::Compact);
    // OpenCode actions have no Terminal to check.
    const auto transferFailed = scoped ? tr("Transfer failed. Check the session before retrying.") : tr("Transfer failed. Check Terminal before retrying.");
    const auto timedOut = scoped ? tr("Request timed out. Check the session before retrying.") : tr("Request timed out. Check Terminal before retrying.");
    connect(process,&QProcess::started,process,[process,payload,fail,transferFailed] {
        if (process->write(payload) != payload.size()) { fail(transferFailed); process->kill(); }
        process->closeWriteChannel();
    });
    connect(timer,&QTimer::timeout,process,[process,fail,timedOut] { fail(timedOut); process->kill(); });
    connect(process,&QProcess::errorOccurred,process,[process,timer,fail](QProcess::ProcessError error) {
        if (error == QProcess::FailedToStart) { timer->stop(); fail(process->errorString()); process->deleteLater(); }
    });
    connect(process,&QProcess::finished,process,[this,process,timer,fail,reported,request,data,operation,name,host,scoped](int code,QProcess::ExitStatus status) {
        timer->stop();
        if (!*reported) {
            const auto error = QString::fromUtf8(process->readAllStandardError()).trimmed().left(2000);
            if (code || status != QProcess::NormalExit) fail(!error.isEmpty() ? error
                : scoped ? tr("Could not complete the request. Check the session before retrying.") : tr("Could not complete the request. Check Terminal."));
            else ProcessRunner::post(this, [bytes = process->readAllStandardOutput()] { return JsonParse::object(bytes); },
                [this, fail, reported, request, data, operation, name, host, scoped](const QJsonObject &result) {
                const auto state = result.value("status").toString();
                const bool identity = result.value("request_id") == data.value("request_id") && result.value("name") == name
                    && result.value("run_id") == data.value("expected_run_id") && result.value("conversation_id") == data.value("expected_conversation_id");
                if (scoped && identity && (state == "failed" || state == "uncertain")) {
                    // A durable receipt that is not a success is never retried here.
                    const auto reason = result.value("error").toString().trimmed().left(2000);
                    fail(state == "failed" ? (!reason.isEmpty() ? reason : operation == "forget" ? tr("The session was not forgotten.") : tr("The queued message was not removed."))
                        : reason.isEmpty() ? tr("The result was not confirmed. Refresh before trying again.")
                        : tr("The result was not confirmed (%1). Refresh before trying again.").arg(reason));
                } else if (!identity
                    || (data.contains("expected_created") && result.value("created").toInteger(-1) != data.value("expected_created").toInteger())
                    || !(state == (scoped ? "completed" : operation == "attachment" ? "staged" : "submitted")
                        || (operation == "clear-context" && state == "confirmed")))
                    fail(scoped ? tr("Invalid acknowledgement. Check the session before retrying.") : tr("Invalid acknowledgement. Check Terminal before retrying."));
                else { *reported = true; emit sessionActionFinished(request,true,result,{}); if (operation != "attachment") emit sessionWriteDone(host); }
            });
        }
        process->deleteLater();
    });
    timer->start(host.isEmpty() ? 15000 : 45000); process->start(); return request;
}

quint64 HgsClient::requestQueueSendNow(const QString &host, const QString &name, const QString &run, const QString &conversation, const QString &queue)
{
    const auto request = ++m_messageRequest;
    const QString requestId = QUuid::createUuid().toString(QUuid::WithoutBraces);
    const auto payload = QJsonDocument(QJsonObject{{"request_id", requestId}, {"expected_run_id", run},
        {"expected_conversation_id", conversation}, {"queue_id", queue}}).toJson(QJsonDocument::Compact);
    QStringList args{"send-now", name, "--json"}; if (!host.isEmpty()) args.prepend('@' + host);
    auto *process = new QProcess(this); configureProcess(process, m_hgs, args);
    auto *timer = new QTimer(process); timer->setSingleShot(true);
    auto reported = std::make_shared<bool>(false);
    const auto fail = [this, request, reported](const QString &error) {
        if (*reported) return; *reported = true;
        QTimer::singleShot(0, this, [this, request, error] { emit queueSendFinished(request, false, {}, error); });
    };
    const auto timeout = [process, fail] {
        fail(tr("Queue action timed out. Check Terminal before retrying.")); process->kill();
    };
    connect(process, &QProcess::started, process, [process, timer, reported, payload, fail, timeout] {
        if (*reported || timer->remainingTime() <= 0) {
            if (!*reported) timeout(); else process->kill();
            process->closeWriteChannel();
            return;
        }
        if (process->write(payload) != payload.size()) { fail(tr("Queue action failed. Check Terminal before retrying.")); process->kill(); }
        process->closeWriteChannel();
    });
    connect(timer, &QTimer::timeout, process, timeout);
    connect(process, &QProcess::errorOccurred, process, [process, timer, fail](QProcess::ProcessError error) {
        if (error == QProcess::FailedToStart) { timer->stop(); fail(process->errorString()); process->deleteLater(); }
    });
    connect(process, &QProcess::finished, process, [this, process, timer, fail, reported, request, requestId, name, run, conversation, queue, host](int code, QProcess::ExitStatus status) {
        timer->stop();
        if (!*reported) {
            if (code || status != QProcess::NormalExit) {
                const QString error = QString::fromUtf8(process->readAllStandardError()).trimmed().left(2000);
                fail(error.isEmpty() ? tr("Could not confirm sending the queue. Check Terminal before retrying.") : error);
            } else ProcessRunner::post(this, [bytes = process->readAllStandardOutput()] { return objectReply(bytes); },
                [this, fail, reported, request, requestId, name, run, conversation, queue, host](const ObjectReply &doc) {
                const auto &receipt = doc.object;
                if (!doc.ok() || receipt.value("status") != "submitted" || receipt.value("request_id") != requestId
                    || receipt.value("name") != name || receipt.value("run_id") != run || receipt.value("conversation_id") != conversation || receipt.value("queue_id") != queue)
                    fail(tr("Invalid queue acknowledgement. Check Terminal before retrying."));
                else { *reported = true; emit queueSendFinished(request, true, receipt, {}); emit sessionWriteDone(host); }
            });
        }
        process->deleteLater();
    });
    timer->start(host.isEmpty() ? 20000 : 35000);
    // Return the request ID before native startup can reenter the event loop.
    QTimer::singleShot(0, process, [process, timer, reported, timeout] {
        if (*reported) { process->deleteLater(); return; }
        if (timer->remainingTime() <= 0) { timeout(); process->deleteLater(); return; }
        process->start();
    });
    return request;
}

quint64 HgsClient::requestEffort(const QString &host, const QString &name, const QString &effort, const QString &run, const QString &conversation)
{
    const auto request = ++m_effortRequest;
    const QString requestId = QUuid::createUuid().toString(QUuid::WithoutBraces);
    const auto payload = QJsonDocument(QJsonObject{{"request_id", requestId}, {"expected_run_id", run},
        {"expected_conversation_id", conversation}, {"effort", effort}}).toJson(QJsonDocument::Compact);
    QStringList args{"effort", name, "--json"}; if (!host.isEmpty()) args.prepend('@' + host);
    auto *process = new QProcess(this); configureProcess(process, m_hgs, args);
    auto *timer = new QTimer(process); timer->setSingleShot(true);
    auto reported = std::make_shared<bool>(false);
    const auto fail = [this, request, reported](const QString &error) {
        if (*reported) return; *reported = true; emit effortFinished(request, false, {}, error);
    };
    connect(process, &QProcess::started, process, [process, payload, fail] {
        if (process->write(payload) != payload.size()) { fail(tr("Effort transfer failed. Check Terminal before retrying.")); process->kill(); }
        process->closeWriteChannel();
    });
    connect(timer, &QTimer::timeout, process, [process, fail] {
        fail(tr("Effort change timed out. Check Terminal before retrying.")); process->kill();
    });
    connect(process, &QProcess::errorOccurred, process, [process, timer, fail](QProcess::ProcessError error) {
        if (error == QProcess::FailedToStart) { timer->stop(); fail(process->errorString()); process->deleteLater(); }
    });
    connect(process, &QProcess::finished, process, [this, process, timer, fail, reported, request, requestId, name, run, conversation, effort, host](int code, QProcess::ExitStatus status) {
        timer->stop();
        if (!*reported) {
            if (code || status != QProcess::NormalExit) {
                const QString error = QString::fromUtf8(process->readAllStandardError()).trimmed().left(2000);
                fail(error.isEmpty() ? tr("Could not confirm effort. Check Terminal before retrying.") : error);
            } else ProcessRunner::post(this, [bytes = process->readAllStandardOutput()] { return objectReply(bytes); },
                [this, fail, reported, request, requestId, name, run, conversation, effort, host](const ObjectReply &doc) {
                const auto &receipt = doc.object;
                if (!doc.ok() || receipt.value("status") != "confirmed" || receipt.value("request_id") != requestId
                    || receipt.value("name") != name || receipt.value("run_id") != run || receipt.value("conversation_id") != conversation || receipt.value("effort") != effort)
                    fail(tr("Invalid effort acknowledgement. Check Terminal before retrying."));
                else { *reported = true; emit effortFinished(request, true, receipt, {}); emit sessionWriteDone(host); }
            });
        }
        process->deleteLater();
    });
    timer->start(host.isEmpty() ? 20000 : 35000); process->start(); return request;
}

quint64 HgsClient::requestSettings(const QString &host, const QString &name, const QString &model, const QString &effort, const QString &run, const QString &conversation, const QString &expectedPendingId, bool withoutTerminal)
{
    const auto check = [withoutTerminal](const QString &terminal, const QString &session) { return withoutTerminal ? session : terminal; };
    const auto transferFailed = check(tr("Settings transfer failed. Check Terminal before retrying."), tr("Settings transfer failed. Check the session before retrying."));
    const auto timedOut = check(tr("Settings change timed out. Check Terminal before retrying."), tr("Settings change timed out. Check the session before retrying."));
    const auto unconfirmed = check(tr("Could not confirm session settings. Check Terminal before retrying."), tr("Could not confirm session settings. Check the session before retrying."));
    const auto invalid = check(tr("Invalid settings acknowledgement. Check Terminal before retrying."), tr("Invalid settings acknowledgement. Check the session before retrying."));
    const auto request = ++m_settingsRequest;
    const QString requestId = QUuid::createUuid().toString(QUuid::WithoutBraces);
    QJsonObject data{{"request_id", requestId}, {"expected_run_id", run}, {"expected_conversation_id", conversation}};
    if (!model.isEmpty()) data["model"] = model;
    if (!effort.isEmpty()) data["effort"] = effort;
    if (!expectedPendingId.isEmpty()) data["expected_pending_id"] = expectedPendingId;
    const auto payload = QJsonDocument(data).toJson(QJsonDocument::Compact);
    QStringList args{"settings", name, "--json"}; if (!host.isEmpty()) args.prepend('@' + host);
    auto *process = new QProcess(this); configureProcess(process, m_hgs, args);
    auto *timer = new QTimer(process); timer->setSingleShot(true);
    auto reported = std::make_shared<bool>(false);
    const auto fail = [this, request, reported](const QString &error) {
        if (*reported) return; *reported = true;
        QTimer::singleShot(0, this, [this, request, error] { emit settingsFinished(request, false, {}, error); });
    };
    const auto timeout = [process, fail, timedOut] {
        fail(timedOut); process->kill();
    };
    connect(process, &QProcess::started, process, [process, timer, reported, payload, fail, transferFailed, timeout] {
        if (*reported || timer->remainingTime() <= 0) {
            if (!*reported) timeout(); else process->kill();
            process->closeWriteChannel();
            return;
        }
        if (process->write(payload) != payload.size()) { fail(transferFailed); process->kill(); }
        process->closeWriteChannel();
    });
    connect(timer, &QTimer::timeout, process, timeout);
    connect(process, &QProcess::errorOccurred, process, [process, timer, fail](QProcess::ProcessError error) {
        if (error == QProcess::FailedToStart) { timer->stop(); fail(process->errorString()); process->deleteLater(); }
    });
    connect(process, &QProcess::finished, process, [this, process, timer, fail, reported, request, requestId, name, run, conversation, model, effort, host, unconfirmed, invalid](int code, QProcess::ExitStatus status) {
        timer->stop();
        if (!*reported) {
            if (code || status != QProcess::NormalExit) {
                const QString error = QString::fromUtf8(process->readAllStandardError()).trimmed().left(2000);
                fail(error.isEmpty() ? unconfirmed : error);
            } else ProcessRunner::post(this, [bytes = process->readAllStandardOutput()] { return objectReply(bytes); },
                [this, fail, reported, request, requestId, name, run, conversation, model, effort, host, invalid](const ObjectReply &doc) {
                const auto &receipt = doc.object;
                if (!doc.ok() || (receipt.value("status") != "applied" && receipt.value("status") != "scheduled") || receipt.value("request_id") != requestId
                    || receipt.value("name") != name || receipt.value("run_id") != run || receipt.value("conversation_id") != conversation
                    || (!model.isEmpty() && receipt.value("model") != model) || (!effort.isEmpty() && receipt.value("effort") != effort))
                    fail(invalid);
                else { *reported = true; emit settingsFinished(request, true, receipt, {}); emit sessionWriteDone(host); }
            });
        }
        process->deleteLater();
    });
    timer->start(host.isEmpty() ? 20000 : 35000);
    // Return the request ID before native startup can reenter the event loop.
    QTimer::singleShot(0, process, [process, timer, reported, timeout] {
        if (*reported) { process->deleteLater(); return; }
        if (timer->remainingTime() <= 0) { timeout(); process->deleteLater(); return; }
        process->start();
    });
    return request;
}

quint64 HgsClient::requestNotifications(const QString &host, const QString &name, const QJsonObject &payload)
{
    const auto request = ++m_notificationsRequest;
    const auto expected = SessionNotifications::revision(payload.value("expected_revision"));
    if (name.isEmpty() || !SessionNotifications::keys(payload, {"run_id", "conversation_id", "muted", "expected_revision"})
        || !SessionNotifications::identity(payload.value("run_id"), 128)
        || !SessionNotifications::identity(payload.value("conversation_id"), 256) || !payload.value("muted").isBool() || expected < 0) {
        QTimer::singleShot(0, this, [this, request] {
            emit notificationsFinished(request, false, {}, tr("The session notification identity is unavailable. Refresh the session."));
        });
        return request;
    }
    // Probe the configured LOCAL CLI before giving it an unfamiliar command:
    // an old CLI can pass a new peer's listing through, yet interpret an unknown
    // command as an agent launch. The receiving CLI checks the remote marker.
    const auto executable = m_hgs;
    auto capability = processRequest(executable, {"--help"});
    capability.input = QByteArray();
    capability.timeoutMs = 5000;
    capability.maximumOutputBytes = 256 * 1024;
    ProcessRunner::run(std::move(capability), this, [this, request, executable, host, name, payload, expected](const ProcessRunner::Result &help) {
        bool marker = false;
        for (auto line : help.standardOutput.split('\n')) {
            if (line.endsWith('\r')) line.chop(1); // Permit only the CRLF line ending.
            if (line == "session-notifications-v1") marker = true;
        }
        if (help.outcome != ProcessRunner::Result::Finished || help.exitStatus != QProcess::NormalExit || help.exitCode != 0 || !marker) {
            emit notificationsFinished(request, false, {}, tr("Update the configured local hgs to support shared notification preferences."));
            return;
        }
        QStringList args{"notifications", name, "--json"}; if (!host.isEmpty()) args.prepend('@' + host);
        auto job = processRequest(executable, args);
        job.input = QJsonDocument(payload).toJson(QJsonDocument::Compact);
        job.timeoutMs = host.isEmpty() ? 15000 : 35000;
        job.maximumOutputBytes = 512 * 1024; // Includes the bounded 256-child suppression baseline.
        ProcessRunner::run(std::move(job), this, [](ProcessRunner::Result result) { return parsedResult(std::move(result)); },
                           [this, request, payload, expected](const ParsedResult &parsed) {
            const auto &reply = parsed.result;
            const auto result = parsed.reply.object, preferences = result.value("preferences").toObject();
            const auto revision = SessionNotifications::revision(preferences.value("revision"));
            const bool applied = result.value("applied").toBool();
            const bool confirmed = reply.outcome == ProcessRunner::Result::Finished && reply.exitStatus == QProcess::NormalExit
                && reply.exitCode == 0 && parsed.reply.ok()
                && SessionNotifications::keys(result, {"ok", "applied", "run_id", "conversation_id", "preferences"})
                && result.value("ok").isBool() && result.value("ok").toBool() && result.value("applied").isBool()
                && result.value("run_id") == payload.value("run_id") && result.value("conversation_id") == payload.value("conversation_id")
                && SessionNotifications::valid(preferences, payload.value("conversation_id").toString())
                && preferences.value("muted") == payload.value("muted")
                && (applied ? expected < std::numeric_limits<qint64>::max() && revision == expected + 1 : revision == expected);
            QString error;
            if (!confirmed) {
                error = QString::fromUtf8(reply.standardError).trimmed().left(2000);
                if (error.isEmpty()) error = tr("The shared notification change was not confirmed. Refresh the session before trying again.");
            }
            emit notificationsFinished(request, confirmed, confirmed ? result : QJsonObject(), error);
        });
    });
    return request;
}

quint64 HgsClient::requestRead(const QString &host, const QString &name, const QJsonObject &payload)
{
    const auto request = ++m_readRequest;
    QStringList args{"read", name, "--json"}; if (!host.isEmpty()) args.prepend('@' + host);
    const auto input = QJsonDocument(payload).toJson(QJsonDocument::Compact);
    auto *process = new QProcess(this); configureProcess(process, m_hgs, args);
    auto *timer = new QTimer(process); timer->setSingleShot(true);
    auto reported = std::make_shared<bool>(false);
    const auto fail = [this, request, reported](const QString &error) {
        if (*reported) return; *reported = true; emit readFinished(request, false, {}, error);
    };
    connect(process, &QProcess::started, process, [process, input, fail] {
        if (process->write(input) != input.size()) { fail(tr("Read state transfer failed.")); process->kill(); }
        process->closeWriteChannel();
    });
    connect(timer, &QTimer::timeout, process, [process, fail] { fail(tr("Saving read state timed out.")); process->kill(); });
    connect(process, &QProcess::errorOccurred, process, [process, timer, fail](QProcess::ProcessError error) {
        if (error == QProcess::FailedToStart) { timer->stop(); fail(process->errorString()); process->deleteLater(); }
    });
    connect(process, &QProcess::finished, process, [this, process, timer, fail, reported, request, payload](int code, QProcess::ExitStatus status) {
        timer->stop();
        if (!*reported) ProcessRunner::post(this,
            [bytes = process->readAllStandardOutput(), errorText = QString::fromUtf8(process->readAllStandardError()).trimmed().left(2000)] {
                ObjectReply reply = objectReply(bytes); return std::make_pair(reply, errorText);
            },
            [fail, reported, request, payload, code, status, this](const std::pair<ObjectReply, QString> &prepared) {
            const auto &result = prepared.first.object;
            const auto applied = result.value("applied").toObject();
            const bool notFound = code == 1 && result == QJsonObject{{"ok", false}, {"error", "not_found"}};
            const bool answered = code == 0 && result.value("ok") == true && applied.value("reply_id").isBool()
                && applied.value("attention_signature").isBool()
                && (result.value("read").isNull()
                    || result.value("read").toObject().value("conversation_id") == payload.value("conversation_id"));
            if (status == QProcess::NormalExit && prepared.first.ok() && (notFound || answered)) {
                *reported = true; emit readFinished(request, true, result, {});
            } else {
                const auto &error = prepared.second;
                fail(error.isEmpty() ? tr("Could not save read state on that machine.") : error);
            }
        });
        process->deleteLater();
    });
    timer->start(host.isEmpty() ? 15000 : 35000); process->start(); return request;
}

quint64 HgsClient::repositoryOperation(const QString &host,const QString &action,const QString &url,
    const QString &parent,const QString &name,const QString &token,const QString &expectedMachine)
{
    const auto request=++m_directoryRequest;
    QStringList help{"--help"};if(!host.isEmpty())help.prepend('@'+host);
    guardMachine(help,expectedMachine);
    runHgs(m_hgs,help,15000,this,[this,request,host,action,url,parent,name,token,expectedMachine](const QByteArray &help){
        if(!help.contains("repository-clone-v1")){
            emit repositoryFinished(request,false,{},tr("Update hgs on %1 to clone repositories.").arg(host.isEmpty()?tr("this computer"):host));return;
        }
        QStringList args{"repository",action,"--json"};
        if(action!="status")args<<"--url"<<url<<"--parent"<<parent<<"--name"<<name;
        if(action!="check")args<<"--request-id"<<token;
        if(!host.isEmpty())args.prepend('@'+host);
        guardMachine(args,expectedMachine);
        auto process=processRequest(m_hgs,args);process.input=QByteArray();process.maximumOutputBytes=65536;
        process.timeoutMs=action=="clone"?650000:30000;
        ProcessRunner::run(std::move(process),this,[](ProcessRunner::Result result){return parsedResult(std::move(result));},
                           [this,request,action,name,token,host](const ParsedResult &parsed){
            const auto &result=parsed.result;
            if(result.outcome!=ProcessRunner::Result::Finished||result.exitStatus!=QProcess::NormalExit||result.exitCode!=0){
                auto error=QString::fromUtf8(result.standardError).trimmed().left(4000);
                if(error.isEmpty())error=tr("The computer did not confirm the repository operation. Check its result before trying again.");
                emit repositoryFinished(request,false,{},error);return;
            }
            const auto &data=parsed.reply.object;
            const auto style=host.isEmpty()&&MachinePlatform::windowsShaped(data["parent"].toString())?MachinePlatform::Style::Windows:MachinePlatform::Style::Posix;
            const auto status=data["status"].toString();
            const auto expected=MachinePlatform::clean(data["parent"].toString()+'/'+name,style);
            const bool receipt=action=="check"?status=="ready"&&data["name"]==name
                &&MachinePlatform::same(data["path"].toString(),expected,style):
                data["request_id"]==token&&(status=="cloned"||status=="failed"||status=="unconfirmed");
            if(!receipt||!MachinePlatform::absolute(data["parent"].toString(),style)
                ||!MachinePlatform::absolute(data["path"].toString(),style)){
                emit repositoryFinished(request,false,{},tr("Invalid repository response. Check the destination before trying again."));return;
            }
            emit repositoryFinished(request,true,data,{});
        });
    },[this,request](const QString &,const QString &error){emit repositoryFinished(request,false,{},error);});
    return request;
}
