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
        else -> local ?: "The saved message was not sent. Review it before trying again."
    }
}
