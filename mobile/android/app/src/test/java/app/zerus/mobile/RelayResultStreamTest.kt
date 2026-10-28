package app.zerus.mobile

import kotlinx.coroutines.async
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withContext
import kotlinx.coroutines.Dispatchers
import okhttp3.OkHttpClient
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

class RelayResultStreamTest {
    @Test fun unadvertisedStreamsDoNotOpenASocket() = runBlocking {
        MockWebServer().use { server ->
            server.start();val client=OkHttpClient();val streams=RelayResultStream(client)
            try {
                val connection=Connection("fixture","Fixture",server.url("/").toString().trimEnd('/'),"synthetic",2)
                assertFalse(streams.ready(connection,"retained",50))
                assertEquals(0,server.requestCount)
            } finally { streams.close();client.dispatcher.executorService.shutdown() }
        }
    }
    @Test fun oneSocketMultiplexesDurableIdsAndIgnoresDuplicateOrForeignHints() = runBlocking {
        MockWebServer().use { server ->
            val watches = CountDownLatch(1)
            server.enqueue(MockResponse().withWebSocketUpgrade(object : WebSocketListener() {
                override fun onMessage(socket: WebSocket, text: String) {
                    val ids = JSONObject(text).getJSONArray("request_ids")
                    if (ids.length() != 2) return
                    watches.countDown()
                    for (id in listOf("foreign", "one", "one", "two")) socket.send(
                        JSONObject().put("type", "ready").put("request_id", id).put("state", "completed").toString())
                }
            }))
            server.start()
            val client = OkHttpClient(); val streams = RelayResultStream(client)
            val connection = Connection("fixture", "Fixture", server.url("/").toString().trimEnd('/'), "synthetic", 2)
            streams.configure(connection, true)
            try {
                val one = async { streams.ready(connection, "one", 2000) }
                val two = async { streams.ready(connection, "two", 2000) }
                assertTrue(one.await()); assertTrue(two.await())
                assertTrue(watches.await(1, TimeUnit.SECONDS)); assertEquals(1, server.requestCount)
                val upgrade = server.takeRequest()
                assertEquals("GET", upgrade.method); assertEquals("/v2/stream", upgrade.path)
                assertEquals("Bearer synthetic", upgrade.getHeader("Authorization"))
            } finally { streams.close(); client.connectionPool.evictAll(); client.dispatcher.executorService.shutdown() }
        }
    }

    @Test fun unsupportedOrDisconnectedStreamReturnsToReceiptRecoveryWithoutPosting() = runBlocking {
        for (unsupported in listOf(true, false)) MockWebServer().use { server ->
            server.enqueue(if (unsupported) MockResponse().setResponseCode(404) else
                MockResponse().withWebSocketUpgrade(object : WebSocketListener() {
                    override fun onMessage(socket: WebSocket, text: String) { socket.close(1001, "") }
                }))
            server.start(); val client = OkHttpClient(); val streams = RelayResultStream(client)
            val connection = Connection("fixture", "Fixture", server.url("/").toString().trimEnd('/'), "synthetic", 2)
            streams.configure(connection, true)
            try {
                assertFalse(streams.ready(connection, "same-retained-id", 2000))
                assertFalse(streams.ready(connection, "same-retained-id", 2000)) // bounded reconnect cooldown
                assertEquals(1, server.requestCount); assertEquals("GET", server.takeRequest().method)
            } finally { streams.close(); client.connectionPool.evictAll(); client.dispatcher.executorService.shutdown() }
        }
    }

    @Test fun normalServerRotationAllowsTheNextWatchWithoutNetworkFailureCooldown() = runBlocking {
        MockWebServer().use { server ->
            server.enqueue(MockResponse().withWebSocketUpgrade(object:WebSocketListener() {
                override fun onMessage(socket:WebSocket,text:String) { socket.close(1000,"") }
            }))
            server.enqueue(MockResponse().withWebSocketUpgrade(object:WebSocketListener() {
                override fun onMessage(socket:WebSocket,text:String) {
                    val ids=JSONObject(text).getJSONArray("request_ids")
                    if(ids.length()>0) socket.send(JSONObject().put("type","ready").put("request_id",ids.getString(0)).put("state","completed").toString())
                }
            }))
            server.start();val client=OkHttpClient();val streams=RelayResultStream(client)
            val connection=Connection("fixture","Fixture",server.url("/").toString().trimEnd('/'),"synthetic",2)
            streams.configure(connection,true)
            try {
                assertFalse(streams.ready(connection,"retained",2000))
                assertTrue(streams.ready(connection,"retained",2000))
                assertEquals(2,server.requestCount)
            } finally { streams.close();client.connectionPool.evictAll();client.dispatcher.executorService.shutdown() }
        }
    }
    @Test fun cancellationUnsubscribesOnlyThatWaiterAndNeverResubmits() = runBlocking {
        MockWebServer().use { server ->
            val registered = CountDownLatch(1); val removed = CountDownLatch(1)
            server.enqueue(MockResponse().withWebSocketUpgrade(object : WebSocketListener() {
                override fun onMessage(socket: WebSocket, text: String) {
                    if (JSONObject(text).getJSONArray("request_ids").length() == 1) registered.countDown() else removed.countDown()
                }
            }))
            server.start(); val client = OkHttpClient(); val streams = RelayResultStream(client)
            try {
                val connection = Connection("fixture", "Fixture", server.url("/").toString().trimEnd('/'), "synthetic", 2)
                streams.configure(connection, true)
                val wait = async { streams.ready(connection, "retained", 2000) }
                assertTrue(withContext(Dispatchers.IO) { registered.await(2, TimeUnit.SECONDS) })
                wait.cancelAndJoin()
                assertTrue(withContext(Dispatchers.IO) { removed.await(2, TimeUnit.SECONDS) })
                assertEquals(1, server.requestCount)
            } finally { streams.close(); client.connectionPool.evictAll(); client.dispatcher.executorService.shutdown() }
        }
    }
}
