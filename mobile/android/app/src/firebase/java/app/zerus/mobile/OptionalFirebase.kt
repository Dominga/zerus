package app.zerus.mobile

import android.content.Context
import android.os.Bundle
import androidx.work.*
import com.google.firebase.FirebaseApp
import com.google.firebase.analytics.FirebaseAnalytics
import com.google.firebase.crashlytics.FirebaseCrashlytics
import com.google.firebase.messaging.FirebaseMessaging
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import org.json.JSONObject
import java.util.concurrent.Executor

/** Compiled only with private Firebase project configuration. */
object OptionalFirebase {
    private val tokenLock = Any()
    private var tokenGeneration = 0L
    fun tokenChanged(context: Context, token: String) {
        val generation = synchronized(tokenLock) { ++tokenGeneration }
        CoroutineScope(Dispatchers.IO).launch {
            runCatching { synchronized(tokenLock) {
                if (generation == tokenGeneration) saveTokenAndEnqueue(context,token)
            } }
        }
    }
    @JvmStatic fun create(context: Context, preferences: TelemetryPreferences): TelemetryBackend {
        // Owner OFF is authoritative even if a previous asynchronous SDK disable
        // did not reach disk: never create Analytics during an OFF cold start.
        val analyticsGate = AnalyticsComponentGate(context)
        analyticsGate.reconcile(preferences.analytics, durable = false)
        checkNotNull(FirebaseApp.initializeApp(context))
        val crash = FirebaseCrashlytics.getInstance()
        val previous = checkNotNull(Thread.getDefaultUncaughtExceptionHandler())
        // Let Crashlytics and Android terminate normally; serialize only a sanitized error.
        Thread.setDefaultUncaughtExceptionHandler(SanitizedCrashHandler(previous))
        return FirebaseBackend(context, analyticsGate, crash, preferences.crashes)
    }
    @JvmStatic fun start(context: Context) {
        FirebaseMessaging.getInstance().isAutoInitEnabled = true
        val generation = synchronized(tokenLock) { tokenGeneration }
        FirebaseMessaging.getInstance().token.addOnSuccessListener { token ->
            CoroutineScope(Dispatchers.IO).launch {
                runCatching { synchronized(tokenLock) {
                    if (generation == tokenGeneration) saveTokenAndEnqueue(context,token)
                } }
            }
        }.addOnFailureListener {
            // No Google Play services, or none reachable yet. A confirmed binding keeps working.
            CoroutineScope(Dispatchers.IO).launch {
                runCatching { val store = PrivateStore(context)
                    store.connections().filter { store.firebaseBinding(it.id).isBlank() }.forEach {
                        store.savePushStatusIfCurrent(it, "Firebase push is unavailable on this phone right now. Use the live connection if this persists.")
                    } }
            }
        }
    }
    private fun saveTokenAndEnqueue(context:Context,token:String) {
        val store=PrivateStore(context)
        val policy=if(store.firebaseToken()!=token) ExistingWorkPolicy.REPLACE else ExistingWorkPolicy.KEEP
        store.saveFirebaseToken(token)
        enqueue(context,policy)
    }
    private fun enqueue(context: Context,policy:ExistingWorkPolicy) {
        val store = PrivateStore(context)
        val token = store.firebaseToken()
        if (token.isBlank()) return
        store.connections().filter { FcmRegistrationPolicy.needsRegistration(token, store.firebaseBinding(it.id)) }.forEach { connection ->
            enqueue(context,connection,policy)
        }
    }
    /** A verified catalog repairs registration without inheriting pending work's
     * backoff. Its separate KEEP chain never cancels a registration in flight;
     * both chains serialize and recheck the binding inside PushSetupLocks. */
    @JvmStatic fun ready(context:Context,connection:Connection) {
        CoroutineScope(Dispatchers.IO).launch {
            runCatching {
                val store=PrivateStore(context);val token=store.firebaseToken()
                if(store.connections().contains(connection) &&
                    FcmRegistrationPolicy.needsRegistration(token,store.firebaseBinding(connection.id)))
                    enqueue(context,connection,ExistingWorkPolicy.KEEP,ready=true)
            }
        }
    }
    private fun enqueue(context:Context,connection:Connection,policy:ExistingWorkPolicy,ready:Boolean=false) {
        val request=OneTimeWorkRequestBuilder<FirebaseTokenWorker>()
            .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
            .setInputData(Data.Builder().putString("connection",connection.id).build()).build()
        WorkManager.getInstance(context).enqueueUniqueWork(FcmRegistrationPolicy.workName(connection.id,ready),policy,request)
    }
}
private class FirebaseBackend(private val context: Context,
    private val analyticsGate: AnalyticsComponentGate,
    private val crash: FirebaseCrashlytics, initialCrashes: Boolean) : TelemetryBackend {
    @Volatile private var analytics: FirebaseAnalytics? = null
    private val reportPreferences = context.getSharedPreferences("zerus_telemetry", Context.MODE_PRIVATE)
    private val reportLock = Any()
    @Volatile private var analyticsEnabled = false
    @Volatile private var crashesEnabled = initialCrashes
    @Volatile private var captureEnabled = false
    init {
        // Never automatically upload. Documented manual send/delete chooses one
        // startup action without racing SDK automatic-collection permission.
        crash.setCrashlyticsCollectionEnabled(false)
        val callbackExecutor = Executor { command -> CoroutineScope(Dispatchers.IO).launch { command.run() } }
        crash.checkForUnsentReports().addOnSuccessListener(callbackExecutor) { hasReports -> synchronized(reportLock) {
            val currentEnabled = crashesEnabled && reportPreferences.getBoolean("crashes", true)
            val plan = CrashReportingPolicy.startup(currentEnabled,
                reportPreferences.getBoolean("discard_crash_reports", false), hasReports)
            if (plan.action == PendingCrashAction.Delete) crash.deleteUnsentReports()
            // A failed boundary commit remains denied and must never grant send.
            val saved = reportPreferences.edit().putBoolean("discard_crash_reports", plan.discardPending).commit()
            captureEnabled = saved && plan.capture
            if (saved && plan.action == PendingCrashAction.Send && currentEnabled) crash.sendUnsentReports()
        } }
    }
    override fun analytics(enabled: Boolean) {
        if (enabled) {
            analyticsGate.reconcile(true, durable = true)
            val sdk = analytics ?: FirebaseAnalytics.getInstance(context).also { created ->
                created.setConsent(mapOf(FirebaseAnalytics.ConsentType.AD_STORAGE to FirebaseAnalytics.ConsentStatus.DENIED,
                    FirebaseAnalytics.ConsentType.AD_USER_DATA to FirebaseAnalytics.ConsentStatus.DENIED,
                    FirebaseAnalytics.ConsentType.AD_PERSONALIZATION to FirebaseAnalytics.ConsentStatus.DENIED))
                analytics = created
            }
            sdk.setAnalyticsCollectionEnabled(true)
            analyticsEnabled = true
        } else {
            analyticsEnabled = false
            analytics?.setAnalyticsCollectionEnabled(false)
            analyticsGate.reconcile(false, durable = true)
        }
    }
    override fun crashes(enabled: Boolean) = synchronized(reportLock) {
        if (!enabled) {
            crashesEnabled = false; captureEnabled = false
            crash.setCrashlyticsCollectionEnabled(false)
            // The controller atomically persists OFF plus the discard boundary.
            crash.deleteUnsentReports()
        } else crashesEnabled = true // Upload/capture boundary changes after restart.
    }
    override fun opened() { if (analyticsEnabled) analytics?.logEvent("app_open", null) }
    override fun screen(value: TelemetryScreen) { if (analyticsEnabled) analytics?.logEvent("screen_open", Bundle().apply { putString("screen", value.name.lowercase()) }) }
    override fun failure(value: DiagnosticFailure) { if (crashesEnabled && captureEnabled) crash.recordException(SafeDiagnostics.failure(value)) }
}

