import Foundation
import ZerusReplica

/// The stage tabs of the Tasks screen, in the order of the desktop's tiles.
public enum TaskTab: CaseIterable, Hashable, Sendable {
    case review, active, todo, backlog, done

    public var label: String {
        switch self {
        case .review: "In review"
        case .active: "In progress"
        case .todo: "To do"
        case .backlog: "Backlog"
        case .done: "Done"
        }
    }
}

/// The stage actions, the same list as on the desktop row and in the task settings.
public enum TaskAction: Hashable, Sendable {
    case edit, toBacklog, toTop, send, toTodo, cancel, backToTodo, accept

    public var label: String {
        switch self {
        case .edit: "Edit"
        case .toBacklog: "Move to Backlog"
        case .toTop: "Move to top"
        case .send: "Send now"
        case .toTodo: "Move to To do"
        case .cancel: "Cancel"
        case .backToTodo: "Back to To do"
        case .accept: "Accept"
        }
    }
}

/// `main`: the stage's main action, the last one.
public struct ActionState: Hashable, Sendable {
    public let action: TaskAction
    public let enabled: Bool
    public let reason: String
    public let main: Bool
    public init(_ action: TaskAction, enabled: Bool, reason: String = "", main: Bool = false) {
        self.action = action
        self.enabled = enabled
        self.reason = reason
        self.main = main
    }
}

/// One card: `note` is the agent's report or the cancellation, In review only; `queue` is the
/// task's place in the project's To do, 0 elsewhere; `filedBy` names the agent that filed a To do
/// or Backlog task.
public struct TaskCard: Hashable, Sendable {
    public let uuid: String
    public let id: String
    public let text: String
    public let tab: TaskTab
    public let sessionLabel: String
    public let highlighted: Bool
    public let note: String
    public let actions: [ActionState]
    public let anySession: Bool
    public let queue: Int
    public let filedBy: String
    public let stageAt: Int64
    public init(uuid: String, id: String, text: String, tab: TaskTab, sessionLabel: String, highlighted: Bool, note: String,
                actions: [ActionState], anySession: Bool = false, queue: Int = 0, filedBy: String = "", stageAt: Int64 = 0) {
        self.uuid = uuid
        self.id = id
        self.text = text
        self.tab = tab
        self.sessionLabel = sessionLabel
        self.highlighted = highlighted
        self.note = note
        self.anySession = anySession
        self.queue = queue
        self.filedBy = filedBy
        self.stageAt = stageAt
        self.actions = actions
    }
}

/// The session Tasks is for: it receives new tasks and Any session tasks are sent to it.
public struct ChosenSession: Hashable, Sendable {
    public let name: String
    public let computerId: String
    public let machineId: String
    public init(name: String, computerId: String, machineId: String) {
        self.name = name
        self.computerId = computerId
        self.machineId = machineId
    }
}

public enum ChipDot: Hashable, Sendable { case working, attention, ready }

public struct SessionChip: Hashable, Sendable {
    public let session: ChosenSession
    public let dot: ChipDot
}

/// The computer a Send now or Cancel goes to, and the session it names; `direct` when that is the session's own computer.
public struct SendRoute: Hashable, Sendable {
    public let computerId: String
    public let session: String
    public let direct: Bool
    public init(computerId: String, session: String, direct: Bool = true) {
        self.computerId = computerId
        self.session = session
        self.direct = direct
    }
}

public enum RouteState: Hashable, Sendable {
    case ready(SendRoute)
    case blocked(String)
}

/// What the phone's Tasks screen shows, from the replica's listing (docs/superpowers/specs/2026-10-13-project-task-queue-design.md,
/// "Android interface"), ported from Android `TasksPresentation.kt`.
public enum TasksPresentation {
    public static func tab(_ stage: TaskStage) -> TaskTab {
        switch stage {
        case .review: .review
        case .active: .active
        case .todo: .todo
        case .backlog: .backlog
        case .done: .done
        }
    }

    public static func actions(_ tab: TaskTab) -> [TaskAction] {
        switch tab {
        case .todo: [.edit, .toBacklog, .toTop, .send]
        case .backlog: [.edit, .toTodo, .send]
        case .active: [.cancel]
        case .review: [.backToTodo, .accept]
        case .done: [.backToTodo]
        }
    }

