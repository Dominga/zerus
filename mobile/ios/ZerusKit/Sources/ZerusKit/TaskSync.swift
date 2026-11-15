import CryptoKit
import Foundation
import ZerusReplica

/// The phone's task replicas (docs/superpowers/specs/2026-10-18-task-replication-loro-design.md,
/// "Phone"), ported from Android `TaskSync.kt`: one folder per relay connection and swarm below
/// `root`, so two relays never share documents. The documents are not encrypted further; `root` is
/// excluded from backup and protected until first unlock.
public final class TaskReplicas: Sendable {
    public let root: URL
    /// Store calls may wait seconds for the project's file lock: they run here, never on the
    /// main actor or the cooperative pool.
    private let queue = DispatchQueue(label: "app.zerus.task-replicas", qos: .utility, attributes: .concurrent)

    public init(root: URL) throws {
        self.root = root
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        #if os(iOS)
        try FileManager.default.setAttributes([.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication], ofItemAtPath: root.path)
        #endif
        #if !os(Linux)
        var excluded = URLResourceValues()
        excluded.isExcludedFromBackup = true
        var folder = root
        try folder.setResourceValues(excluded)
        #endif
    }

    /// `Application Support/task-replicas-v1`.
    public static func live() throws -> TaskReplicas {
        let support = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
        return try TaskReplicas(root: support.appendingPathComponent("task-replicas-v1", isDirectory: true))
    }

    public func run<T: Sendable>(_ body: @escaping @Sendable () throws -> T) async throws -> T {
        try await withCheckedThrowingContinuation { continuation in
            queue.async { continuation.resume(with: Result { try body() }) }
        }
    }

    /// The project's tasks; each call opens the project on its own.
    public func store(_ key: ProjectKey, name: String) -> TaskStore {
        TaskStore(root: folder(key).path, project: key.id, projectName: name)
    }

    public func folder(_ key: ProjectKey) -> URL {
        root.appendingPathComponent(Self.hex(key.connectionId), isDirectory: true).appendingPathComponent(Self.hex(key.swarmId), isDirectory: true)
    }

    /// Drops every replica the connection holds.
    public func purge(connectionId: String) async throws {
        let folder = root.appendingPathComponent(Self.hex(connectionId), isDirectory: true)
        try await run {
            guard FileManager.default.fileExists(atPath: folder.path) else { return }
            try FileManager.default.removeItem(at: folder)
        }
    }

    static func hex(_ value: String) -> String { SHA256.hash(data: Data(value.utf8)).map { String(format: "%02x", $0) }.joined() }
}

/// How a Send now or Cancel ended on the session's computer.
public enum TaskOutcome: Equatable, Sendable {
    case done
    /// Nothing happened: hgs's reason, or why the phone's edits could not reach that computer.
    case refused(String)
    /// It may have happened: the next exchange shows the task's stage.
    case uncertain(String)
}

/// Where the next Done page starts.
public struct DoneCursor: Equatable, Sendable {
    public let at: Int64
    public let uuid: String
    public init(at: Int64, uuid: String) {
        self.at = at
        self.uuid = uuid
    }
}

public struct DoneRow: Equatable, Sendable {
    public let uuid: String
    public let id: String
    public let text: String
    public let session: SessionRef?
    public let acceptedAt: Int64
}

/// One page of a computer's Done list, newest first; `recent` counts the last 7 days.
public struct DonePage: Equatable, Sendable {
    public let rows: [DoneRow]
    public let next: DoneCursor?
    public let total: Int
    public let recent: Int
    public let damaged: Int
}

/// Retains only required keys across pages, never a growing per-page refusal journal.
struct TaskExchangeEvidence: Sendable {
    private let required: Set<String>
    private var blocked: [String: String] = [:]
    private(set) var digest = ""
    private(set) var warning: String?
    private let mediaUnavailable: String
    init(tasks: [String], mediaUnavailable: String) { required = Set(tasks).union(["project"]); self.mediaUnavailable = mediaUnavailable }
    mutating func accept(_ progress: ExchangeProgress) {
        digest = progress.digest
        for refusal in progress.refusedHere + progress.refusedThere { problem(refusal.uuid, refusal.error) }
        for document in progress.unsent { problem(document, mediaUnavailable) }
    }
    private mutating func problem(_ document: String, _ reason: String) {
        let message = "\(document.prefix(128)): \(reason.prefix(512))"
        if warning == nil { warning = message }
        if required.contains(document), blocked[document] == nil { blocked[document] = message }
    }
    func clean(_ task: String) -> Bool { blocked["project"] == nil && blocked[task] == nil }
    func requireClean(_ tasks: [String]) throws {
        if let reason = blocked["project"] { throw TaskSyncError(reason) }
        for task in tasks { if let reason = blocked[task] { throw TaskSyncError(reason) } }
    }
}

