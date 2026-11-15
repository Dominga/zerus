import CryptoKit
import Darwin
import Foundation

public struct TaskEditDraftScope: Equatable, Sendable {
    public let connectionId: String
    public let swarmId: String
    public let project: String
    public let task: String
    public init(connectionId: String, swarmId: String, project: String, task: String) {
        self.connectionId = connectionId; self.swarmId = swarmId; self.project = project; self.task = task
    }
    fileprivate var json: JSONObject { ["connection": .string(connectionId), "swarm": .string(swarmId), "project": .string(project), "task": .string(task)] }
    fileprivate func validate() throws {
        for text in [connectionId, swarmId, project] {
            try draftRequire(!text.isEmpty && text.utf8.count <= 512 && !text.unicodeScalars.contains(where: CharacterSet.controlCharacters.contains))
        }
        try draftUUID(task)
    }
    fileprivate static func parse(_ value: JSONObject) throws -> Self {
        try WorkspaceProtocol.fields(value, ["connection", "swarm", "project", "task"])
        let scope = try Self(connectionId: WorkspaceProtocol.string(value, "connection"), swarmId: WorkspaceProtocol.string(value, "swarm"),
                             project: WorkspaceProtocol.string(value, "project"), task: WorkspaceProtocol.string(value, "task"))
        try scope.validate(); return scope
    }
}
public enum TaskEditDraftState: String, Sendable { case editing = "Editing", frozen = "Frozen", pending = "Pending", unknown = "Unknown", completed = "Completed", refused = "Refused" }
public enum TaskEditDraftOutcome: Sendable { case acknowledged, completed, refused, unknown }

/// Saved local evidence. This value cannot reconstruct an FFI edit handle or authorize a POST.
public struct TaskEditSavedDraft: Equatable, Sendable, CustomStringConvertible {
    public let scope: TaskEditDraftScope
    public let draftId: String
    public let machineId: String
    public let hostDevice: String
    public let qualificationId: String
    public let editId: String
    public let original: String
    public fileprivate(set) var changes = ""
    public fileprivate(set) var changeSequence: Int64 = 0
    public fileprivate(set) var state: TaskEditDraftState = .editing
    public fileprivate(set) var qualified = false
    public fileprivate(set) var manifest: String?
    public fileprivate(set) var pendingId: String?
    public fileprivate(set) var pendingPayload: String?
    public fileprivate(set) var usedRequests: [String] = []
    public var description: String { "TaskEditSavedDraft(id=\(draftId), state=\(state.rawValue))" }
    fileprivate var json: JSONObject {
        ["version": 1, "scope": .object(scope.json), "draft_id": .string(draftId), "machine_id": .string(machineId),
         "host_device": .string(hostDevice), "qualification_id": .string(qualificationId), "edit_id": .string(editId),
         "original": .string(original), "changes": .string(changes), "change_sequence": .int(changeSequence),
         "state": .string(state.rawValue), "qualified": .bool(qualified),
         "manifest": manifest.map(JSON.string) ?? .null, "pending_id": pendingId.map(JSON.string) ?? .null,
         "pending_payload": pendingPayload.map(JSON.string) ?? .null, "used_requests": .array(usedRequests.map(JSON.string))]
    }
    fileprivate func validate() throws {
        try scope.validate()
        for id in [draftId, machineId, qualificationId, editId] { try draftUUID(id) }
        try WorkspaceProtocol.identifier(hostDevice)
        try draftRequire(qualificationId != editId && usedRequests.count <= 256 && Set(usedRequests).count == usedRequests.count)
        try draftRequire(changeSequence >= 0)
        for id in usedRequests { try draftUUID(id) }
        try draftRequire(!qualified || usedRequests.contains(qualificationId))
        try draftRequire(manifest == nil || qualified && manifest?.isEmpty == false)
        try draftRequire((pendingId == nil) == (pendingPayload == nil))
        if let pendingId {
            try draftUUID(pendingId); try draftRequire(usedRequests.contains(pendingId) && pendingPayload?.isEmpty == false)
        }
        switch state {
        case .editing: try draftRequire(manifest == nil && pendingId == nil)
        case .frozen: try draftRequire(qualified && manifest != nil && pendingId == nil)
        case .pending, .unknown, .refused: try draftRequire(pendingId != nil)
        case .completed: try draftRequire(qualified && manifest != nil && pendingId == editId)
        }
        if !qualified, let pendingId { try draftRequire(pendingId == qualificationId && manifest == nil) }
    }
    fileprivate static func parse(_ value: JSONObject) throws -> Self {
        try WorkspaceProtocol.fields(value, ["version", "scope", "draft_id", "machine_id", "host_device", "qualification_id", "edit_id", "original", "changes", "change_sequence", "state", "qualified", "manifest", "pending_id", "pending_payload", "used_requests"])
        guard try WorkspaceProtocol.number(value, "version") == 1, let scope = value["scope"]?.object,
              case .bool(let qualified)? = value["qualified"], let rawUsed = value["used_requests"]?.array,
              rawUsed.count <= 256, let state = TaskEditDraftState(rawValue: try WorkspaceProtocol.string(value, "state")) else { throw draftError() }
        func nullable(_ name: String) throws -> String? { value[name]?.isNull == true ? nil : try WorkspaceProtocol.string(value, name) }
        var record = try Self(scope: TaskEditDraftScope.parse(scope), draftId: WorkspaceProtocol.string(value, "draft_id"),
                              machineId: WorkspaceProtocol.string(value, "machine_id"), hostDevice: WorkspaceProtocol.string(value, "host_device"),
                              qualificationId: WorkspaceProtocol.string(value, "qualification_id"), editId: WorkspaceProtocol.string(value, "edit_id"), original: WorkspaceProtocol.string(value, "original"))
        record.state = state; record.qualified = qualified
        record.changes = try WorkspaceProtocol.string(value, "changes"); record.changeSequence = try WorkspaceProtocol.number(value, "change_sequence")
        record.manifest = try nullable("manifest"); record.pendingId = try nullable("pending_id"); record.pendingPayload = try nullable("pending_payload")
        record.usedRequests = try rawUsed.map { item in guard case .string(let id) = item else { throw draftError() }; return id }
        try record.validate(); return record
    }
}
private func draftError(_ message: String = "The saved task draft is unavailable or changed. Its bytes were kept for review.") -> ZerusError { ZerusError.invalid(message) }
private func draftRequire(_ condition: Bool, _ message: String = "The original task draft is held for review and will not be submitted again.") throws { if !condition { throw draftError(message) } }
private func draftUUID(_ text: String) throws { try draftRequire(UUID(uuidString: text)?.uuidString.lowercased() == text) }

