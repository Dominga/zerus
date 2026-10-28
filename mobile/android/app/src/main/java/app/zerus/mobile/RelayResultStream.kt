package app.zerus.mobile

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import okio.ByteString
import org.json.JSONArray
import org.json.JSONObject

/** Readiness hints only. A hint cannot settle a journal, authorize a result or
 * create/replay a command. All payloads still pass the existing receipt path.
 * One socket multiplexes <=16 durable IDs per connection; there is no socket
 * per poll, no unbounded outgoing queue, and no background keepalive service. */
internal class RelayResultStream(private val client: OkHttpClient) {
    private data class Key(val id: String, val url: String, val token: String)
    private class Watch(val ready: CompletableDeferred<Boolean> = CompletableDeferred(), var users: Int = 0)
    private val lock = Any()
    private val sessions = mutableMapOf<Key, Session>()
    private val supported = linkedSetOf<Key>()
    private val cooldown = linkedMapOf<Key, Long>()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private fun key(connection: Connection) = Key(connection.id, ManagedRelay.transport(connection.url), connection.token)

    fun configure(connection: Connection, available: Boolean) = synchronized(lock) {
        val key = key(connection)
        if (available) {
            supported += key
            while (supported.size > 8) {
                val oldest = supported.first(); supported.remove(oldest)
                sessions[oldest]?.let { finish(it, failed = false) }
            }
        } else {
            supported.remove(key)
            sessions[key]?.let { finish(it, failed = false) }
        }
    }
    fun available(connection: Connection): Boolean = synchronized(lock) { key(connection) in supported }

    private inner class Session(val key: Key) : WebSocketListener() {
        val watches = linkedMapOf<String, Watch>()
        var socket: WebSocket? = null
        var connected = false
        var idle: Job? = null

        // Called only under lock, including callbacks and subscription changes.
        fun publish() {
            if (!connected) return
            val text = JSONObject().put("type", "watch")
                .put("request_ids", JSONArray(watches.keys.toList())).toString()
            if (socket?.send(text) != true) finish(this, failed = true)
        }
        override fun onOpen(webSocket: WebSocket, response: Response) = synchronized(lock) {
            if (sessions[key] !== this) { webSocket.cancel(); return@synchronized }
            socket = webSocket; connected = true
            RelayReadTiming.point(RelayReadTiming.Stage.ResultStreamOpen)
            publish()
        }
        override fun onMessage(webSocket: WebSocket, text: String) = synchronized(lock) {
            if (sessions[key] !== this) return@synchronized
            val valid = runCatching {
                require(text.length <= 4096 && text.toByteArray().size <= 4096)
                val value = JSONObject(text)
                require(value.keys().asSequence().toSet() == setOf("type", "request_id", "state") && value.getString("type") == "ready")
                val id = value.getString("request_id")
                require(id.length in 1..128 && value.getString("state") in setOf("completed", "failed", "uncertain", "expired", "cancelled"))
                watches[id]?.ready?.complete(true)
            }.isSuccess
            if (!valid) finish(this, failed = true)
        }
        override fun onMessage(webSocket: WebSocket, bytes: ByteString) = synchronized(lock) { finish(this, failed = true) }
        override fun onClosing(webSocket: WebSocket, code: Int, reason: String) = synchronized(lock) { finish(this, failed = code != 1000) }
        override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) = synchronized(lock) { finish(this, failed = true) }
    }

    private fun finish(session: Session, failed: Boolean) {
        if (sessions[session.key] !== session) return
        sessions.remove(session.key); session.idle?.cancel()
        session.watches.values.forEach { it.ready.complete(false) }
        session.socket?.cancel()
        if (failed) {
            cooldown[session.key] = System.nanoTime() + 30_000_000_000L
            while (cooldown.size > 8) cooldown.remove(cooldown.keys.first())
        }
    }

    fun close(connectionId: String? = null) = synchronized(lock) {
        sessions.values.toList().filter { connectionId == null || it.key.id == connectionId }
            .forEach { finish(it, failed = false) }
        cooldown.keys.removeAll { connectionId == null || it.id == connectionId }
        supported.removeAll { connectionId == null || it.id == connectionId }
    }

    /** On failure/old servers the caller falls back to receipt GETs with the
     * SAME ID. A newly registered watch asks for already-committed results,
     * closing the ready-before-subscribe and reconnect gaps. */
    suspend fun ready(connection: Connection, id: String, timeoutMs: Long): Boolean {
        val key = key(connection)
        val pair = synchronized(lock) {
            if (key !in supported) return false
            if ((cooldown[key] ?: 0) > System.nanoTime()) return false
            val session = sessions[key] ?: run {
                if (sessions.size >= 8) return false
                Session(key).also { next ->
                    sessions[key] = next
                    val request = Request.Builder().url(key.url + "/v2/stream")
                        .header("Authorization", "Bearer ${key.token}").build()
                    next.socket = client.newWebSocket(request, next)
                }
            }
            if (id !in session.watches && session.watches.size >= 16) return false
            session.idle?.cancel(); session.idle = null
            val watch = session.watches.getOrPut(id) { Watch() }; watch.users++
            session.publish()
            session to watch
        }
        val (session, watch) = pair
        try {
            return withTimeoutOrNull(timeoutMs) { watch.ready.await() } ?: false
        } finally {
            synchronized(lock) {
                if (sessions[key] === session) {
                    if (--watch.users == 0) session.watches.remove(id)
                    session.publish()
                    if (session.watches.isEmpty()) session.idle = scope.launch {
                        delay(10_000)
                        synchronized(lock) {
                            if (session.watches.isEmpty()) finish(session, failed = false)
                        }
                    }
                }
            }
        }
    }
}
