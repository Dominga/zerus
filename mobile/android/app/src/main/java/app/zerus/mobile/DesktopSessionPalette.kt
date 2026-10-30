package app.zerus.mobile

enum class DesktopBadgeKind { Neutral, Working, Building, Attention, Error, Paused, Unread, Scheduled, Background, Waiting, Deps, Cooling, Cold }
data class DesktopBadgeColors(val background: Long, val foreground: Long)
data class DesktopSessionBadge(val caption: String, val kind: DesktopBadgeKind, val additionalUnread: Boolean)

/** Native predicates and precedence from SessionCardDelegate.h; colors from SessionStatusBadge.h. */
object DesktopSessionPalette {
    fun badge(session: Session, online: Boolean): DesktopSessionBadge {
        val raw = session.raw
        // U30: a host that reports session_state speaks the seven-state contract; the words and the
        // precedence come from it (SessionState.kt), and the older hosts keep the derivation below.
        if (SessionStateContract.hasHostState(raw) || !online) {
            val status = SessionStateContract.status(raw, online)
            val muted = SessionNotificationPreferences.muted(session)
            // A device-local reminder keeps its attention color when the host
            // is offline or reports a calm state, as on the desktop.
            val reviewLater = !SessionFilters.archived(session) &&
                (raw.optBoolean("review_later") || raw.optBoolean("mobile_review_later"))
            val kind = when {
                muted && (status.state == SessionState.User || reviewLater) -> DesktopBadgeKind.Neutral
                reviewLater -> DesktopBadgeKind.Attention
                else -> sessionStateKind(status.state)
            }
            return DesktopSessionBadge(status.caption, kind, false)
        }
        val state = SessionFilters.status(session, online)
        val attention = raw.optBoolean("review_later") || raw.optBoolean("mobile_review_later") || online && !raw.optBoolean("attention_acknowledged") &&
            (SessionFilters.needsAction(session) || childAttention(session, online))
        val working = SessionFilters.working(session, online)
        val unread = SessionFilters.visibleUnreadReply(session, online)
        val nativeKind = when {
            attention -> if (detailKind(session, online) == DetailKind.Error) DesktopBadgeKind.Error else DesktopBadgeKind.Attention
            working -> if (SessionFilters.backgroundIdle(session)) DesktopBadgeKind.Background else DesktopBadgeKind.Working
            unread -> DesktopBadgeKind.Unread
            online && SessionFilters.waitingIdle(session) -> DesktopBadgeKind.Waiting
            SessionFilters.scheduled(session, online) -> DesktopBadgeKind.Scheduled
            online && SessionFilters.state(session) == "paused" && !SessionFilters.archived(session) -> DesktopBadgeKind.Paused
            else -> DesktopBadgeKind.Neutral
        }
        val muted = SessionNotificationPreferences.muted(session)
        val kind = if(muted && nativeKind in setOf(DesktopBadgeKind.Attention,DesktopBadgeKind.Unread)) DesktopBadgeKind.Neutral else nativeKind
        val caption = when {
            working -> if (raw.string("phase") == "compacting") "Compacting" else if (SessionFilters.backgroundIdle(session)) "Background" else "Working"
            kind == DesktopBadgeKind.Waiting -> SessionFilters.waitingLabel(session,includeProducer=false,compact=true)
            unread && !attention -> "New reply"
            online && !SessionFilters.archived(session) && SessionFilters.state(session) !in listOf("paused", "stopped") &&
                raw.string("process_state") != "exited" && !raw.optBoolean("tracked") && raw.string("activity").isBlank() -> "Untracked"
            else -> state
        }
        return DesktopSessionBadge(caption, kind, unread && (working || attention))
    }

    /** U30: the badge kind of a host-reported state (SESSION-STATES.md section 1). */
    fun sessionStateKind(state: SessionState): DesktopBadgeKind = when (state) {
        SessionState.Working -> DesktopBadgeKind.Working
        SessionState.Building -> DesktopBadgeKind.Building
        SessionState.User -> DesktopBadgeKind.Attention
        SessionState.Deps -> DesktopBadgeKind.Deps
        SessionState.Cooling -> DesktopBadgeKind.Cooling
        SessionState.Cold -> DesktopBadgeKind.Cold
        SessionState.Stopped -> DesktopBadgeKind.Paused
        SessionState.Offline -> DesktopBadgeKind.Neutral
    }

    private fun childAttention(session: Session, online: Boolean): Boolean {
        if (!SessionFilters.currentActivity(session, online) || session.raw.string("subagent_source") == "hook_profiles") return false
        val children = session.raw.optJSONObject("subagents")?.let { roster ->
            roster.keys().asSequence().mapNotNull { roster.optJSONObject(it) }.toList()
        }.orEmpty() + session.raw.optJSONArray("subagent_previews")?.objects().orEmpty()
        return children.any { it.string("display_state", "state") in listOf("approval", "input", "attention") }
    }

