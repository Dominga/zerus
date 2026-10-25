package app.zerus.mobile

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.async
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import okhttp3.mockwebserver.Dispatcher
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import okhttp3.mockwebserver.RecordedRequest
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.util.UUID
import java.util.concurrent.ConcurrentLinkedQueue
import java.util.concurrent.atomic.AtomicInteger

/** Real worker registration path with synthetic public crypto vectors and HTTP;
 * no Firebase SDK, Keystore, phone, FCM delivery or WorkManager runtime. */
class FcmRegistrationTest {
    private class Fixture(pending:Boolean=true,bootstrapOnly:Boolean=false):AutoCloseable {
        val vector=JSONObject(FcmRegistrationTest::class.java.classLoader!!.getResource("encrypted-workspace-v1.json")!!.readText())
        val phone=WorkspaceIdentity.parse(vector.getJSONObject("phone_identity"))
        val gateway=WorkspaceIdentity.parse(vector.getJSONObject("gateway_identity"))
        val roster=WorkspaceMembership.parse(vector.getJSONObject("final_state"))
        val approval=WorkspaceApproval.parse(vector.getJSONObject("approval"))
        val journal=MemoryWorkspaceJournal()
        val server=MockWebServer()
        val posts=AtomicInteger()
        val enrollmentPosts=ConcurrentLinkedQueue<String>()
        val pushBodies=ConcurrentLinkedQueue<String>()
        @Volatile var approved=!pending
        @Volatile var rejectFirstEnrollment=false
        @Volatile var pushStatus=200
        @Volatile var afterCapabilities:(()->Unit)?=null
        @Volatile var current=true
        @Volatile var token="synthetic-fcm-token"
        var binding=""
        var confirmations=0
        val connection:Connection
        val api:RelayApi
        init {
            server.dispatcher=object:Dispatcher() {
                override fun dispatch(request:RecordedRequest):MockResponse {
                    val path=request.path.orEmpty()
                    if(path=="/v2/enrollments" && request.method=="POST") {
                        enrollmentPosts.add(request.body.readUtf8())
                        return MockResponse().setResponseCode(if(rejectFirstEnrollment && enrollmentPosts.size==1) 503 else 202).setBody("{}")
                    }
                    if(path.startsWith("/v2/enrollments/")) return MockResponse().setBody(JSONObject()
                        .put("workspace_id",roster.workspaceId).put("state",if(approved) "active" else "pending")
                        .put("enrollment",approval.json()).put("bootstrap",vector.getJSONObject("bootstrap_snapshot")).toString())
                    if(path.startsWith("/v2/membership")) {
                        val after=request.requestUrl!!.queryParameter("after")!!.toLong()
                        return MockResponse().setBody(JSONObject().put("changes",JSONArray().also {
                            if(after<approval.membership.revision) it.put(approval.membership.json())
                        }).put("membership_revision",roster.revision).put("membership_head",roster.head)
                            .put("next_after",JSONObject.NULL).toString())
                    }
                    if(path.startsWith("/v2/computers")) return MockResponse().setBody("""{"computers":[],"next_after":null}""")
                    if(path=="/v2/capabilities") {
                        afterCapabilities?.invoke()
                        return MockResponse().setBody("""{"push_providers":["fcm"]}""")
                    }
                    if(path=="/v2/push") {
                        posts.incrementAndGet();pushBodies.add(request.body.readUtf8())
                        return MockResponse().setResponseCode(if(approved) pushStatus else 401).setBody("{}")
                    }
                    return MockResponse().setResponseCode(404).setBody("{}")
                }
            }
            server.start()
            connection=Connection(UUID.randomUUID().toString(),"Synthetic workspace",server.url("/").toString().trimEnd('/'),"synthetic-service-token",2)
            val membership=when {
                bootstrapOnly -> WorkspaceMembership.parse(vector.getJSONObject("bootstrap_snapshot").getJSONObject("state"))
                pending -> null
                else -> roster
            }
            journal.create(connection,WorkspaceState(phone,WorkspaceCandidate.parse(vector.getJSONObject("candidate")),gateway.device,0,
                vector.getJSONObject("bootstrap_snapshot").getJSONObject("state").getString("head"),membership,
                enrollment=if(!pending || bootstrapOnly) approval else null))
            api=RelayApi(workspace=journal)
        }
        suspend fun attempt(api:RelayApi=this.api):FcmRegistrationResult {
            val captured=token
            return PushSetupLocks.withConnection(connection.id) { api.registerFcmWhenReady(connection,captured,
                isCurrent={ current && token==captured && FcmRegistrationPolicy.needsRegistration(captured,binding) },
                confirm={ if(current && token==captured) { binding=captured;confirmations++ } }) }
        }
        suspend fun activate() { approved=true;api.computers(connection) }
        fun revoke() {
            val old=journal.load(connection);val membership=requireNotNull(old.membership)
            val unsigned=WorkspaceMembershipChange(membership.workspaceId,membership.revision+1,membership.head,
                gateway.device.deviceId,1,"revoke",null,phone.device.deviceId,"")
            val signed=unsigned.copy(signature=WorkspaceProtocol.encode(WorkspaceCrypto.sign(gateway.keys,unsigned.frame())))
            journal.update(connection,old,old.withMembership(membership.apply(signed)),listOf(signed))
        }
        override fun close() { server.close() }
    }

