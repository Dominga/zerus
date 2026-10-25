package app.zerus.mobile

import android.app.Application
import android.content.Context

/** Non-sensitive switches intentionally avoid encrypted store/Keystore startup dependencies. */
class AndroidTelemetryPreferences(context: Context) : TelemetryPreferenceStore {
    private val preferences = context.getSharedPreferences("zerus_telemetry", Context.MODE_PRIVATE)
    override fun read() = TelemetryPreferences(preferences.getBoolean("analytics", true), preferences.getBoolean("crashes", true))
    override fun write(value: TelemetryPreferences) {
        val edit = preferences.edit().putBoolean("analytics", value.analytics).putBoolean("crashes", value.crashes)
        // The OFF preference and discard boundary are one durable transaction.
        if (!value.crashes) edit.putBoolean("discard_crash_reports", true)
        check(edit.commit()) { "Could not save telemetry settings." }
    }
}
object AppTelemetry {
    private var backend: TelemetryBackend? = null
    private var controller: TelemetryController? = null
    fun initialize(context: Context, preferences: AndroidTelemetryPreferences) {
        if (!BuildConfig.FIREBASE_ENABLED) return
        val loaded = Class.forName("app.zerus.mobile.OptionalFirebase").getMethod("create", Context::class.java, TelemetryPreferences::class.java)
            .invoke(null, context, preferences.read()) as TelemetryBackend
        backend = loaded
        controller = TelemetryController(preferences, loaded).also { it.initialize() }
    }
    fun effective() = controller?.effective
    fun analytics(enabled: Boolean) { checkNotNull(controller).analytics(enabled) }
    fun crashes(enabled: Boolean) { checkNotNull(controller).crashes(enabled) }
    fun opened() { backend?.opened() }
    fun screen(value: TelemetryScreen) { backend?.screen(value) }
    fun failure(value: DiagnosticFailure) { backend?.failure(value) }
    fun registerPush(context: Context) {
        if (BuildConfig.FIREBASE_ENABLED) Class.forName("app.zerus.mobile.OptionalFirebase")
            .getMethod("start", Context::class.java).invoke(null, context)
    }
    fun encryptedWorkspaceReady(context:Context,connection:Connection) {
        if (BuildConfig.FIREBASE_ENABLED && connection.protocolVersion==2) Class.forName("app.zerus.mobile.OptionalFirebase")
            .getMethod("ready", Context::class.java, Connection::class.java).invoke(null,context,connection)
    }
}
class ZerusApplication : Application() {
    private lateinit var telemetry: AndroidTelemetryPreferences
    override fun attachBaseContext(base: Context) {
        super.attachBaseContext(base)
        telemetry = AndroidTelemetryPreferences(base)
        // Read before any provider. FirebaseInitProvider is removed in Firebase builds.
        telemetry.read()
    }
    override fun onCreate() {
        super.onCreate()
        AppTelemetry.initialize(this, telemetry)
        // WorkManager's provider is now ready; this also handles service/worker-only starts.
        AppTelemetry.registerPush(this)
    }
}
