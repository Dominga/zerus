import Foundation

/// Labels for saved replica facts only. No runtime observation or task action.
public enum SavedExecutionText {
    public static func phase(_ tag: String) -> String? {
        switch tag {
        case "submitted": "Submitted — begin unconfirmed"
        case "running": "Begin recorded"
        case "paused": "Pause recorded"
        case "ready": "Ready for review recorded"
        case "cancelled": "Cancellation recorded"
        case "not_started": "Not started"
        case "unknown": "Delivery unknown"
        case "unconfirmed": "Begin unconfirmed"
        default: nil
        }
    }

    public static func notices(conflict: Bool, unknown: Bool, unexplained: Bool, legacy: Bool, stale: Bool) -> [String] {
        var result: [String] = []
        if conflict { result.append("Execution records conflict.") }
        if unknown { result.append("Some attempt delivery is unknown.") }
        if unexplained { result.append("Some execution history is unexplained.") }
        if legacy { result.append("Legacy execution record") }
        if stale { result.append("This saved execution could not be refreshed.") }
        return result
    }
}