    private static func same(_ ref: SessionRef?, _ chosen: ChosenSession?) -> Bool {
        guard let ref, let chosen else { return false }
        return ref.machine == chosen.machineId && ref.session == chosen.name
    }

    private static func pinned(_ task: TaskInfo) -> SessionRef? {
        if case .session(let session) = task.target { session } else { nil }
    }

    /// Pinned to the chosen session in To do and Backlog; sent to it in the other stages.
    public static func highlighted(_ task: TaskInfo, chosen: ChosenSession?) -> Bool {
        switch task.stage {
        case .todo, .backlog: same(pinned(task), chosen)
        default: same(task.session, chosen)
        }
    }

    /// Only this session: its own tasks, and the Any session tasks it can take.
    public static func visible(_ task: TaskInfo, chosen: ChosenSession?, onlyThisSession: Bool) -> Bool {
        !onlyThisSession || highlighted(task, chosen: chosen) || (task.target == .anySession && [.todo, .backlog].contains(task.stage))
    }

    /// The counts of the replica's stages; Done counts come from a computer's list.
    public static func counts(_ tasks: [TaskInfo], chosen: ChosenSession?, onlyThisSession: Bool) -> [TaskTab: Int] {
        Dictionary(uniqueKeysWithValues: [TaskTab.review, .active, .todo, .backlog].map { tab in
            (tab, tasks.count { self.tab($0.stage) == tab && visible($0, chosen: chosen, onlyThisSession: onlyThisSession) })
        })
    }

    public static func defaultTab(_ counts: [TaskTab: Int]) -> TaskTab { (counts[.review] ?? 0) > 0 ? .review : .todo }

    /// How long a task has been in its stage, as the desktop row says it.
    public static func age(_ ms: Int64) -> String {
        let minutes = max(0, ms / 60_000)
        if minutes < 60 { return "\(minutes) min" }
        if minutes < 24 * 60 { return "\(minutes / 60) h" }
        return "\(minutes / (24 * 60)) d"
    }

    /// A session as the session list names it: its name after the agent and the project.
    public static func shortName(_ session: String) -> String {
        let parts = session.split(separator: "/", omittingEmptySubsequences: false).map(String.init)
        let tag = parts.dropFirst(2).joined(separator: "/")
        if !tag.isEmpty { return tag }
        return parts.count > 1 && !parts[1].isEmpty ? parts[1] : session
    }

    private static func anySession(_ task: TaskInfo) -> Bool {
        if case .anySession = task.target { return [.todo, .backlog].contains(task.stage) }
        return false
    }

    /// The session a task is pinned to or ran in; an Any session task names the chosen session it goes to now.
    public static func sessionLabel(_ task: TaskInfo, chosen: ChosenSession? = nil) -> String {
        let target = pinned(task)?.session
        let name: String? = switch task.stage {
        case .todo, .backlog: target
        default: task.session?.session ?? target
        }
        guard let name else {
            if let chosen, anySession(task) { return "any · now \(shortName(chosen.name))" }
            return "any session"
        }
        return shortName(name)
    }

    public static func note(_ task: TaskInfo) -> String {
        guard task.stage == .review else { return "" }
        if task.cancelledAt != nil { return task.cancelReason.isEmpty ? "Cancelled" : "Cancelled: \(task.cancelReason)" }
        return task.summary
    }

    public static func cards(_ tasks: [TaskInfo], tab: TaskTab, chosen: ChosenSession?, onlyThisSession: Bool,
                             send: (TaskInfo) -> RouteState, cancel: (TaskInfo) -> RouteState) -> [TaskCard] {
        // To do keeps its project-wide numbers whatever the filter hides.
        let queue = tasks.filter { $0.stage == .todo }.map(\.uuid)
        return tasks.filter { self.tab($0.stage) == tab && visible($0, chosen: chosen, onlyThisSession: onlyThisSession) }.map { task in
            let stageActions = actions(tab)
            let states = stageActions.map { action -> ActionState in
                let main = action == stageActions.last
                let route: RouteState? = switch action {
                case .send: send(task)
                case .cancel: cancel(task)
                default: nil
                }
                if !task.writable { return ActionState(action, enabled: false, reason: "Update Zerus to change this task", main: main) }
                if case .blocked(let reason) = route { return ActionState(action, enabled: false, reason: reason, main: main) }
                return ActionState(action, enabled: true, main: main)
            }
            var filer = ""
            if case .agent(let session) = task.author, [.todo, .backlog].contains(task.stage) { filer = shortName(session.session) }
            return TaskCard(uuid: task.uuid, id: task.id, text: task.text, tab: tab, sessionLabel: sessionLabel(task, chosen: chosen),
                            highlighted: highlighted(task, chosen: chosen), note: note(task), actions: states, anySession: anySession(task),
                            queue: task.stage == .todo ? (queue.firstIndex(of: task.uuid) ?? -1) + 1 : 0, filedBy: filer, stageAt: task.stageAt)
        }
    }

