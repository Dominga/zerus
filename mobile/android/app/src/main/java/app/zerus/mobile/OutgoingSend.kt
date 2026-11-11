package app.zerus.mobile

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.withTimeout
import org.json.JSONObject

/**
 * The outbox send entry: one attempt, one settlement (zerus-a4mub.6/.21).
 * [submit] receives the same [SendAttempt] the settlement reads, so a call site
 * cannot drop or replace the transport evidence: the evidence is required by
 * type all the way to the mutation POST, and a missing wiring fails to compile.
 */
internal class OutgoingSend(
    private val attempt: SendAttempt,
    private val submit: suspend (SendAttempt) -> JSONObject,
    private val await: suspend (JSONObject) -> JSONObject,
    private val settle: suspend (JSONObject) -> Unit,
    private val failed: suspend (status: String, message: String) -> Unit,
    private val timeoutMillis: Long = 45_000,
) {
    suspend fun run() {
        var rejected: RelayException? = null
        try {
            settle(withTimeout(timeoutMillis) {
                val initial = try { submit(attempt) }
                catch (e: RelayException) { if (e.status in REJECTED) rejected = e; throw e }
                await(initial)
            })
        } catch (e: Exception) {
            // Positive transport evidence decides the settlement: a failure
            // before the mutation request was enqueued is Not sent, and once it
            // may have left the phone a lost response, timeout or cancellation
            // stays unknown with the same request identity.
            failed(SendSettlement.status(attempt.stage, rejected),
                SendSettlement.message(attempt.stage, rejected, (e as? AttachmentException)?.message))
            if (e is CancellationException && e !is TimeoutCancellationException) throw e
        }
    }

    private companion object {
        val REJECTED = listOf(400, 401, 403, 404, 409, 413, 429)
    }
}

/** The outbox mutation POST. The attempt evidence is required by type (zerus-a4mub.21). */
internal suspend fun submitOutgoing(api: RelayApi, connection: Connection, target: Target, payload: JSONObject, id: String,
    attempt: SendAttempt): JSONObject = api.submit(connection, target, "send", payload, id, attempt)
