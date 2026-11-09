import Foundation

/// A local warning choice never changes the original delivery's identity or outcome.
public enum OutgoingRecoveryPolicies {
    public static let explanation = "Dismiss only on this phone. The original message stays saved and may already have been delivered."
    public static let dismissedExplanation = "Warning dismissed on this phone. Delivery is still unconfirmed; you can check its original receipt."

    /// Recovery is allowed only after the caller verified that this exact send's flight has settled.
    public static func dismiss(_ state: MessageState, _ captured: OutgoingMessage, recoverInterruptedSending: Bool = false) -> MessageState {
        let matches = state.outgoing.indices.filter { state.outgoing[$0].requestId == captured.requestId }
        guard captured.status == "uncertain", matches.count == 1, let index = matches.first else { return state }
        var current = state.outgoing[index]
        guard current.target == captured.target, current.text == captured.text,
              current.attachments == captured.attachments, current.createdAt == captured.createdAt,
              !current.deliveryDismissed else { return state }
        if recoverInterruptedSending && current.status == "sending" { current = current.recover() }
        guard current.status == "uncertain" else { return state }
        var copy = state
        copy.outgoing[index] = current
        copy.outgoing[index].deliveryDismissed = true
        return copy
    }
}