    fun colors(kind: DesktopBadgeKind, dark: Boolean): DesktopBadgeColors = when (kind) {
        DesktopBadgeKind.Working -> if (dark) DesktopBadgeColors(0xFF194D36, 0xFF97F0BA) else DesktopBadgeColors(0xFFCCEFDC, 0xFF145B37)
        DesktopBadgeKind.Background -> if (dark) DesktopBadgeColors(0xFF273544, 0xFFAAC0DF) else DesktopBadgeColors(0xFFE9F0F8, 0xFF466B99)
        DesktopBadgeKind.Waiting -> if (dark) DesktopBadgeColors(0xFF443A25, 0xFFEDD096) else DesktopBadgeColors(0xFFF5E8C7, 0xFF795913)
        DesktopBadgeKind.Attention, DesktopBadgeKind.Unread -> DesktopBadgeColors(if (dark) 0xFFFFDA76 else 0xFFF4CE65, 0xFF392900)
        DesktopBadgeKind.Error -> if (dark) DesktopBadgeColors(0xFFFFABB6, 0xFF46202A) else DesktopBadgeColors(0xFFB73750, 0xFFFFFFFF)
        DesktopBadgeKind.Paused -> if (dark) DesktopBadgeColors(0xFF443654, 0xFFDEC3FF) else DesktopBadgeColors(0xFFE8DDF5, 0xFF634187)
        DesktopBadgeKind.Scheduled -> if (dark) DesktopBadgeColors(0xFF173E48, 0xFF9FE0F0) else DesktopBadgeColors(0xFFD3EDF4, 0xFF165A6B)
        DesktopBadgeKind.Neutral -> if (dark) DesktopBadgeColors(0xFF2B343E, 0xFFB6C2D0) else DesktopBadgeColors(0xFFE7EDF2, 0xFF566575)
        // U30: the tokens of SESSION-STATES.md section 6, filled like the badge draws them. Building
        // takes the desktop's Build tone (SessionStatusBadge.h), 13 % of it over the card surface.
        DesktopBadgeKind.Building -> if (dark) DesktopBadgeColors(0xFF203832, 0xFF4CC98A) else DesktopBadgeColors(0xFFE3F2EB, 0xFF2A9D63)
        DesktopBadgeKind.Deps -> if (dark) DesktopBadgeColors(0xFF1B2A44, 0xFF6FA8FF) else DesktopBadgeColors(0xFFE6EFFB, 0xFF2F66C9)
        DesktopBadgeKind.Cooling -> if (dark) DesktopBadgeColors(0xFF12333B, 0xFF56C8E0) else DesktopBadgeColors(0xFFE1F3F8, 0xFF137F95)
        DesktopBadgeKind.Cold -> if (dark) DesktopBadgeColors(0xFF2A3547, 0xFF8FA6C4) else DesktopBadgeColors(0xFFE9EEF5, 0xFF55708F)
    }

    private enum class DetailKind { Neutral, Working, Ready, Attention, Error, Paused, Background, Waiting }
    // This follows SessionPresentation::status ordering, rather than interpreting translated labels.
    private fun detailKind(session: Session, online: Boolean): DetailKind {
        val raw = session.raw
        val phase = raw.string("phase")
        return when {
            !online || SessionFilters.archived(session) -> DetailKind.Neutral
            SessionFilters.state(session) == "paused" -> DetailKind.Paused
            SessionFilters.state(session) == "stopped" || raw.string("process_state") == "exited" -> DetailKind.Neutral
            !raw.optBoolean("tracked") && raw.string("activity").isBlank() -> DetailKind.Neutral
            phase in listOf("approval", "input") -> DetailKind.Attention
            raw.optJSONObject("recovery")?.string("state") in listOf("waiting", "dispatching", "retrying") &&
                !RecoveryPresentation.resumed(raw.optJSONObject("recovery")) -> DetailKind.Neutral
            phase == "error" -> DetailKind.Error
            phase in listOf("interrupted", "starting") -> DetailKind.Neutral
            phase in listOf("compacting", "tool") || raw.string("activity") == "busy" -> DetailKind.Working
            SessionFilters.backgroundIdle(session) -> DetailKind.Background
            SessionFilters.waitingIdle(session) -> DetailKind.Waiting
            raw.string("activity") == "idle" -> DetailKind.Ready
            else -> DetailKind.Neutral
        }
    }

    /** SessionsWindow.cpp tone() and its detail badge background. */
    fun detailColors(session: Session, online: Boolean, dark: Boolean): DesktopBadgeColors {
        val foreground = when (detailKind(session, online)) {
            DetailKind.Working -> if (dark) 0xFF55E39A else 0xFF12834E
            DetailKind.Background -> if (dark) 0xFFAAC0DF else 0xFF466B99
            DetailKind.Waiting -> if (dark) 0xFFEDD096 else 0xFF795913
            DetailKind.Ready -> if (dark) 0xFF78DABB else 0xFF11775D
            DetailKind.Attention -> if (dark) 0xFFF0C77B else 0xFF885400
            DetailKind.Error -> if (dark) 0xFFFF9298 else 0xFFB52B3D
            DetailKind.Paused -> if (dark) 0xFFB9A5ED else 0xFF7051A4
            DetailKind.Neutral -> if (dark) 0xFF9AA4B2 else 0xFF646F7E
        }
        return DesktopBadgeColors(if (dark) 0xFF2B323D else 0xFFEDF1F5, foreground)
    }
}
