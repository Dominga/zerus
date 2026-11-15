import Foundation

public struct TaskEditCheckpoint: Sendable {
    public let sequence: Int64
    public let changes: String
    public init(sequence: Int64, changes: String) { self.sequence = sequence; self.changes = changes }
}

/// One physical write and one latest pending snapshot. Offers never cancel entered I/O.
@MainActor public final class TaskEditCheckpoints {
    private let debounce: Duration
    private let write: @MainActor (TaskEditCheckpoint) async throws -> Void
    private let failed: @MainActor (TaskEditCheckpoint, Error) -> Void
    private var worker: Task<Void, Never>?
    private var latest: TaskEditCheckpoint?
    private var pending: TaskEditCheckpoint?
    private var failure: Error?
    private var closed = false
    public init(debounce: Duration = .milliseconds(150), write: @escaping @MainActor (TaskEditCheckpoint) async throws -> Void,
                failed: @escaping @MainActor (TaskEditCheckpoint, Error) -> Void = { _, _ in }) {
        self.debounce = debounce; self.write = write; self.failed = failed
    }
    @discardableResult public func offer(_ value: TaskEditCheckpoint) -> Bool {
        guard !closed, value.sequence >= 0 else { return false }
        if let latest, value.sequence < latest.sequence || value.sequence == latest.sequence && value.changes != latest.changes { return false }
        latest = value; pending = value
        start()
        return true
    }
    private func start() {
        guard worker == nil, pending != nil, !closed else { return }
        worker = Task { await pump() }
    }
    private func pump() async {
        defer { worker = nil; start() }
        while !closed && pending != nil {
            do { try await Task.sleep(for: debounce) }
            catch { failure = error; return }
            guard !closed, let value = pending else { return }
            pending = nil
            do { try await write(value); failure = nil }
            catch { failure = error; failed(value, error) }
        }
    }
    /// A cancelled caller can stop waiting only after this owned write has physically settled.
    public func flush(_ value: TaskEditCheckpoint) async throws {
        guard offer(value) else { throw ZerusError.invalid("The original task checkpoint is closed or superseded.") }
        while let worker { await worker.value }
        guard !closed else { throw ZerusError.invalid("The original task checkpoint is closed.") }
        if let failure { throw failure }
        try Task.checkCancellation()
    }
    public func close() { closed = true; pending = nil }
}
