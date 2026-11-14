import Foundation

/// Read-only child route outcomes from the computer's bounded inspection.
/// Shared with desktop and Android through question-route-outcome-contract.json.
/// No receipt, answer, retry or outgoing identity is created here.
public enum QuestionRouteActivity {
    public static let source = "question_route_outcome"
    private static func exact(_ value: JSONObject, _ keys: [String]) -> Bool { Set(value.keys) == Set(keys) }
    private static func uuid(_ value: JSON?) -> Bool {
        guard case .string(let text) = value, text != "00000000-0000-0000-0000-000000000000" else { return false }
        return text.range(of: "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$", options: .regularExpression) != nil
    }
    private static func text(_ value: JSON?, _ bytes: Int) -> Bool {
        guard case .string(let text) = value else { return false }
        return !text.isEmpty && text.utf8.count <= bytes
    }
    private static func number(_ value: JSON?, _ min: Int64, _ max: Int64) -> Bool {
        guard case .int(let value) = value else { return false }
        return (min...max).contains(value)
    }
    public static func label(_ line: JSONObject, sessionId: String) -> String? {
        guard exact(line,["request_id","at","role","child","parent","question","outcome"]), line["role"] == .string("child"),
              uuid(line["request_id"]), number(line["at"],1,9_007_199_254_740_991),
              let child = line.object("child"), let parent = line.object("parent"), let question = line.object("question"),
              exact(child,["name","reference","session_id"]), text(child["name"],512), uuid(child["session_id"]),
              child["session_id"] == .string(sessionId), text(child["reference"],1024), child.string("reference").hasPrefix("zerus://session?"),
              exact(parent,["name","session_id"]), text(parent["name"],512), uuid(parent["session_id"]), parent["session_id"] != child["session_id"],
              exact(question,["kind","questions","options","preview"]),
              ["single-choice","multiple-choice","free-text","multi-part"].contains(question.string("kind")),
              number(question["questions"],1,0xffffffff), number(question["options"],0,0xffffffff),
              case .string(let preview) = question["preview"], preview.unicodeScalars.count <= 300,
              !preview.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) }) else { return nil }
        let name = parent.string("name").components(separatedBy: "/").last ?? parent.string("name")
        switch line.string("outcome") {
        case "not_sent": return "Question not sent to \(name)"
        case "uncertain": return "Question delivery to \(name) uncertain"
        default: return nil
        }
    }
    private static func submitted(_ line: JSONObject, sessionId: String) -> Bool {
        var normalized = line
        normalized["outcome"] = .string("not_sent")
        if line["role"] == .string("parent") {
            guard line.object("parent")?["session_id"] == .string(sessionId), text(line["text"],2048) else { return false }
            normalized = normalized.putting("text",nil)
            normalized["role"] = .string("child")
            return label(normalized,sessionId: line.object("child")?.string("session_id") ?? "") != nil
        }
        return label(normalized,sessionId: sessionId) != nil
    }
    public static func events(_ raw: JSONObject) -> [Event] {
        guard ["","main"].contains(raw.string("agent_id")), raw.string("archive_id").isEmpty,
              let lines = raw["question_route_activity"]?.array, lines.count <= 20 else { return [] }
        var events: [Event] = []
        for value in lines {
            guard let line = value.object else { return [] }
            if line["outcome"] == nil {
                guard submitted(line,sessionId: raw.string("session_id")) else { return [] }
                continue
            }
            guard let label = label(line,sessionId: raw.string("session_id")) else { return [] }
            let body = [label,line.object("question")?.string("preview") ?? "",line.object("child")?.string("reference") ?? ""]
                .filter { !$0.isEmpty }.joined(separator: "\n")
            events.append(Event(id: "route-\(line.string("request_id"))",role: "System",text: body,time: String(Double(line.int64("at"))/1000),
                                at: Double(line.int64("at"))/1000,source: source))
        }
        return events
    }
}