    @Test fun readyChainBypassesPendingBackoffAndTheRetainedRetryDoesNotDuplicateRegistration()=runBlocking {
        Fixture().use { f -> withTimeout(10000) {
            // Hold the original worker's completion: it can finish (then back
            // off) after the verified catalog, with no later UI refresh.
            val observed=CompletableDeferred<Result<FcmRegistrationResult>>();val finish=CompletableDeferred<Unit>()
            val kept=async {
                val result=runCatching { f.attempt() };observed.complete(result);finish.await();result
            }
            try {
                val outcome=observed.await();val failure=outcome.exceptionOrNull()
                val category=failure?.javaClass?.simpleName ?: "none"
                val status=(failure as? RelayException)?.status
                if(failure!=null) {
                    assertTrue("Fixture precondition: only an observed pending HTTP refusal is admissible; class=$category",failure is RelayException)
                    assertEquals("Fixture precondition: pending /push must return HTTP401",401,status)
                    assertEquals("Fixture precondition: exactly one pending /push must have been observed",1,f.posts.get())
                    assertEquals("Fixture precondition: only capabilities and the refused /push may have run",2,f.server.requestCount)
                }
                assertEquals("Pending enrollment must remain retryable instead of POSTing an unapproved bearer into terminal401 (class=$category,status=$status)",
                    FcmRegistrationResult.Pending,outcome.getOrNull())
                assertEquals("An unapproved bearer must never reach /push",0,f.server.requestCount)
                f.activate()
                assertNotEquals("Ready KEEP must not inherit the token chain's pending backoff",
                    FcmRegistrationPolicy.workName(f.connection.id),FcmRegistrationPolicy.workName(f.connection.id,ready=true))
                assertEquals("The ready chain must run while the old worker is still finishing",
                    FcmRegistrationResult.Complete,f.attempt())
                assertTrue("Ready work must not cancel the original worker",kept.isActive)
                finish.complete(Unit)
                assertEquals("The retained worker must request retry, not terminal401",FcmRegistrationResult.Pending,kept.await().getOrNull())
                // When the original chain eventually retries it observes the
                // binding inside the same lock and performs no second POST.
                assertEquals(FcmRegistrationResult.Complete,f.attempt())
                assertEquals(1,f.posts.get());assertEquals(1,f.confirmations)
                assertEquals("synthetic-fcm-token",JSONObject(f.pushBodies.single()).getString("token"))
            } finally { finish.complete(Unit);kept.cancel() }
        } }
    }

    @Test fun explicitResumeAfterEnrollmentFailureRegistersTheSameSavedToken()=runBlocking {
        Fixture().use { f ->
            f.rejectFirstEnrollment=true
            assertTrue(runCatching { f.api.enrollPrepared(f.connection) }.isFailure)
            assertEquals(FcmRegistrationResult.Pending,f.attempt())
            f.api.enrollPrepared(f.connection)
            assertEquals(2,f.enrollmentPosts.size)
            assertEquals(f.enrollmentPosts.first(),f.enrollmentPosts.last())
            f.activate()
            assertEquals(FcmRegistrationResult.Complete,f.attempt())
            assertEquals(1,f.posts.get());assertEquals("synthetic-fcm-token",f.binding)
        }
    }

