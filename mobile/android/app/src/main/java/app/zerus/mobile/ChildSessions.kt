package app.zerus.mobile

import org.json.JSONArray
import org.json.JSONObject
import java.text.BreakIterator
import java.util.Locale

/**
 * Child sessions on Android (zerus-sa9qn.4), the desktop contract of
 * docs/superpowers/specs/2026-11-07-account-labels-child-sessions-design.md:
 * a row's `parent` link nests it under its parent, and a child of a live
 * parent keeps quiet unless only the person can answer it. Pure functions of
 * the `hgs ls --json` rows, so the shared fixture
 * client/core/tests/fixtures/supervised-attention.json pins them exactly as
 * the desktop core (client/core/src/view.rs) projects them.
 */
data class ParentLink(val machine: String, val sessionId: String, val conversationId: String, val name: String, val state: String) {
    /** The child's computer verified this parent running there. */
    val live: Boolean get() = state == "live"
    /** The parent's short name, as the desktop's supervised mark and from-parent chip show it. */
    val shortName: String get() = name.substringAfterLast('/').ifEmpty { name }
}

/**
 * A parent's descendants in its project group: all of them, those working,
 * quiet requests they hold for it (`asking`), requests only the person can
 * answer (`needsYou`) and errors. Informational: it never raises the parent's
 * own attention (spec 2.3).
 */
data class SessionTeam(val total: Int = 0, val working: Int = 0, val asking: Int = 0, val needsYou: Int = 0, val errors: Int = 0) {
    /** With the parent's shown native subagents: one counter for both (zerus-sa9qn.16). Their requests stay the parent's. */
    fun withSubagents(rows: List<SessionCardChild>) = copy(total = total + rows.size,
        working = working + rows.count { it.kind == DesktopBadgeKind.Working })
    /** The toggle's spoken summary, without dots between values. */
    fun summary(): List<String> = listOfNotNull("$working of $total working", "Needs you: $needsYou".takeIf { needsYou > 0 },
        "Asking: $asking".takeIf { asking > 0 }, "Errors: $errors".takeIf { errors > 0 })
}

/** One card in a project group: nested under `parentKey` at `depth`, a parent's `team`, or `fromParent` for a child shown at top level. */
data class SessionCardEntry(val session: Session, val depth: Int = 0, val parentKey: String? = null, val lastChild: Boolean = false,
    val team: SessionTeam? = null, val fromParent: ParentLink? = null, val fromParentKey: String? = null)

/**
 * Parents whose subagents and child sessions are hidden: one toggle for both
 * (zerus-sa9qn.16), kept across relaunches by machine UUID and session ID
 * (zerus-sa9qn.15), never by name or run. Expanded is the default. A choice
 * made here wins over the stored one by row key; a row without durable IDs,
 * or one chosen before the stored list loads, keeps its choice while the app
 * runs. Those choices are bounded too, newest last (zerus-mepi6): a row key
 * names a run and conversation, so resumes would otherwise add entries forever.
 */
data class TeamCollapse(val stored: List<String> = emptyList(), val overrides: Map<String, Boolean> = emptyMap()) {
    private val ids = stored.toHashSet()
    fun collapsed(session: Session, machine: String?): Boolean =
        overrides[session.target.key] ?: ChildSessions.teamId(machine, session)?.let { it in ids } ?: false

    /**
     * After one choice for row `key`: a choice saved in `saved` (the stored
     * list it produced) needs no override; otherwise the row keeps it here.
     */
    fun choose(key: String, collapsed: Boolean, saved: List<String>?): TeamCollapse =
        if (saved != null) TeamCollapse(saved, overrides - key)
        else TeamCollapse(stored, (overrides - key).entries.map { it.key to it.value }.plus(key to collapsed)
            .takeLast(MAX_OVERRIDES).toMap())

    companion object {
        /** Choices this app keeps by row key while it runs. */
        const val MAX_OVERRIDES = 256
    }
}

/** An account mark: the catalog label, its letter and the account's color, the label's default palette slot unless its computer keeps one (spec 1.6, zerus-sa9qn.6). */
data class AccountMark(val label: String, val letter: String, val color: Long, val ink: Long, val provider: String)