/// A computer of a project that can exchange its tasks now, and the digest its heartbeat names.
public struct TaskPeer: Hashable, Sendable {
    public let computerId: String
    public let digest: String
    public init(computerId: String, digest: String) {
        self.computerId = computerId
        self.digest = digest
    }
}

public enum TaskSyncPlan {
    public static let operation = "task_exchange"
    public static let identityFeature = "task_session_identity_v2"
    public static let uploadFeature = "task_media_upload_v1"

    /// Each catalogued project's computers that are online, not last known, and offer the exchange, as their relay does.
    public static func peers(_ digests: [ProjectKey: [String: String]], relayOperations: [String: Set<String>],
                             machineOperations: [MachineKey: Set<String>], machines: [Machine]) -> [ProjectKey: [TaskPeer]] {
        var plan: [ProjectKey: [TaskPeer]] = [:]
        for (key, byComputer) in digests where relayOperations[key.connectionId]?.contains(operation) == true {
            let peers = byComputer.filter { computer, _ in
                machineOperations[MachineKey(connectionId: key.connectionId, computerId: computer)]?.contains(operation) == true &&
                    machines.contains { $0.connectionId == key.connectionId && $0.id == computer && $0.online && !$0.lastKnown }
            }.map { TaskPeer(computerId: $0.key, digest: $0.value) }.sorted { $0.computerId < $1.computerId }
            if !peers.isEmpty { plan[key] = peers }
        }
        return plan
    }
}

/// Why a task exchange or action did not finish.
public struct TaskSyncError: Error, Equatable, CustomStringConvertible {
    public let message: String
    public init(_ message: String) { self.message = message }
    public var description: String { message }

    /// The reason a person reads: the relay's or hgs's message, or the replica's.
    public static func reason(_ error: Error) -> String {
        switch error {
        case let error as TaskSyncError: error.message
        case let error as RelayError: error.message
        case let error as ZerusError: error.message
        // UniFFI's own description names the case; the person reads the replica's message.
        case let error as ReplicaError:
            switch error {
            case .Refused(let message), .Failed(let message): message
            }
        case let error as LocalizedError: error.errorDescription ?? "\(error)"
        default: "\(error)"
        }
    }
}