    @Test fun approvalBootstrapAloneWaitsForSignedEnrollmentHistory()=runBlocking {
        Fixture(bootstrapOnly=true).use { f ->
            assertEquals(FcmRegistrationResult.Pending,f.attempt())
            assertEquals(0,f.server.requestCount)
            f.activate()
            assertEquals(FcmRegistrationResult.Complete,f.attempt())
            assertEquals(1,f.posts.get())
        }
    }

    @Test fun removedMemberAndUnprovenHistoryCannotPostRegistration()=runBlocking {
        Fixture(pending=false).use { f ->
            val badHistory=object:WorkspaceJournal by f.journal {
                override fun head(connection:Connection,revision:Long):String?=null
            }
            assertTrue(runCatching { f.attempt(RelayApi(workspace=badHistory)) }.isFailure)
            assertEquals(0,f.server.requestCount)
            f.revoke()
            assertEquals(FcmRegistrationResult.Removed,f.attempt())
            assertEquals(0,f.server.requestCount);assertEquals(0,f.confirmations)
        }
    }

    @Test fun verifiedActiveUnauthorizedRemainsTerminal()=runBlocking {
        Fixture(pending=false).use { f ->
            f.pushStatus=401
            val result=runCatching { f.attempt() }.exceptionOrNull()
            assertTrue(result is RelayException)
            assertEquals(401,(result as RelayException).status)
            assertFalse("An active revoked credential must not be resurrected",FcmRegistrationPolicy.retry(result.status))
            assertEquals(1,f.posts.get());assertEquals(0,f.confirmations)
        }
    }

    @Test fun forgetAndTokenRotationDuringSetupCannotRegisterTheStaleBinding()=runBlocking {
        Fixture(pending=false).use { f ->
            f.current=false
            assertEquals(FcmRegistrationResult.Complete,f.attempt());assertEquals(0,f.server.requestCount)
            f.current=true;f.afterCapabilities={ f.current=false }
            assertEquals(FcmRegistrationResult.Complete,f.attempt())
            assertEquals(0,f.posts.get());assertEquals(0,f.confirmations)
            f.current=true;f.afterCapabilities={ f.token="rotated-synthetic-token" }
            assertEquals(FcmRegistrationResult.Complete,f.attempt())
            assertEquals(0,f.posts.get());assertEquals(0,f.confirmations)
            f.afterCapabilities=null
            assertEquals(FcmRegistrationResult.Complete,f.attempt())
            assertEquals(1,f.posts.get());assertEquals("rotated-synthetic-token",f.binding)
        }
    }

    @Test fun repeatedVerifiedCatalogsDoNotReregisterAConfirmedToken()=runBlocking {
        Fixture().use { f ->
            f.activate();assertEquals(FcmRegistrationResult.Complete,f.attempt())
            repeat(2) { f.api.computers(f.connection);assertEquals(FcmRegistrationResult.Complete,f.attempt()) }
            assertEquals(1,f.posts.get());assertEquals(1,f.confirmations)
        }
    }

    @Test fun twoReadyWorkersSerializeAndLegacyRegistrationStillUsesV1()=runBlocking {
        Fixture(pending=false).use { f ->
            val first=async { f.attempt() };val second=async { f.attempt() }
            assertEquals(FcmRegistrationResult.Complete,first.await());assertEquals(FcmRegistrationResult.Complete,second.await())
            assertEquals(1,f.posts.get());assertEquals(1,f.confirmations)
        }
        MockWebServer().use { server ->
            server.enqueue(MockResponse().setBody("""{"push_providers":["fcm"]}"""))
            server.enqueue(MockResponse().setBody("{}"));server.start()
            val connection=Connection("synthetic-legacy","Synthetic legacy",server.url("/").toString().trimEnd('/'),"synthetic-legacy-token")
            var confirmed=false
            assertEquals(FcmRegistrationResult.Complete,RelayApi().registerFcmWhenReady(connection,"synthetic-fcm-token",{ true },{ confirmed=true }))
            assertEquals("/v1/capabilities",server.takeRequest().path);assertEquals("/v1/push",server.takeRequest().path)
            assertTrue(confirmed)
        }
    }
}
