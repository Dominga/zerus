import Foundation

/// The expanded question's verified presence (zerus-a4mub.12, iOS zerus-a4mub.25). It describes evidence only and
/// never grants answer authority: a card is answerable solely when the question itself says so and the verified
/// frame is available. Android's QuestionPresencePolicies checks the same shared contract
/// (mobile/android/app/src/test/resources/question-presence-contract.json).
public enum QuestionPresence: String, Sendable {
    case answerable = "Answerable", waiting = "Waiting", checking = "Checking", closed = "Closed", replaced = "Replaced",
         unavailable = "Unavailable"
}

/// A clear, non-enabling presentation for the expanded question. `modal` is true only for a resolved card; every
/// obsolete state is an inline notice, so an unrelated delivery Review stays reachable.
public struct QuestionPresenceView: Equatable, Sendable {
    public let presence: QuestionPresence
    public let title: String
    public let detail: String
    public let age: String
    public let modal: Bool
    public var refresh = false
    public var replacement: Question?
}

public enum QuestionPresencePolicies {
    /// Resolves the expanded identity against the current projection; nil when nothing is expanded. `checkedAtMillis`
    /// is when the last verified projection was installed (0: never), on the same monotonic clock as `nowMillis`.
    /// `complete` is the projection's pending_questions_complete; an older CLI omits it and counts as complete.
    public static func resolve(_ display: QuestionDisplayState, questions: [Question], verified: Bool, available: Bool,
                               checking: Bool, checkedAtMillis: Int64, nowMillis: Int64, complete: Bool = true) -> QuestionPresenceView? {
        guard display.expanded else { return nil }
        let age = age(checkedAtMillis, now: nowMillis)
        if let exact = display.resolve(questions) {
            let answerable = available && exact.canAnswer
            return QuestionPresenceView(presence: answerable ? .answerable : .waiting, title: answerable ? "" : "Waiting for the agent",
                                        detail: answerable ? "" : reason(exact.unavailableReason, optional: exact.optional), age: age, modal: true)
        }
        // Only a verified and complete projection proves that this exact request is gone; an incomplete one is
        // still being indexed.
        if verified && complete {
            if let replacement = questions.first(where: { $0.id == display.expandedId }) {
                return QuestionPresenceView(presence: .replaced, title: "Request replaced",
                                            detail: "The agent replaced this request with a newer one. Your saved answer is kept.",
                                            age: age, modal: false, replacement: replacement)
            }
            return QuestionPresenceView(presence: .closed, title: "Request closed",
                                        detail: "The agent closed this request. Your answer is no longer needed.", age: age, modal: false)
        }
        if verified {
            return QuestionPresenceView(presence: .checking, title: "Checking the conversation",
                                        detail: "The agent is still indexing this conversation's questions. Refresh to check again. Your saved answer is kept.",
                                        age: age, modal: false, refresh: true)
        }
        if checking {
            return QuestionPresenceView(presence: .checking, title: "Checking the conversation",
                                        detail: "Verifying the pending requests. Your saved answer is kept.", age: age, modal: false)
        }
        return QuestionPresenceView(presence: .unavailable, title: "Request unavailable",
                                    detail: "This request is not in the verified conversation. Refresh to check again. Your saved answer is kept.",
                                    age: age, modal: false, refresh: true)
    }

    /// How old the last verified question projection is; 0 means it was never checked.
    public static func age(_ checkedAtMillis: Int64, now nowMillis: Int64) -> String {
        guard checkedAtMillis > 0 else { return "Not checked yet" }
        let elapsed = max(nowMillis - checkedAtMillis, 0)
        let text: String
        switch elapsed {
        case ..<1_000: text = "just now"
        case ..<60_000: text = "\(elapsed / 1_000)s ago"
        case ..<3_600_000: text = "\(elapsed / 60_000)m ago"
        case ..<86_400_000: text = "\(elapsed / 3_600_000)h ago"
        default: text = "\(elapsed / 86_400_000)d ago"
        }
        return "Checked " + text
    }

    /// Maps the native `input::checked_terminal` veto to a clear reason. Raw reasons never reach the person.
    public static func reason(_ raw: String, optional: Bool) -> String {
        let text = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        if text.isEmpty {
            return optional ? "The agent is still working. This optional request stays open until it asks again."
                : "The agent has not opened answers for this request yet."
        }
        func has(_ word: String) -> Bool { text.range(of: word, options: .caseInsensitive) != nil }
        if has("terminal") { return "Answer this request on your computer." }
        if has("draft") { return "Finish the draft in the composer on your computer." }
        if has("dialog") { return "Close the open dialog on your computer." }
        if has("recogni") || has("prompt") { return "The agent is not showing an answerable prompt yet." }
        if has("working") || has("busy") { return "The agent is still working. Answers stay disabled until it asks again." }
        return "Complete this request on your computer."
    }
}
