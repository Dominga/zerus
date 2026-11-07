package app.zerus.mobile

import org.json.JSONObject
import kotlin.math.floor
import kotlin.math.max
import kotlin.math.roundToLong

/**
 * Keep cache warm on the phone, read only: the session's computer decides and
 * changes it (docs/reference.md, Keep cache warm). A running ping is not the
 * person's work: the session reads Ready with "Warming cache", as on the desktop.
 */
object KeepWarmPresentation {
    fun turnActive(raw: JSONObject?): Boolean = raw?.optJSONObject("keep_warm")?.optBoolean("turn_active") == true

    /** "1 h", "48 m", "1 h 30 m". */
    private fun duration(seconds: Double): String {
        val minutes = max(1L, (seconds / 60).roundToLong())
        val hours = minutes / 60
        val rest = minutes % 60
        return when {
            hours == 0L -> "$rest m"
            rest == 0L -> "$hours h"
            else -> "$hours h $rest m"
        }
    }

    private fun left(seconds: Double): String {
        val hours = floor(seconds / 3600).toLong()
        val minutes = floor((seconds % 3600) / 60).toLong()
        return when {
            hours == 0L -> "$minutes m"
            minutes == 0L -> "$hours h"
            else -> "$hours h $minutes m"
        }
    }

    private fun number(warm: JSONObject, key: String): Double? = (warm.opt(key) as? Number)?.toDouble()?.takeIf { it.isFinite() }

    /** The Details fields; none when the computer reports no keep-warm state (an older computer). */
    fun fields(raw: JSONObject?, now: Double): List<ContextDetail> {
        val warm = raw?.optJSONObject("keep_warm") ?: return emptyList()
        if (warm.opt("enabled") !is Boolean) return emptyList()
        val enabled = warm.optBoolean("enabled")
        val until = number(warm, "until")
        val value = when {
            !enabled -> "Off"
            until == null -> "On, always"
            until - now < 60 -> "On, less than a minute left"
            else -> "On, ${left(until - now)} left"
        }
        val reason = warm.string("reason")
        val nextAt = number(warm, "next_at")
        val status = when {
            warm.opt("supported") == false -> reason.ifBlank { "This agent cannot keep its cache warm." }
            !enabled -> ""
            warm.optBoolean("turn_active") -> "Warming the cache now"
            reason.isNotBlank() -> reason
            warm.string("state") == "warming" -> "Warming the cache now"
            warm.string("state") == "waiting" && nextAt != null && nextAt > now -> "Next ping in ${duration(nextAt - now)}"
            else -> ""
        }
        return listOf(ContextDetail("Keep cache warm", value)) + if (status.isNotBlank()) listOf(ContextDetail("Keep-warm status", status)) else emptyList()
    }
}
