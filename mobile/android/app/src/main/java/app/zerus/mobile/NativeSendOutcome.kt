package app.zerus.mobile

import org.json.JSONObject

/** Bounded original binding; the composition and attachment bytes stay in the outbox. */
data class NativeSendIntent(val canonical: String, val bindingSha256: String, val rowSha256: String, val compactionId: String? = null) {
    override fun toString() = "NativeSendIntent(retained)"
    fun json() = JSONObject().put("canonical", canonical).put("binding_sha256", bindingSha256).put("row_sha256", rowSha256)
        .also { if(compactionId!=null) it.put("compaction_id",compactionId) }
    fun value(): JSONObject? = runCatching {
        require(canonical.toByteArray(Charsets.UTF_8).size<=4096)
        val v=JSONObject(canonical);NativeSendOutcome.validateBinding(v)
        require(SendReceiptRecovery.encode(v)==canonical && SendReceiptRecovery.hash(canonical)==bindingSha256 && NativeSendOutcome.hex(rowSha256))
        require(compactionId==null || compactionId.isEmpty() || NativeSendOutcome.uuid(compactionId))
        v
    }.getOrNull()
    fun matches(row: OutgoingMessage): Boolean {
        val v=value() ?: return false;val t=row.target
        return t.agentId.isEmpty() && t.archiveId.isEmpty() && rowSha256==OutboxRetention.binding(row) &&
            v.string("message_request_id")==row.requestId && v.string("computer_id")==t.computerId && v.string("session")==t.session &&
            v.string("run_id")==t.run && v.string("conversation_id")==t.conversation
    }
    companion object {
        fun decode(v:JSONObject)=NativeSendIntent(v.getString("canonical"),v.getString("binding_sha256"),v.getString("row_sha256"),if(v.has("compaction_id")) v.getString("compaction_id") else null)
    }
}
data class NativeSendProof(val canonical: String) {
    override fun toString() = "NativeSendProof(retained)"
    fun value(): JSONObject? = runCatching { require(canonical.toByteArray(Charsets.UTF_8).size<=8192);JSONObject(canonical) }.getOrNull()
}