    /// The computer of `ref` among this connection's machines.
    private static func computer(_ ref: SessionRef, _ machines: [Machine], _ connectionId: String) -> Machine? {
        machines.first { $0.connectionId == connectionId && !$0.machineId.isEmpty && $0.machineId == ref.machine }
    }

    /// A session takes tasks once its computer reports its native ID, which tasks pin.
    private static func outdated(_ session: String) -> String { "\(session) is on a computer that needs an update to take tasks" }

    /// Why the input cannot add a task for the chosen session, or "".
    public static func addReason(_ chosen: ChosenSession?) -> String {
        guard let chosen else { return "Choose a session for new tasks" }
        return chosen.machineId.isEmpty ? outdated(chosen.name) : ""
    }

    private static func reachable(_ machine: Machine) -> Bool { machine.online && !machine.lastKnown }

    /// Where Send now goes, or why it cannot: the pinned session's computer, or the chosen session for an Any session task.
    public static func route(_ task: TaskInfo, chosen: ChosenSession?, machines: [Machine], connectionId: String,
                             operations: (Machine) -> Set<String>, sessions: [Session]) -> RouteState {
        guard let ref = pinned(task) ?? chosen.map({ SessionRef(machine: $0.machineId, session: $0.name) }) else {
            return .blocked("Choose a session for this task")
        }
        if ref.machine.isEmpty { return .blocked(outdated(ref.session)) }
        guard let machine = computer(ref, machines, connectionId) else {
            return .blocked("\(ref.session) is on a computer this phone does not reach")
        }
        if !reachable(machine) { return .blocked("\(machine.name) is offline") }
        if !operations(machine).contains("task_send") { return .blocked("\(machine.name) needs an update to send tasks") }
        if !sessions.contains(where: { $0.target.computerId == machine.id && $0.target.session == ref.session && $0.target.archiveId.isEmpty }) {
            return .blocked("\(ref.session) no longer exists — choose another session")
        }
        return .ready(SendRoute(computerId: machine.id, session: ref.session))
    }

    /// Cancel goes to the session's computer, or, while it is out of reach, to any online computer of the project, which cancels it there as hgs does.
    public static func cancelRoute(_ task: TaskInfo, machines: [Machine], operations: (Machine) -> Set<String>) -> RouteState {
        guard let ref = task.session else { return .blocked("This task was not sent to a session") }
        let able = machines.filter { reachable($0) && !$0.machineId.isEmpty && operations($0).contains("task_cancel") }
        guard let machine = able.first(where: { $0.machineId == ref.machine }) ?? able.first else {
            return .blocked("No computer of this project is online")
        }
        return .ready(SendRoute(computerId: machine.id, session: ref.session, direct: machine.machineId == ref.machine))
    }

    /// One chip per live session of the project, with its status.
    public static func chips(_ sessions: [Session], machines: [Machine]) -> [SessionChip] {
        sessions.filter { $0.target.archiveId.isEmpty && !SessionFilters.archived($0) }.map { session in
            let machine = machines.first { $0.connectionId == session.target.connectionId && $0.id == session.target.computerId }
            let online = SessionFilters.online(session, machines)
            let dot: ChipDot = SessionFilters.needsAction(session) ? .attention : SessionFilters.working(session, online) ? .working : .ready
            return SessionChip(session: ChosenSession(name: session.target.session, computerId: session.target.computerId,
                                                      machineId: machine?.machineId ?? ""), dot: dot)
        }
    }

    /// "Show older (N)": the older Done tasks, while some are not shown.
    public static func olderLabel(total: Int, recent: Int, shown: Int, expanded: Bool, more: Bool) -> String? {
        let older = expanded ? total - shown : total - recent
        return older > 0 && (more || !expanded) ? "Show older (\(older))" : nil
    }
}
