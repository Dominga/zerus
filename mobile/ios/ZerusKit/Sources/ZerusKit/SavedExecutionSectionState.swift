import Foundation

public struct SavedExecutionRow: Equatable, Sendable {
    public let label: String
    public let value: String?
    public init(_ label: String, _ value: String?) { self.label = label; self.value = value }
}

/// A small presentation value, never a task action or native observation.
public struct SavedExecutionSectionState: Equatable, Sendable {
    public let available: Bool
    public let hasCurrent: Bool
    public let phase: String?
    public let notices: [String]
    public let rows: [SavedExecutionRow]
    public init(available: Bool, hasCurrent: Bool = false, phase: String? = nil,
                notices: [String] = [], rows: [SavedExecutionRow] = []) {
        self.available = available
        self.hasCurrent = hasCurrent
        self.phase = phase
        self.notices = notices
        self.rows = rows
    }
}