object NativeSendOutcome {
    const val send="send_outcome"
    const val read="send_outcome_receipt"
    val operations=setOf(send,read)
    fun supported(relay:Set<String>?,native:Set<String>?)=relay?.containsAll(operations)==true && native?.containsAll(operations)==true
    internal fun hex(v:String)=v.matches(Regex("[0-9a-f]{64}"))
    internal fun uuid(v:String)=AttachmentPolicy.validId(v) && v!="00000000-0000-0000-0000-000000000000"
    private fun keys(v:JSONObject)=v.keys().asSequence().toSet()
    private fun text(v:JSONObject,key:String)=v.get(key) as? String ?: error("Invalid send outcome")
    private fun identity(v:String,max:Int,empty:Boolean=false)=(empty || v.isNotEmpty()) && v.toByteArray(Charsets.UTF_8).size<=max && v.none { it.isISOControl() }
    internal fun validateBinding(v:JSONObject) {
        require(keys(v)==setOf("schema","message_request_id","computer_id","machine_id","session_id","session","provider","run_id","conversation_id","native_encoding","native_digest"))
        require(v.opt("schema") is Int && v.getInt("schema")==2 && text(v,"provider") in setOf("claude","codex","kimi") && text(v,"native_encoding") in setOf("compact","python_default") && hex(text(v,"native_digest")))
        for(k in listOf("message_request_id","machine_id","session_id")) require(uuid(text(v,k)))
        for((k,max,empty) in listOf(Triple("computer_id",36,false),Triple("session",512,false),Triple("run_id",128,false),Triple("conversation_id",256,true))) require(identity(text(v,k),max,empty))
    }
    fun freeze(row:OutgoingMessage,payload:JSONObject,provider:String,machineId:String,sessionId:String,encoding:String):NativeSendIntent? = runCatching {
        require(!payload.has("agent_id") && text(payload,"request_id")==row.requestId && text(payload,"text")==row.text &&
            text(payload,"expected_run_id")==row.target.run && text(payload,"expected_conversation_id")==row.target.conversation && payload.getJSONArray("attachments").length()==row.attachments.size)
        val binding=JSONObject().put("schema",2).put("message_request_id",row.requestId).put("computer_id",row.target.computerId)
            .put("machine_id",machineId).put("session_id",sessionId).put("session",row.target.session).put("provider",provider)
            .put("run_id",row.target.run).put("conversation_id",row.target.conversation).put("native_encoding",encoding)
            .put("native_digest",SendReceiptRecovery.hash(SendReceiptRecovery.encode(payload,false,encoding=="python_default")))
        validateBinding(binding);val canonical=SendReceiptRecovery.encode(binding)
        NativeSendIntent(canonical,SendReceiptRecovery.hash(canonical),OutboxRetention.binding(row),if(payload.has("expected_compaction_id")) text(payload,"expected_compaction_id") else null).also { require(it.matches(row)) }
    }.getOrNull()
    fun request(id:String,intent:NativeSendIntent,payload:JSONObject?=null):JSONObject {
        val binding=intent.value() ?: error("The saved message details are unavailable. Review this message.")
        require(uuid(id) && if(payload==null) id!=binding.string("message_request_id") else id==binding.string("message_request_id"))
        val wrapper=JSONObject().put("schema",2).put("request_id",id).put("binding",binding).put("binding_sha256",intent.bindingSha256)
        if(payload!=null) {
            require(SendReceiptRecovery.hash(SendReceiptRecovery.encode(payload,false,binding.string("native_encoding")=="python_default"))==binding.string("native_digest")) { "The saved message details are unavailable. Review this message." }
            wrapper.put("payload",payload)
        }
        return wrapper
    }
    fun proof(id:String,intent:NativeSendIntent,result:JSONObject):NativeSendProof? = runCatching {
        require(uuid(id) && intent.value()!=null && result.opt("schema") is Int && result.getInt("schema")==2 && text(result,"request_id")==id &&
            text(result,"binding_sha256")==intent.bindingSha256 && SendReceiptRecovery.encode(result.getJSONObject("binding"))==intent.canonical)
        val base=setOf("schema","request_id","binding","binding_sha256","status","proof")
        when(text(result,"status")) {
            "not_offered" -> require(keys(result)==base+"reason" && text(result,"proof")=="native_input_lock" && identity(text(result,"reason"),4096))
            "submitted" -> require(keys(result)==base+setOf("submitted_at","queued") && text(result,"proof")=="original_receipt" && result.opt("queued") is Boolean &&
                result.opt("submitted_at") is Number && result.getDouble("submitted_at").isFinite() && result.getDouble("submitted_at")>=0)
            else -> error("Unconfirmed send outcome")
        }
        NativeSendProof(result.toString()).also { require(it.value()!=null) }
    }.getOrNull()
    fun fromReceipt(id:String,intent:NativeSendIntent,receipt:JSONObject,lookup:Boolean=false):NativeSendProof? {
        if(receipt.string("request_id")!=id) return null
        val result=receipt.optJSONObject("result") ?: return null
        val proof=proof(id,intent,result) ?: return null
        val expected=if(lookup || result.string("status")=="submitted") "completed" else "failed"
        return proof.takeIf { receipt.string("state")==expected }
    }
    fun confirmed(row:OutgoingMessage):Boolean {
        val intent=row.nativeSendIntent ?: return false;val result=row.nativeSendProof?.value() ?: return false
        return row.status in setOf("submitted","recorded") && intent.matches(row) && result.string("status")=="submitted" && proof(result.string("request_id"),intent,result)!=null
    }
    fun settle(state:MessageState,captured:OutgoingMessage,proof:NativeSendProof,deadlineMs:Long,nowMs:Long):MessageState {
        val intent=captured.nativeSendIntent ?: return state;val result=proof.value() ?: return state
        if(nowMs>=deadlineMs || !intent.matches(captured) || NativeSendOutcome.proof(result.string("request_id"),intent,result)==null) return state
        val index=state.outgoing.indices.singleOrNull { state.outgoing[it].requestId==captured.requestId } ?: return state
        val row=state.outgoing[index]
        if(row.status !in setOf("sending","uncertain") || row.target!=captured.target || row.createdAt!=captured.createdAt || row.nativeSendIntent!=intent || !intent.matches(row)) return state
        val next=if(result.string("status")=="not_offered") row.copy(status="failed",nativeSendProof=proof,error="The computer rejected this message. Review it before trying again.")
            else row.copy(status="submitted",nativeSendProof=proof,submittedAt=result.getDouble("submitted_at"),submittedText=row.text,error="")
        return state.copy(outgoing=state.outgoing.mapIndexed { at,current -> if(at==index) next else current })
    }
    fun settleReceipt(state:MessageState,captured:OutgoingMessage,receipt:JSONObject,deadlineMs:Long,nowMs:Long):MessageState {
        val intent=captured.nativeSendIntent ?: return state
        fromReceipt(captured.requestId,intent,receipt)?.let { return settle(state,captured,it,deadlineMs,nowMs) }
        if(nowMs>=deadlineMs || captured.status !in setOf("sending","uncertain") || state.outgoing.count { it.requestId==captured.requestId }!=1) return state
        return state.copy(outgoing=state.outgoing.map { if(it!=captured) it else it.copy(status="uncertain",error="Delivery is unconfirmed. Check its receipt before sending again.") })
    }
}