/// Bounded encrypted records with no automatic expiry or eviction. Call off the main actor.
/// Pending records after restart remain held for read-only review; they never recreate a live handle.
public final class TaskEditDraftFiles: @unchecked Sendable {
    private static let lock = NSRecursiveLock()
    private static let maxRecord = 64 * 1024 * 1024
    private let directory: URL
    private let keys: KeyProvider
    private let maxEntries: Int
    private let maxBytes: Int64
    private let syncDirectory: @Sendable () throws -> Void
    public init(directory: URL, keys: KeyProvider, maxEntries: Int = 32, maxBytes: Int64 = 256 * 1024 * 1024,
                syncDirectory: @escaping @Sendable () throws -> Void) {
        self.directory = directory; self.keys = keys; self.maxEntries = maxEntries; self.maxBytes = maxBytes; self.syncDirectory = syncDirectory
    }
    public static func applicationSupport() throws -> TaskEditDraftFiles {
        let support = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
        let root = support.appendingPathComponent("Zerus/task-edit-drafts-v1", isDirectory: true)
        return TaskEditDraftFiles(directory: root, keys: KeychainKeyProvider(account: "task-edit-drafts-v1"), syncDirectory: {
            let fd = Darwin.open(root.path, O_RDONLY | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW)
            guard fd >= 0 else { throw draftError("The private task draft directory could not be synced. No request was sent.") }
            defer { Darwin.close(fd) }
            guard Darwin.fsync(fd) == 0 else { throw draftError("The private task draft directory could not be synced. No request was sent.") }
        })
    }
    private func locked<T>(_ action: () throws -> T) rethrows -> T { Self.lock.lock(); defer { Self.lock.unlock() }; return try action() }
    public func create(_ scope: TaskEditDraftScope, machineId: String, hostDevice: String, qualificationId: String, editId: String, original: String) throws -> TaskEditSavedDraft { try locked {
        try scope.validate(); try ensureRoot()
        try draftRequire(!exists(file(scope)), "This task already has a saved draft. Review it before creating another.")
        var record = TaskEditSavedDraft(scope: scope, draftId: UUID().uuidString.lowercased(), machineId: machineId, hostDevice: hostDevice,
                                       qualificationId: qualificationId, editId: editId, original: original)
        record.changes = original
        return try write(record)
    } }
    public func read(_ scope: TaskEditDraftScope) throws -> TaskEditSavedDraft? { try locked {
        try scope.validate()
        guard exists(directory) else { return nil }
        try ensureRoot()
        let url = try file(scope)
        guard exists(url) else { return nil }
        let attributes = try FileManager.default.attributesOfItem(atPath: url.path)
        let size = (attributes[.size] as? NSNumber)?.int64Value ?? -1
        try draftRequire(attributes[.type] as? FileAttributeType == .typeRegular && size >= 29 && size <= Int64(Self.maxRecord + 28))
        let handle = try FileHandle(forReadingFrom: url); defer { try? handle.close() }
        var packed = Data()
        while let part = try handle.read(upToCount: 64 * 1024), !part.isEmpty {
            try draftRequire(packed.count + part.count <= Self.maxRecord + 28); packed.append(part)
        }
        let clear = try AES.GCM.open(AES.GCM.SealedBox(combined: packed), using: keys.key(), authenticating: aad(url))
        try draftRequire(clear.count <= Self.maxRecord)
        guard let json = try JSON.parse(clear).object else { throw draftError() }
        let record = try TaskEditSavedDraft.parse(json); try draftRequire(record.scope == scope); return record
    } }
    public func edit(_ scope: TaskEditDraftScope, draftId: String, sequence: Int64, changes: String) throws -> TaskEditSavedDraft { try locked {
        var previous = try original(scope, id: draftId)
        if previous.state == .editing && previous.qualified && sequence == previous.changeSequence && changes == previous.changes { return previous }
        try draftRequire(previous.state == .editing && previous.qualified && sequence > previous.changeSequence)
        previous.changes = changes; previous.changeSequence = sequence; return try write(previous)
    } }
    public func freeze(_ scope: TaskEditDraftScope, draftId: String, manifest: String, expectedSequence: Int64 = 0) throws -> TaskEditSavedDraft { try locked {
        var previous = try original(scope, id: draftId)
        try draftRequire(previous.state == .editing && previous.qualified && previous.manifest == nil && !manifest.isEmpty && previous.changeSequence == expectedSequence)
        previous.state = .frozen; previous.manifest = manifest; return try write(previous)
    } }
    public func reserve(_ scope: TaskEditDraftScope, draftId: String, requestId: String, payload: String) throws -> TaskEditSavedDraft { try locked {
        var previous = try original(scope, id: draftId); try draftUUID(requestId)
        try draftRequire([.editing, .frozen].contains(previous.state) && !previous.usedRequests.contains(requestId) && !payload.isEmpty)
        try draftRequire(!previous.qualified ? requestId == previous.qualificationId : previous.manifest != nil && requestId != previous.qualificationId)
        previous.state = .pending; previous.pendingId = requestId; previous.pendingPayload = payload; previous.usedRequests.append(requestId)
        return try write(previous)
    } }
    public func releaseCompleted(_ scope: TaskEditDraftScope, draftId: String) throws { try locked {
        let previous = try original(scope, id: draftId)
        try draftRequire(previous.state == .completed, "Only this task's verified completed edit can release its draft.")
        try FileManager.default.removeItem(at: file(scope))
        try syncDirectory()
    } }
    /// Only after an explicit person confirmation. Unknown delivery cannot be discarded here.
    public func discardUnsubmitted(_ scope: TaskEditDraftScope, draftId: String) throws { try locked {
        let previous = try original(scope, id: draftId)
        try draftRequire([.editing, .refused].contains(previous.state) || previous.state == .frozen && !previous.usedRequests.contains(previous.editId),
                         "This task edit may have been submitted. Its draft must stay for review.")
        try FileManager.default.removeItem(at: file(scope))
        try syncDirectory()
    } }
    /// The caller must first verify the original FFI proof before supplying an acknowledged outcome.
    public func settle(_ scope: TaskEditDraftScope, draftId: String, requestId: String, outcome: TaskEditDraftOutcome) throws -> TaskEditSavedDraft { try locked {
        var previous = try original(scope, id: draftId)
        try draftRequire(previous.state == .pending && previous.pendingId == requestId)
        switch outcome {
        case .acknowledged:
            try draftRequire(requestId != previous.editId)
            previous.state = previous.manifest == nil ? .editing : .frozen; previous.qualified = true; previous.pendingId = nil; previous.pendingPayload = nil
        case .completed:
            try draftRequire(requestId == previous.editId && previous.manifest != nil); previous.state = .completed
        case .refused: previous.state = .refused
        case .unknown: previous.state = .unknown
        }
        return try write(previous)
    } }
    private func original(_ scope: TaskEditDraftScope, id: String) throws -> TaskEditSavedDraft {
        guard let record = try read(scope), record.draftId == id else { throw draftError("The original task draft identity changed or is unavailable.") }; return record
    }
    private func file(_ scope: TaskEditDraftScope) throws -> URL {
        let data = WorkspaceProtocol.frame("zerus-task-draft-key-v1", [scope.connectionId, scope.swarmId, scope.project, scope.task].map { Data($0.utf8) })
        let name = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
        return directory.appendingPathComponent(name + ".sealed")
    }
    private func aad(_ url: URL) -> Data { Data(("zerus-task-edit-draft-v1:" + url.lastPathComponent).utf8) }
    private func exists(_ url: URL) -> Bool {
        // lstat recognizes a symlink even when its target is missing.
        var info = stat(); return url.path.withCString { Darwin.lstat($0, &info) == 0 }
    }
    private func ensureRoot() throws {
        if !exists(directory) { try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700]) }
        let attributes = try FileManager.default.attributesOfItem(atPath: directory.path)
        try draftRequire(attributes[.type] as? FileAttributeType == .typeDirectory)
        try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: directory.path)
        var root = directory, values = URLResourceValues(); values.isExcludedFromBackup = true; try root.setResourceValues(values)
    }
    private func write(_ value: TaskEditSavedDraft) throws -> TaskEditSavedDraft {
        try value.validate(); try ensureRoot()
        let raw = try JSON.object(value.json).data(); try draftRequire(raw.count <= Self.maxRecord)
        let destination = try file(value.scope)
        guard let packed = try AES.GCM.seal(raw, using: keys.key(), authenticating: aad(destination)).combined else { throw draftError() }
        var enumerationFailed = false
        guard let rows = FileManager.default.enumerator(at: directory, includingPropertiesForKeys: [.isRegularFileKey, .fileSizeKey], options: [.skipsSubdirectoryDescendants], errorHandler: { _, _ in enumerationFailed = true; return false }) else { throw draftError() }
        var count = 0, bytes: Int64 = 0
        while let url = rows.nextObject() as? URL {
            count += 1; try draftRequire(count <= maxEntries)
            let attributes = try FileManager.default.attributesOfItem(atPath: url.path)
            try draftRequire(attributes[.type] as? FileAttributeType == .typeRegular)
            if url != destination { bytes += (attributes[.size] as? NSNumber)?.int64Value ?? maxBytes }
            try draftRequire(bytes <= maxBytes)
        }
        try draftRequire(!enumerationFailed)
        try draftRequire((exists(destination) || count < maxEntries) && bytes + Int64(packed.count) <= maxBytes, "Private task draft storage is full. Saved drafts were kept.")
        #if os(iOS)
        try packed.write(to: destination, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
        #else
        try packed.write(to: destination, options: [.atomic])
        #endif
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: destination.path)
        let fd = Darwin.open(destination.path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW)
        guard fd >= 0 else { throw draftError("The private task draft could not be synced. No request was sent.") }
        defer { Darwin.close(fd) }
        guard Darwin.fsync(fd) == 0 else { throw draftError("The private task draft could not be synced. No request was sent.") }
        try syncDirectory()
        return value
    }
}
