import Foundation
import ZerusReplica

/// Selected immutable entry and token. Arriving history rows never reinterpret a saved array position.
public struct TaskMediaSelection: Sendable {
    public let task: String, entry: String?, token: String
    public init(task: String, entry: String?, token: String) { self.task = task; self.entry = entry; self.token = token }
    public func read(_ store: TaskStore) throws -> TaskMediaContent {
        let metadata = try store.media(reference: task, entry: entry)
        guard let index = metadata.firstIndex(where: { $0.token == token }), metadata.filter({ $0.token == token }).count == 1 else { throw ZerusError.invalid("Refresh this task to open its media.") }
        return try store.readMedia(reference: task, entry: entry, index: UInt32(index), expectedToken: token)
    }
}
