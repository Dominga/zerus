import Foundation
import ZerusReplica

/// Reads the selected local task only. Never resolves a runtime or offers an action.
public enum SavedExecutionPresentation {
    public static func section(_ task: TaskInfo, projectId: String?, stale: Bool, time: (Int64) -> String) -> SavedExecutionSectionState {
        let unavailable = SavedExecutionSectionState(available: false, notices: SavedExecutionText.notices(
            conflict: false, unknown: false, unexplained: false, legacy: false, stale: stale))
        guard let projectId, task.project == projectId, (1...8).contains(task.schema), let e = task.execution,
              e.requirement <= safe, e.attempts <= 256, e.current.map(valid) ?? true else { return unavailable }
        let c = e.current
        var notices = SavedExecutionText.notices(conflict: e.conflict, unknown: e.hasUnknown,
            unexplained: e.hasUnexplained, legacy: c?.legacy == true, stale: stale)
        var rows = [SavedExecutionRow("Current requirement revision", String(e.requirement))]
        if let c {
            if !c.legacy {
                rows.append(SavedExecutionRow("Attempt", c.id))
                rows.append(SavedExecutionRow("Generation", String(c.generation)))
            }
            rows.append(SavedExecutionRow("Requirement revision", String(c.requirement)))
            rows.append(SavedExecutionRow("Request", c.requestId.isEmpty ? nil : c.requestId))
            if c.executor == nil || c.executor?.machine.isEmpty == true || c.executor?.session.isEmpty == true || c.executor?.sessionId.isEmpty == true {
                notices.append("Executor identity unresolved")
            }
            let identity = c.executor.map { [$0.machine, $0.session, $0.sessionId].filter { !$0.isEmpty }.joined(separator: "\n") }
            rows.append(SavedExecutionRow("Recorded executor", identity?.isEmpty == false ? identity : nil))
            rows.append(SavedExecutionRow("Recorded at", c.legacy && c.at == 0 ? nil : time(c.at)))
            rows.append(SavedExecutionRow("Begin recorded at", c.beganAt.map(time)))
            rows.append(SavedExecutionRow("Progress recorded at", c.progressAt.map(time)))
        }
        return SavedExecutionSectionState(available: true, hasCurrent: c != nil,
            phase: c.flatMap { SavedExecutionText.phase($0.phase) }, notices: notices, rows: rows)
    }
    private static let safe: UInt64 = 9_007_199_254_740_991
    private static func text(_ s: String) -> Bool {
        s.utf8.count <= 512 && !s.unicodeScalars.contains { (0...31).contains($0.value) || (127...159).contains($0.value) }
    }
    private static func date(_ t: Int64?) -> Bool { t.map { (-8_640_000_000_000_000...8_640_000_000_000_000).contains($0) } ?? true }
    private static func valid(_ c: TaskAttemptInfo) -> Bool {
        guard text(c.id), text(c.requestId), c.generation <= safe, c.requirement <= safe,
              ["send", "start", "handoff", "reopen"].contains(c.origin), SavedExecutionText.phase(c.phase) != nil,
              date(c.at), date(c.beganAt), date(c.progressAt), c.phase != "running" || c.beganAt != nil else { return false }
        if c.legacy {
            guard c.id.isEmpty, c.generation == 0 else { return false }
        } else {
            guard c.generation > 0, c.id.range(of: "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$", options: .regularExpression) != nil else { return false }
        }
        return c.executor.map { text($0.machine) && text($0.session) && text($0.sessionId) } ?? c.legacy
    }
}
