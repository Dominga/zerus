package app.zerus.mobile

import org.json.JSONObject

/** Receipt-only polling: reads get a short responsive window before the ordinary cadence; identities and poll count stay bounded. */
internal object ReceiptPolling {
    suspend fun await(initial: JSONObject, maxPolls: Int, pollDelayMs: Long, firstPollDelayMs: Long,
        pause: suspend (Long) -> Unit, receipt: suspend (String) -> JSONObject): JSONObject {
        var result = initial
        val ordinary = pollDelayMs.coerceIn(50, 1000)
        val first = firstPollDelayMs.coerceIn(50, 1000)
        repeat(maxPolls) { index ->
            if (result.string("state") !in listOf("queued", "claimed")) return result
            pause(if (index < 8) minOf(ordinary, if(index==0) first else first*2) else ordinary)
            result = receipt(result.getString("request_id"))
        }
        return result
    }
}
