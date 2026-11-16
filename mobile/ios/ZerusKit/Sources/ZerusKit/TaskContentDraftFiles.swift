import CryptoKit
import Darwin
import Foundation

private func draftError(_ message: String = "The saved task draft is unavailable or changed. Its bytes were kept for review.") -> ZerusError { ZerusError.invalid(message) }
private func draftRequire(_ condition: Bool, _ message: String = "The original task draft is held for review and will not be submitted again.") throws { if !condition { throw draftError(message) } }
private func draftUUID(_ text: String) throws { try draftRequire(UUID(uuidString: text)?.uuidString.lowercased() == text) }

public final class TaskContentDraftFiles: @unchecked Sendable {
    private static let lock = NSRecursiveLock()
    private static let maxRecord = 1024 * 1024
    private let directory: URL
    private let keys: KeyProvider
    private let maxEntries: Int
    private let maxBytes: Int64
    private let syncDirectory: @Sendable () throws -> Void
    public init(directory: URL, keys: KeyProvider, maxEntries: Int = 32, maxBytes: Int64 = 16 * 1024 * 1024,
                syncDirectory: @escaping @Sendable () throws -> Void) {
        self.directory = directory; self.keys = keys; self.maxEntries = maxEntries; self.maxBytes = maxBytes; self.syncDirectory = syncDirectory
    }
    public static func applicationSupport() throws -> TaskContentDraftFiles {
        let support = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
        let root = support.appendingPathComponent("Zerus/task-content-drafts-v1", isDirectory: true)
        return TaskContentDraftFiles(directory: root, keys: KeychainKeyProvider(account: "task-content-drafts-v1"), syncDirectory: {
            let fd = Darwin.open(root.path, O_RDONLY | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW)
            guard fd >= 0 else { throw draftError("The private task draft directory could not be synced. No request was sent.") }
            defer { Darwin.close(fd) }
            guard Darwin.fsync(fd) == 0 else { throw draftError("The private task draft directory could not be synced. No request was sent.") }
        })
    }
    private func locked<T>(_ action: () throws -> T) rethrows -> T { Self.lock.lock(); defer { Self.lock.unlock() }; return try action() }
    public func create(_ value: TaskContentDraft) throws -> TaskContentDraft { try locked {
        try draftRequire(read(value.scope, isNew: value.isNew) == nil); return try write(value)
    } }
    public func checkpoint(_ original: TaskContentDraft, sequence: Int64, changes: String) throws -> TaskContentDraft { try locked {
        var previous = try self.original(original)
        try draftRequire(previous.state == .editing && sequence >= previous.form.sequence)
        try draftRequire(sequence != previous.form.sequence || changes == previous.form.json())
        let form = try TaskEditForm.read(changes, sequence: sequence)
        try draftRequire(form.title == previous.form.title && form.originalTarget == previous.form.originalTarget && form.originalRank == previous.form.originalRank)
        previous.form = form; return try write(previous)
    } }
    public func begin(_ value: TaskContentDraft) throws -> TaskContentDraft { try locked {
        var previous = try original(value); try draftRequire(previous == value && value.state == .editing)
        previous.state = .applying; return try write(previous)
    } }
    public func completed(_ value: TaskContentDraft) throws -> TaskContentDraft { try locked {
        var previous = try original(value), expected = value; expected.state = .applying
        try draftRequire(previous == expected); previous.state = .completed; return try write(previous)
    } }
    public func release(_ value: TaskContentDraft) throws { try locked {
        let previous = try original(value)
        try draftRequire(previous.state != .applying)
        try FileManager.default.removeItem(at: file(value.scope, isNew: value.isNew)); try syncDirectory()
    } }
    private func original(_ value: TaskContentDraft) throws -> TaskContentDraft {
        guard let previous = try read(value.scope, isNew: value.isNew) else { throw draftError() }
        try draftRequire(previous.id == value.id && previous.computer == value.computer && previous.revision == value.revision && previous.stage == value.stage)
        return previous
    }
    public func read(_ scope: TaskEditDraftScope, isNew: Bool) throws -> TaskContentDraft? { try locked {
        try draftUUID(scope.task)
        guard exists(directory) else { return nil }
        try ensureRoot()
        let url = try file(scope, isNew: isNew)
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
        let record = try TaskContentDraft.read(json); try draftRequire(record.scope == scope && record.isNew == isNew); return record
    } }
    private func file(_ scope: TaskEditDraftScope, isNew: Bool) throws -> URL {
        let data = WorkspaceProtocol.frame("zerus-task-content-key-v1", [scope.connectionId, scope.swarmId, scope.project, scope.task].map { Data($0.utf8) })
        let name = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
        return directory.appendingPathComponent((isNew ? "new-" : "task-") + name + ".sealed")
    }
    private func aad(_ url: URL) -> Data { Data(("zerus-task-content-v1:" + url.lastPathComponent).utf8) }
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
    private func write(_ value: TaskContentDraft) throws -> TaskContentDraft {
        try value.validate(); try ensureRoot()
        let raw = try JSON.object(value.json()).data(); try draftRequire(raw.count <= Self.maxRecord)
        let destination = try file(value.scope, isNew: value.isNew)
        guard let packed = try AES.GCM.seal(raw, using: keys.key(), authenticating: aad(destination)).combined else { throw draftError() }
        var enumerationFailed = false
        guard let rows = FileManager.default.enumerator(at: directory, includingPropertiesForKeys: [.isRegularFileKey, .fileSizeKey], options: [.skipsSubdirectoryDescendants], errorHandler: { _, _ in enumerationFailed = true; return false }) else { throw draftError() }
        var count = 0, bytes: Int64 = 0
        while let url = rows.nextObject() as? URL {
            count += 1; try draftRequire(count <= maxEntries)
            let attributes = try FileManager.default.attributesOfItem(atPath: url.path)
            try draftRequire(attributes[.type] as? FileAttributeType == .typeRegular)
            // Enumeration can normalize the parent URL differently. Slot names
            // are unique within this checked directory; a replacement charges
            // its new ciphertext below, without charging the old one again.
            if url.lastPathComponent != destination.lastPathComponent { bytes += (attributes[.size] as? NSNumber)?.int64Value ?? maxBytes }
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
