import Foundation

public struct TaskEditCheckpoint: Sendable {
    public let sequence: Int64
    public let changes: String
    public init(sequence: Int64, changes: String) { self.sequence = sequence; self.changes = changes }
}

/// The existing debounce strategy isolated for a blocked private-write control.
@MainActor public final class TaskEditCheckpoints {
    private let debounce: Duration
    private let write: @MainActor (TaskEditCheckpoint) async throws -> Void
    private let failed: @MainActor (TaskEditCheckpoint, Error) -> Void
    private var worker: Task<Void, Never>?
    private var failure: Error?
    public init(debounce: Duration = .milliseconds(150), write: @escaping @MainActor (TaskEditCheckpoint) async throws -> Void,
                failed: @escaping @MainActor (TaskEditCheckpoint, Error) -> Void = { _, _ in }) {
        self.debounce = debounce; self.write = write; self.failed = failed
    }
    public func offer(_ value: TaskEditCheckpoint) {
        worker?.cancel()
        worker = Task {
            do { try await Task.sleep(for: debounce); try await write(value); failure = nil }
            catch { failure = error; failed(value, error) }
        }
    }
    public func flush(_ value: TaskEditCheckpoint) async throws { offer(value); await worker?.value; if let failure { throw failure } }
    public func close() { worker?.cancel(); worker = nil }
}