object ChildSessions {
    /** Nesting depth bound, as the desktop list (group-rows.ts MAX_DEPTH). */
    const val MAX_DEPTH = 8
    /** Collapsed parents this phone keeps, newest last (desktop sessions/collapsedTeams). */
    const val MAX_COLLAPSED_TEAMS = 2048
    private val UUID = Regex("^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
    private fun field(link: JSONObject, key: String): String =
        link.optString(key).takeIf { link.opt(key) is String && it.length <= 512 && it.none { c -> c.isISOControl() } }.orEmpty()

    /** The row's parent link when well formed: a canonical machine UUID, a session ID or conversation, a name. */
    fun parent(raw: JSONObject): ParentLink? {
        val link = raw.optJSONObject("parent") ?: return null
        val parent = ParentLink(field(link, "machine"), field(link, "session_id"), field(link, "conversation_id"), field(link, "name"), field(link, "state"))
        val identity = UUID.matches(parent.sessionId) || (parent.sessionId.isEmpty() && parent.conversationId.isNotEmpty())
        return parent.takeIf { UUID.matches(it.machine) && identity && it.name.isNotEmpty() }
    }

    /** The parent runs on the child's computer: the supervised mark, muted or not (spec 3.4). */
    fun supervised(raw: JSONObject): Boolean = parent(raw)?.live == true

    /** A request only the person may answer: an approval (also a native subagent's), signing in or an authentication failure. */
    fun personOnlyRequest(raw: JSONObject): Boolean {
        fun approval(child: JSONObject?) = child != null && listOf("display_state", "state").any { child.opt(it) == "approval" }
        val subagents = raw.optJSONObject("subagents")
        val previews = raw.optJSONArray("subagent_previews") ?: JSONArray()
        return raw.opt("phase") == "approval" ||
            raw.optJSONObject("provider_error")?.opt("error_kind") == "authentication" ||
            raw.opt("activity_summary") == "Sign in required" ||
            (subagents?.keys()?.asSequence()?.any { approval(subagents.optJSONObject(it)) } == true) ||
            (0 until previews.length()).any { approval(previews.optJSONObject(it)) }
    }

    /**
     * Whether the row's attention is its parent's: a live parent, an explicit
     * host classification that no request is the person's, and none only the
     * person can answer. An older or unknown host keeps the person's attention.
     */
    fun quiet(raw: JSONObject): Boolean =
        supervised(raw) && raw.opt("human_request") == false && !personOnlyRequest(raw)

    /** Gray badges and no notification: muted on all devices, or quiet (spec 2.1, the mute treatment). */
    fun silenced(session: Session): Boolean = SessionNotificationPreferences.muted(session) || quiet(session.raw)

    /**
     * Each listed row by its computer's machine UUID and durable session ID,
     * and by conversation for a parent an older hgs listed without one. Names
     * never match. Returns, for a row, its listed parent's key or null
     * (desktop model.ts parentIndex).
     */
    fun parentIndex(rows: Iterable<Session>, machine: (Session) -> String?): (Session) -> String? {
        val bySession = HashMap<String, String>()
        val byConversation = HashMap<String, Session>()
        for (row in rows) {
            val id = machine(row)?.takeIf { it.isNotEmpty() } ?: continue
            row.raw.string("session_id").takeIf { it.isNotEmpty() }?.let { bySession["$id\n$it"] = row.target.key }
            row.raw.string("conversation_id").takeIf { it.isNotEmpty() }?.let { byConversation["$id\n$it"] = row }
        }
        return { row ->
            parent(row.raw)?.let { p ->
                p.sessionId.takeIf { it.isNotEmpty() }?.let { bySession["${p.machine}\n$it"] }
                    ?: p.conversationId.takeIf { it.isNotEmpty() }?.let { byConversation["${p.machine}\n$it"] }
                        // An identified link only falls back to a parent row that lists no session ID.
                        ?.takeIf { old -> p.sessionId.isEmpty() || old.raw.string("session_id").isEmpty() }?.target?.key
            }
        }
    }

    /**
     * One project group's visible cards in their order, child sessions nested
     * under a parent shown in the same list (desktop group-rows.ts). Nesting
     * never hides a row by itself: a loop or a parent elsewhere leaves the card
     * at top level. A collapsed parent hides its descendants.
     */
    fun nest(list: List<Session>, parentKey: (Session) -> String?, online: (Session) -> Boolean,
        collapsed: (Session) -> Boolean): List<SessionCardEntry> {
        fun top(s: Session) = SessionCardEntry(s, fromParent = parent(s.raw), fromParentKey = parent(s.raw)?.let { parentKey(s) })
        val shown = list.mapTo(HashSet()) { it.target.key }
        val parentOf = HashMap<String, String>()
        for (s in list) parentKey(s)?.takeIf { it != s.target.key && it in shown }?.let { parentOf[s.target.key] = it }
        if (parentOf.isEmpty()) return list.map(::top)
        val children = LinkedHashMap<String, MutableList<Session>>()
        for (s in list) parentOf[s.target.key]?.let { children.getOrPut(it) { mutableListOf() } += s }
        data class Node(val s: Session, val depth: Int, val parent: String?, val last: Boolean)
        val nodes = mutableListOf<Node>()
        val seen = HashSet<String>()
        fun walk(root: Session) {
            val stack = ArrayDeque(listOf(Node(root, 0, null, true)))
            while (stack.isNotEmpty()) {
                val node = stack.removeLast()
                if (!seen.add(node.s.target.key)) continue
                nodes += node
                if (node.depth >= MAX_DEPTH) continue
                val kids = children[node.s.target.key].orEmpty().filter { it.target.key !in seen }
                for (i in kids.indices.reversed()) stack.addLast(Node(kids[i], node.depth + 1, node.s.target.key, i == kids.lastIndex))
            }
        }
        // Cards below a session without a shown parent; the rest sit in a loop.
        val rooted = HashSet<String>()
        for (s in list) {
            if (s.target.key in parentOf) continue
            val stack = ArrayDeque(listOf(s))
            while (stack.isNotEmpty()) {
                val at = stack.removeLast()
                if (rooted.add(at.target.key)) stack.addAll(children[at.target.key].orEmpty())
            }
        }
        // In order: a real root, or the first card of a loop, starts a walk.
        for (s in list) if (s.target.key !in seen && (s.target.key !in parentOf || s.target.key !in rooted)) walk(s)
        // Below the depth bound a card starts at top level again; nothing is hidden.
        for (s in list) if (s.target.key !in seen) walk(s)
        // Each parent's descendants, summed once from the leaves up.
        val teams = HashMap<String, SessionTeam>()
        for (i in nodes.indices.reversed()) {
            val (s, _, parent) = nodes[i]
            if (parent == null) continue
            val below = teams[s.target.key] ?: SessionTeam()
            val sum = teams[parent] ?: SessionTeam()
            val up = online(s)
            val archived = SessionFilters.archived(s)
            val kind = if (up && !archived) nativeKind(s, up) else DesktopBadgeKind.Neutral
            val quiet = quiet(s.raw)
            // A quiet request waits for the parent; any other one needs the person (spec 2.3).
            val asks = kind == DesktopBadgeKind.Attention && !s.raw.optBoolean("review_later") && !s.raw.optBoolean("mobile_review_later")
            val needsYou = !SessionNotificationPreferences.muted(s) && !quiet && (asks || SessionFilters.needsAttention(s, up))
            teams[parent] = SessionTeam(sum.total + 1 + below.total,
                sum.working + (if (SessionFilters.working(s, up)) 1 else 0) + below.working,
                sum.asking + (if (asks && quiet) 1 else 0) + below.asking,
                sum.needsYou + (if (needsYou) 1 else 0) + below.needsYou,
                sum.errors + (if (kind == DesktopBadgeKind.Error) 1 else 0) + below.errors)
        }
        val hidden = HashSet<String>()
        return nodes.mapNotNull { (s, depth, parent, last) ->
            // Hidden with a collapsed parent; never promoted to top level.
            if (parent != null && parent in hidden) { hidden += s.target.key; return@mapNotNull null }
            val team = teams[s.target.key]
            if (team != null && collapsed(s)) hidden += s.target.key
            if (parent == null) top(s).copy(team = team) else SessionCardEntry(s, depth, parent, last, team)
        }
    }

    /** The badge kind before muting: Attention for a request, Error for an error (DesktopSessionPalette). */
    private fun nativeKind(session: Session, online: Boolean) = DesktopSessionPalette.nativeKind(session, online)

    /** A parent's durable ID: its computer's machine UUID and its session ID, or null (desktop teamCollapse.ts teamId). */
    fun teamId(machine: String?, session: Session): String? {
        val id = session.raw.string("session_id")
        return if (machine != null && UUID.matches(machine) && UUID.matches(id)) "$machine\n$id" else null
    }

    /** The stored collapsed parents, oldest first; malformed entries are dropped. */
    fun parseTeams(raw: JSONArray?): List<String> {
        if (raw == null) return emptyList()
        val ids = (0 until raw.length()).mapNotNull { raw.opt(it) as? String }.filter { id ->
            id.split('\n').let { it.size == 2 && UUID.matches(it[0]) && UUID.matches(it[1]) }
        }
        return ids.distinct().takeLast(MAX_COLLAPSED_TEAMS)
    }

    /** The list after one choice: the newest last, the oldest dropped first. Expanded is the default and is not stored. */
    fun withTeam(list: List<String>, id: String, collapsed: Boolean): List<String> =
        (list.filter { it != id } + listOfNotNull(id.takeIf { collapsed })).takeLast(MAX_COLLAPSED_TEAMS)

    /** The desktop entity palette (dark skin), indexed by a label's slot. */
    private val palette = listOf(0xFF5FBFA5, 0xFF8EA9F4, 0xFFC098E6, 0xFFE7AE66, 0xFFE38F9E, 0xFF75BFCF, 0xFFACC577, 0xFF8D9BAA)

    /** A stable palette slot (0-7) for a label: the desktop's 31-hash over UTF-16 units (accountMarks.ts labelSlot). */
    fun labelSlot(label: String): Int {
        var hash = 0
        for (c in label) hash = hash * 31 + c.code
        return kotlin.math.abs(hash % 8)
    }

    private val numbers = setOf(Character.DECIMAL_DIGIT_NUMBER, Character.LETTER_NUMBER, Character.OTHER_NUMBER).map { it.toInt() }
    /** A Unicode letter or number (`\p{L}` or `\p{N}`), as the desktop's mark regex: circled digits, fractions and Roman numerals too. */
    private fun letterOrNumber(codePoint: Int) = Character.isLetter(codePoint) || Character.getType(codePoint) in numbers

    /** The first grapheme of the label holding a letter or number, uppercased; the provider's initial when there is none. */
    fun markLetter(label: String, provider: String = "", locale: Locale = Locale.getDefault()): String {
        val text = label.trim()
        val graphemes = BreakIterator.getCharacterInstance(locale).apply { setText(text) }
        var start = graphemes.first()
        var end = graphemes.next()
        while (end != BreakIterator.DONE) {
            val grapheme = text.substring(start, end)
            if (grapheme.codePoints().anyMatch(::letterOrNumber)) return grapheme.uppercase(locale)
            start = end; end = graphemes.next()
        }
        return provider.take(1).uppercase(locale).ifEmpty { "?" }
    }

    private fun luminance(color: Long): Double {
        fun channel(shift: Int) = ((color shr shift) and 0xFF) / 255.0
        fun linear(c: Double) = if (c <= 0.03928) c / 12.92 else Math.pow((c + 0.055) / 1.055, 2.4)
        return 0.2126 * linear(channel(16)) + 0.7152 * linear(channel(8)) + 0.0722 * linear(channel(0))
    }
    /** The ink with the stronger WCAG contrast on the color (accountMarks.ts inkFor). */
    fun inkFor(color: Long): Long {
        val l = luminance(color)
        fun ratio(ink: Long) = luminance(ink).let { (maxOf(it, l) + 0.05) / (minOf(it, l) + 0.05) }
        return if (ratio(0xFF10151B) >= ratio(0xFFFFFFFF)) 0xFF10151B else 0xFFFFFFFF
    }

    /** The provider whose account the row recorded: its agent, with DeepSeek's profiles under dsh. */
    private fun accountProvider(session: Session) =
        session.raw.string("account_provider").ifEmpty { session.raw.string("cmd").ifEmpty { session.agent }.let { if (it == "deepseek") "dsh" else it } }

    /**
     * The row's account mark: its computer and recorded `account_id` against
     * that computer's account catalog, labeled by the grouped account. A row
     * without a recorded binding, or whose profile is not in the loaded
     * catalog, shows no mark rather than a guess (spec 1.6).
     */
    fun accountMark(session: Session, accounts: List<GroupedAccount>): AccountMark? {
        val id = session.raw.opt("account_id") as? String ?: return null
        if (id.isEmpty()) return null
        val provider = accountProvider(session)
        val machine = SessionFilters.machineKey(session.target)
        val group = accounts.firstOrNull { group -> group.members.any {
            it.catalog.key == machine && it.account.provider == provider && it.account.id == id } } ?: return null
        val label = group.label.ifBlank { group.snapshot.account.identity.email }.ifBlank { provider }
        val color = hostColor(session.raw) ?: palette[labelSlot(label)]
        return AccountMark(label, markLetter(label, provider), color, inkFor(color), provider)
    }

    /** The color the row's computer keeps for its account (`account_mark.color`, `#rrggbb`), if any. */
    fun hostColor(raw: JSONObject): Long? = (raw.optJSONObject("account_mark")?.opt("color") as? String)
        ?.takeIf { Regex("^#[0-9a-f]{6}$").matches(it) }?.substring(1)?.toLong(16)?.or(0xFF000000)
}