/// Exchanges a project's tasks with its computers through the relay: one project at a time
/// (a lock per project), each computer whose digest is not the phone's, until their copies
/// meet. The Rust exchange decides every request; this only carries its JSON.
public actor TaskSync {
    public static let retry: TimeInterval = 60
    public static let exchangePolls = 80
    public static let actionPolls = 150
    public static let pollDelay: Duration = .milliseconds(400)
    static let scopedTasks = 64

    private struct PeerKey: Hashable { let project: ProjectKey; let computerId: String }
    /// After an exchange: the phone's digest, the heartbeat's it answered, and the computer's after it.
    private struct Quiet { let mine: String; let heartbeat: String; let theirs: String }

    let client: RelayClient
    let replicas: TaskReplicas
    private let exchangeTimeout: Duration
    private let clock: @Sendable () -> Date
    private let mediaUnavailable: String
    private let heldSend: String
    private let capabilities: @MainActor @Sendable () -> [MachineKey: Set<String>]
    private var waiting: [PeerKey: Date] = [:]
    private var quiet: [PeerKey: Quiet] = [:]
    private var locked: Set<ProjectKey> = []
    private var queued: [ProjectKey: [CheckedContinuation<Void, Never>]] = [:]
    /// Tasks whose Send now may have happened, with their project and computer.
    private struct PendingSend: Equatable { let peer: PeerKey; let requestId: String }
    private var sends: [String: PendingSend] = [:]
    private var settleAfter: [String: String] = [:]
    private var known: Set<ProjectKey> = []

    public init(client: RelayClient, replicas: TaskReplicas, exchangeTimeout: Duration = .seconds(300),
                mediaUnavailable: String = "Update Zerus on this computer to transfer this task's media.",
                heldSend: String = "It may have been sent already; its stage arrives with the next sync",
                capabilities: @escaping @MainActor @Sendable () -> [MachineKey: Set<String>] = { [:] },
                clock: @escaping @Sendable () -> Date = Date.init) {
        self.client = client
        self.replicas = replicas
        self.exchangeTimeout = exchangeTimeout
        self.clock = clock
        self.mediaUnavailable = mediaUnavailable
        self.heldSend = heldSend
        self.capabilities = capabilities
    }

    /// Exchanges `key`'s tasks with each of `peers` whose digest is not the phone's; the failures by computer ID.
    public func sync(_ connection: Connection, _ key: ProjectKey, name: String, peers: [TaskPeer]) async -> [String: String] {
        await lock(key)
        defer { unlock(key) }
        var failures: [String: String] = [:]
        let store = replicas.store(key, name: name)
        for peer in peers.sorted(by: { $0.computerId < $1.computerId }) {
            if Task.isCancelled { break }
            let wait = PeerKey(project: key, computerId: peer.computerId)
            if clock() < waiting[wait, default: .distantPast] { continue }
            do {
                let mine = try await replicas.run { try store.liveDigest() }
                if mine == peer.digest { continue }
                if let known = quiet[wait], known.mine == mine, [known.heartbeat, known.theirs].contains(peer.digest) { continue }
                let evidence = try await bounded { try await self.exchange(connection, store, computerId: peer.computerId) }
                quiet[wait] = Quiet(mine: try await replicas.run { try store.liveDigest() }, heartbeat: peer.digest, theirs: evidence.digest)
                if let warning = evidence.warning { failures[peer.computerId] = warning }
                waiting[wait] = nil
            } catch {
                if Task.isCancelled { break }
                waiting[wait] = clock().addingTimeInterval(Self.retry)
                let reason = TaskSyncError.reason(error)
                failures[peer.computerId] = reason.isEmpty ? "The tasks could not be exchanged." : reason
            }
        }
        return failures
    }

    /// Drops the connection's replicas once its running exchanges end, so none writes them again.
    public func purge(connectionId: String) async {
        let keys = known.filter { $0.connectionId == connectionId }.sorted { $0.key < $1.key }
        for key in keys { await lock(key) }
        defer { keys.forEach(unlock) }
        try? await replicas.purge(connectionId: connectionId)
        settleAfter[connectionId] = nil
    }

    // MARK: The project's lock

    func lock(_ key: ProjectKey) async {
        known.insert(key)
        guard locked.contains(key) else { locked.insert(key); return }
        await withCheckedContinuation { queued[key, default: []].append($0) }
    }

    func unlock(_ key: ProjectKey) {
        if var waiters = queued[key], !waiters.isEmpty {
            let next = waiters.removeFirst()
            queued[key] = waiters
            next.resume()
        } else {
            locked.remove(key)
        }
    }

    /// Runs `body` under the project's lock with its store.
    func project<T: Sendable>(_ key: ProjectKey, name: String, _ body: (TaskStore) async throws -> T) async rethrows -> T {
        await lock(key)
        defer { unlock(key) }
        return try await body(replicas.store(key, name: name))
    }

    // MARK: Actions

    /// One exchange with this computer now, whatever the digests: the phone's edits reach it, and `fetch` brings tasks it does not hold.
    public func exchangeNow(_ connection: Connection, _ key: ProjectKey, name: String, computerId: String, fetch: [String] = []) async throws {
        guard connection.id == key.connectionId, fetch.count <= Self.scopedTasks else { throw TaskSyncError("The original task exchange scope is invalid.") }
        try await project(key, name: name) { store in
            let evidence = try await bounded { try await self.exchange(connection, store, computerId: computerId, fetch: fetch, required: fetch) }
            try evidence.requireClean(fetch)
        }
    }

    /// Send now: the phone's edits reach the session's computer first, then it sends, then its copy comes back.
    public func send(_ connection: Connection, _ key: ProjectKey, name: String, computerId: String, task: String,
                     session: String, computerName: String? = nil, sessionId: String = "") async -> TaskOutcome {
        let identity = await identitySchema(connection, computerId: computerId, sessionId: sessionId)
        var payload: JSONObject = ["schema": .int(identity ? 2 : 1), "project": .string(key.id), "task": .string(task), "session": .string(session)]
        if identity { payload = payload.putting("session_id", .string(sessionId)) }
        return await act(connection, key, name: name, computerId: computerId, computerName: computerName ?? computerId,
                         operation: "task_send", payload: payload)
    }

    private func identitySchema(_ connection: Connection, computerId: String, sessionId: String) async -> Bool {
        guard connection.protocolVersion == 2, !sessionId.isEmpty else { return false }
        let advertised = await capabilities()
        return advertised[MachineKey(connectionId: connection.id, computerId: computerId)]?.contains(TaskSyncPlan.identityFeature) == true
    }

    /// A Send now that may have happened holds the task until an exchange with that computer shows its stage.
    public func held(_ task: String) -> Bool { sends[task] != nil }
    public func heldTasks() -> Set<String> { Set(sends.keys) }

    /// Exchanges once with each computer of this connection that holds a task back; one that completes releases them.
    public func settleSends(_ connection: Connection, names: @Sendable (ProjectKey) -> String) async {
        // The active in-memory hold map already exists. Keep at most64 selected
        // keys, in stable key order; never load task files or an entire journal.
        func page(after: String?) -> [(String, PendingSend)] {
            var page: [(String, PendingSend)] = []
            for (task, peer) in sends where peer.peer.project.connectionId == connection.id && (after == nil || task > after!) {
                page.append((task, peer)); page.sort { $0.0 < $1.0 }
                if page.count > Self.scopedTasks { page.removeLast() }
            }
            return page
        }
        var pending = page(after: settleAfter[connection.id])
        if pending.isEmpty { pending = page(after: nil) }
        if let last = pending.last { settleAfter[connection.id] = last.0 }
        for (original, tasks) in Dictionary(grouping: pending, by: { $0.1.peer }) {
            do {
                let ids = tasks.map(\.0)
                let evidence = try await project(original.project, name: names(original.project)) { store in
                    try await bounded { try await self.exchange(connection, store, computerId: original.computerId, fetch: ids, required: ids) }
                }
                for (task, peer) in tasks where evidence.clean(task) && sends[task] == peer { sends[task] = nil }
            } catch {
                if Task.isCancelled { return }
                // The next sync tries again.
            }
        }
    }

    public func cancel(_ connection: Connection, _ key: ProjectKey, name: String, computerId: String, task: String,
                       reason: String, computerName: String? = nil) async -> TaskOutcome {
        var payload: JSONObject = ["schema": 1, "project": .string(key.id), "task": .string(task)]
        if !reason.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { payload = payload.putting("reason", .string(reason)) }
        return await act(connection, key, name: name, computerId: computerId, computerName: computerName ?? computerId,
                         operation: "task_cancel", payload: payload)
    }

    /// One page of that computer's Done list; with `session`, of the tasks sent to it.
    public func donePage(_ connection: Connection, _ key: ProjectKey, computerId: String, before: DoneCursor?,
                         limit: Int = 50, session: SessionRef? = nil) async throws -> DonePage {
        guard connection.id == key.connectionId else { throw TaskSyncError("The original task exchange scope changed.") }
        let identity = await identitySchema(connection, computerId: computerId, sessionId: session?.sessionId ?? "")
        var payload: JSONObject = ["schema": .int(identity ? 2 : 1), "project": .string(key.id), "limit": .int(Int64(limit))]
        if let before { payload = payload.putting("before", ["at": .int(before.at), "uuid": .string(before.uuid)]) }
        if let session {
            var ref: JSONObject = ["machine": .string(session.machine), "session": .string(session.session)]
            if identity { ref = ref.putting("session_id", .string(session.sessionId)) }
            payload = payload.putting("session", .object(ref))
        }
        let receipt = try await request(connection, computerId: computerId, operation: "task_done", payload: payload, maxPolls: Self.exchangePolls)
        guard receipt.string("state") == "completed" else {
            let error = receipt.string("error")
            throw TaskSyncError(error.isEmpty ? "The computer did not list its Done tasks." : error)
        }
        guard let result = receipt.object("result") else { throw TaskSyncError("The computer's Done list has no result.") }
        let rows = try result.objects("tasks").map { row in
            let session = try row.object("session").map { value in
                let id: String
                if let raw = value["session_id"] {
                    guard case .string(let string) = raw else { throw TaskSyncError("The Done task has an invalid session identity.") }
                    id = string
                } else { id = "" }
                return SessionRef(machine: value.string("machine"), session: value.string("session"), sessionId: id)
            }
            return DoneRow(uuid: row.string("uuid"), id: row.string("id"), text: row.string("text"), session: session,
                    acceptedAt: Int64(row.int("accepted_at")))
        }
        let next = result.object("next").map { DoneCursor(at: Int64($0.int("at")), uuid: $0.string("uuid")) }
        return DonePage(rows: rows, next: next, total: result.int("total"), recent: result.int("recent"), damaged: result.int("damaged"))
    }

    /// The exchange, then `operation` on that computer, then the exchange that shows its result.
    private func act(_ connection: Connection, _ key: ProjectKey, name: String, computerId: String, computerName: String,
                     operation: String, payload: JSONObject) async -> TaskOutcome {
        await project(key, name: name) { store in
            let task = payload.string("task")
            if operation == "task_send", sends[task] != nil { return .refused(heldSend) }
            do {
                guard connection.id == key.connectionId else { throw TaskSyncError("The original task exchange scope changed.") }
                let evidence = try await bounded { try await self.exchange(connection, store, computerId: computerId, required: [task]) }
                try evidence.requireClean([task])
            }
            catch { return .refused("The tasks could not reach \(computerName): \(TaskSyncError.reason(error))") }
            let outcome: TaskOutcome
            let originalRequestId = UUID().uuidString.lowercased()
            do {
                let receipt = try await request(connection, computerId: computerId, operation: operation, payload: payload,
                                                maxPolls: Self.actionPolls, requestId: originalRequestId)
                let reason = receipt.string("error")
                switch receipt.string("state") {
                case "completed": outcome = .done
                case "failed": outcome = .refused(reason.isEmpty ? "The computer refused it." : reason)
                case "uncertain": outcome = .uncertain(reason.isEmpty ? "The computer could not confirm it." : reason)
                default: outcome = .uncertain("The computer has not answered yet.")
                }
            } catch let error as RelayError where (400..<500).contains(error.status) && error.status != 408 {
                // The relay refused the request itself: nothing was queued.
                outcome = .refused(error.message.isEmpty ? "The relay refused it." : error.message)
            } catch {
                // The request may be queued although its answer was lost.
                outcome = .uncertain("The relay did not confirm it: \(TaskSyncError.reason(error))")
            }
            if operation == "task_send", case .uncertain = outcome {
                sends[task] = PendingSend(peer: PeerKey(project: key, computerId: computerId), requestId: originalRequestId)
            }
            // Its copy comes back now; when that fails the next sync brings it.
            _ = try? await bounded { try await self.exchange(connection, store, computerId: computerId) }
            return outcome
        }
    }

    // MARK: Exchange

    /// Besides its request budget, an exchange ends after its time.
    func bounded<T: Sendable>(_ body: @escaping @Sendable () async throws -> T) async throws -> T {
        let seconds = exchangeTimeout.components.seconds
        return try await withThrowingTaskGroup(of: T?.self) { group in
            group.addTask { try await body() }
            group.addTask { [exchangeTimeout] in
                try await Task.sleep(for: exchangeTimeout)
                return nil
            }
            defer { group.cancelAll() }
            guard let first = try await group.next(), let value = first else {
                throw TaskSyncError("The task exchange took longer than \(seconds) seconds.")
            }
            return value
        }
    }

    /// One whole exchange with the computer; its live digest after the last answer.
    nonisolated func exchange(_ connection: Connection, _ store: TaskStore, computerId: String, fetch: [String] = [], required: [String] = []) async throws -> TaskExchangeEvidence {
        guard required.count <= Self.scopedTasks else { throw TaskSyncError("Too many task outcomes in one exchange.") }
        let exchange = try await replicas.run { fetch.isEmpty ? store.exchange() : store.exchangeFetching(tasks: fetch) }
        var evidence = TaskExchangeEvidence(tasks: required, mediaUnavailable: mediaUnavailable)
        while true {
            guard let request = try await replicas.run({ try exchange.nextRequest() }) else { return evidence }
            guard let payload = try JSON.parse(request).object else { throw TaskSyncError("The task exchange made an invalid request.") }
            let receipt = try await self.request(connection, computerId: computerId, operation: TaskSyncPlan.operation, payload: payload,
                                                 maxPolls: Self.exchangePolls)
            guard receipt.string("state") == "completed" else {
                let error = receipt.string("error")
                throw TaskSyncError(error.isEmpty ? "The computer did not answer the task exchange." : error)
            }
            guard let result = receipt.object("result") else { throw TaskSyncError("The computer's answer has no result.") }
            let answer = JSON.object(result).compact
            evidence.accept(try await replicas.run { try exchange.accept(answer: answer) })
        }
    }

    /// One computer-scoped request through the relay, and its receipt once it settles or the polls run out.
    nonisolated func request(_ connection: Connection, computerId: String, operation: String, payload: JSONObject,
                             maxPolls: Int, requestId: String = UUID().uuidString.lowercased()) async throws -> JSONObject {
        let posted = try await client.post(connection, computerId: computerId, operation: operation, payload: payload,
                                           id: requestId)
        return try await client.waitForReceipt(connection, initial: posted, maxPolls: maxPolls, pollDelay: Self.pollDelay)
    }
}