class ZerusFirebaseService : FirebaseMessagingService() {
    override fun onNewToken(token: String) {
        OptionalFirebase.tokenChanged(this, token)
    }
    override fun onMessageReceived(message: RemoteMessage) {
        // No payload enters notifications/telemetry. Fetch authoritative events instead.
        PrivateStore(this).connections().forEach { SessionNotifications.wake(this, it.id); SessionNotifications.enqueue(this, it.id) }
    }
}
class FirebaseTokenWorker(context: Context, parameters: WorkerParameters) : CoroutineWorker(context, parameters) {
    override suspend fun doWork(): Result {
        val store = PrivateStore(applicationContext)
        val connection = store.connections().find { it.id == inputData.getString("connection") } ?: return Result.success()
        val token = store.firebaseToken()
        if (!FcmRegistrationPolicy.needsRegistration(token, store.firebaseBinding(connection.id))) return Result.success()
        return PushSetupLocks.withConnection(connection.id) { try {
            val api = RelayApi(workspace=store.workspaceJournal)
            when(api.registerFcmWhenReady(connection,token,
                isCurrent={ store.connections().contains(connection) && store.firebaseToken()==token &&
                    FcmRegistrationPolicy.needsRegistration(token,store.firebaseBinding(connection.id)) },
                confirm={ store.confirmFirebaseBinding(connection,token) })) {
                FcmRegistrationResult.Complete -> Result.success()
                FcmRegistrationResult.Pending -> {
                    store.savePushStatusIfCurrent(connection,"Firebase push setup is waiting for encrypted phone approval.")
                    Result.retry()
                }
                FcmRegistrationResult.Removed -> Result.failure()
                FcmRegistrationResult.Unsupported -> {
                    store.savePushStatusIfCurrent(connection,"Gateway Firebase push is not configured. Use the live connection.")
                    Result.failure()
                }
            }
        } catch (e: CancellationException) { throw e }
        catch (e: RelayException) {
            store.savePushStatusIfCurrent(connection, "Firebase push setup waiting for gateway connection.")
            if (FcmRegistrationPolicy.retry(e.status)) Result.retry() else Result.failure()
        } catch (_: Exception) { AppTelemetry.failure(DiagnosticFailure.PushRegistration); Result.retry() }
        }
    }
}
