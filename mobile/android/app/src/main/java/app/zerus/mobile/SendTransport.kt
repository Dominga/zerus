package app.zerus.mobile

import java.util.concurrent.atomic.AtomicBoolean

/** Where one outgoing attempt stands relative to its first mutation request. */
enum class TransportStage { Local, Enqueued }

/**
 * Positive transport evidence for one outbox send (zerus-a4mub.6). The API marks
 * it at the exact point a mutation request is handed to the HTTP client, and
 * nothing else may set it. Settlement decides from this evidence instead of
 * guessing from an exception type: only `Local` proves that no mutation request
 * was enqueued. Once `Enqueued`, a lost response, timeout or cancellation stays
 * unknown, because delivery may already have begun.
 */
class SendAttempt {
    private val enqueued = AtomicBoolean(false)
    val stage: TransportStage
        get() = if (enqueued.get()) TransportStage.Enqueued else TransportStage.Local
    internal fun enqueued() { enqueued.set(true) }
}

/** Settlement of one failed send, decided only by transport evidence. */
object SendSettlement {
    /** `failed` is the UI's "Not sent", including a server refusal. */
    fun status(stage: TransportStage, rejected: RelayException?): String = when {
        rejected != null -> "failed"
        stage == TransportStage.Enqueued -> "uncertain"
        else -> "failed"
    }

    fun message(stage: TransportStage, rejected: RelayException?, local: String?): String = when {
        rejected != null -> rejected.message ?: "The computer rejected this message."
        stage == TransportStage.Enqueued -> "Delivery is unconfirmed. Check its receipt or dismiss the warning to continue."
        else -> local?.takeIf { it.isNotBlank() } ?: "The saved message was not sent. Review it before trying again."
    }

    /** The actionable local reason to show for a positively local failure.
     *  Only these reviewed, user-facing texts cross; transport or library text
     *  never does, so an unknown local failure keeps the generic message. */
    fun localReason(error: Throwable): String? {
        (error as? AttachmentException)?.message?.let { return it }
        val message = error.message ?: return null
        return message.takeIf { it in LOCAL_REASONS }
    }

    // Keep in sync with the send path's own user-facing reasons (the English
    // source is the catalog; mobile has no resource string table).
    private val LOCAL_REASONS = setOf(
        "Refresh this computer's encrypted route before sending.",
        "Encrypted outgoing storage is full. Check outstanding deliveries.",
        "The session changed before transport. Review the saved message.",
        "Compact continuation was canceled. The captured message was not sent and remains recoverable.",
        "The executing gateway is no longer enrolled.",
        "This encrypted phone enrollment is not active.",
        "This phone has been removed from the workspace.",
        "Waiting for the inviting computer to approve this phone. Refresh to check again.",
        "Workspace membership was rolled back or forked.",
        "Encrypted workspace storage is unavailable. Plaintext fallback is disabled.",
    )
}
