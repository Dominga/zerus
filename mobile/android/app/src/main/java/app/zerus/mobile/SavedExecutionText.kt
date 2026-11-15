package app.zerus.mobile

/** Labels for saved replica facts only. No runtime observation or task action. */
object SavedExecutionText {
    fun phase(tag: String): String? = when (tag) {
        "submitted" -> "Submitted — begin unconfirmed"
        "running" -> "Begin recorded"
        "paused" -> "Pause recorded"
        "ready" -> "Ready for review recorded"
        "cancelled" -> "Cancellation recorded"
        "not_started" -> "Not started"
        "unknown" -> "Delivery unknown"
        "unconfirmed" -> "Begin unconfirmed"
        else -> null
    }

    fun notices(conflict: Boolean, unknown: Boolean, unexplained: Boolean, legacy: Boolean, stale: Boolean): List<String> = buildList {
        if (conflict) add("Execution records conflict.")
        if (unknown) add("Some attempt delivery is unknown.")
        if (unexplained) add("Some execution history is unexplained.")
        if (legacy) add("Legacy execution record")
        if (stale) add("This saved execution could not be refreshed.")
    }
}
