package app.zerus.mobile

object FcmRegistrationPolicy {
    fun needsRegistration(token: String, registered: String) = token.isNotBlank() && token != registered
    fun retry(status: Int) = status == 408 || status == 429 || status >= 500
    internal fun workName(connection:String,ready:Boolean=false)=
        if(ready) "zerus-fcm-ready:$connection" else "zerus-fcm:$connection"
}

internal enum class FcmRegistrationResult { Complete, Pending, Removed, Unsupported }

/** Shared by the real worker and SDK-free HTTP tests. Pending enrollment is a
 * retry, never an authenticated registration attempt with an unapproved token. */
internal suspend fun RelayApi.registerFcmWhenReady(connection:Connection,token:String,
    isCurrent:()->Boolean,confirm:()->Unit):FcmRegistrationResult {
    if(!isCurrent()) return FcmRegistrationResult.Complete
    if(connection.protocolVersion==2) when(workspacePushReadiness(connection)) {
        WorkspacePushReadiness.Pending -> return FcmRegistrationResult.Pending
        WorkspacePushReadiness.Removed -> return FcmRegistrationResult.Removed
        WorkspacePushReadiness.Active -> Unit
    }
    val providers=capabilities(connection).optJSONArray("push_providers")
    if(providers==null || (0 until providers.length()).none { providers.optString(it)=="fcm" })
        return FcmRegistrationResult.Unsupported
    if(!isCurrent()) return FcmRegistrationResult.Complete
    registerPush(connection,"fcm",token)
    confirm()
    return FcmRegistrationResult.Complete
}
