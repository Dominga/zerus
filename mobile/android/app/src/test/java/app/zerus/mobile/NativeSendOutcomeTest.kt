package app.zerus.mobile

import org.junit.Assert.*
import org.junit.Test
import org.json.JSONObject
import kotlinx.coroutines.runBlocking
import okhttp3.*
import okhttp3.ResponseBody.Companion.toResponseBody
import okio.Buffer

class NativeSendOutcomeTest {
    private fun vectors()=JSONObject(java.io.File("../../../services/mobile/vectors/send_outcome/outcomes.json").readText()).getJSONArray("vectors").objects()
    private fun fixture(v:JSONObject):Triple<OutgoingMessage,JSONObject,NativeSendIntent> {
        val w=v.getJSONObject("send").getJSONObject("payload");val b=w.getJSONObject("binding");val p=w.getJSONObject("payload")
        val t=Target(b.string("computer_id"),b.string("session"),b.string("run_id"),b.string("conversation_id"),"gateway")
        val files=if(p.getJSONArray("attachments").length()==0) emptyList() else listOf(Attachment("55555555-5555-4555-8555-555555555555","note.txt","text/plain",5,SendReceiptRecovery.hash("hello")))
        val row=OutgoingMessage(b.string("message_request_id"),t,p.string("text"),files,1.0,"uncertain")
        val intent=NativeSendOutcome.freeze(row,p,b.string("provider"),b.string("machine_id"),b.string("session_id"),b.string("native_encoding"))!!
        return Triple(row.copy(nativeSendIntent=intent),p,intent)
    }
    @Test fun originalOnlyOutcomeLookupIsAnEncryptedRead() {
        assertEquals("read", WorkspaceProtocol.operationClass("send_outcome_receipt"))
        assertEquals("mutation", WorkspaceProtocol.operationClass("send_outcome"))
    }
    @Test fun sharedVectorsFreezeBothEncodingsAndAttachmentsWithoutCopyingTheComposition() {
        val all=vectors();assertEquals(4,all.size)
        for(v in all) {
            val (row,p,intent)=fixture(v)
            assertEquals(SendReceiptRecovery.encode(v.getJSONObject("send").getJSONObject("payload")),SendReceiptRecovery.encode(NativeSendOutcome.request(row.requestId,intent,p)))
            assertTrue(intent.canonical.toByteArray().size<=4096);assertFalse(intent.canonical.contains("data_base64"))
            for(k in listOf("not_sent","read_not_sent","read_submitted")) {
                val result=v.getJSONObject(k);assertNotNull(k,NativeSendOutcome.proof(result.string("request_id"),intent,result))
            }
            val restored=MessageCodec.outgoing(MessageCodec.outgoingJson(row))
            assertEquals(intent,restored.nativeSendIntent);assertTrue(intent.matches(restored))
            assertEquals(listOf(row),OutboxRetention.normalize(MessageState(outgoing=listOf(row))).outgoing)
        }
    }
    @Test fun onlyExactClosedNegativeUnblocksWhileKeepingPayloadAndOriginalUuid() {
        for(v in vectors()) {
            val (row,_,intent)=fixture(v);val receipt=JSONObject().put("request_id",row.requestId).put("state","failed").put("result",v.getJSONObject("not_sent"))
            val state=MessageState(outgoing=listOf(row));val settled=NativeSendOutcome.settleReceipt(state,row,receipt,100,99);val kept=settled.outgoing.single()
            assertEquals("failed",kept.status);assertFalse(kept.blocksSending)
            assertEquals(row.requestId,kept.requestId);assertEquals(row.text,kept.text);assertEquals(row.attachments,kept.attachments)
            assertEquals(intent,kept.nativeSendIntent);assertNotNull(kept.nativeSendProof)
            val draft=Draft(row.target,row.text,attachments=row.attachments)
            assertThrows(IllegalArgumentException::class.java) { settled.enqueue(draft,row.requestId,2.0) }
            val next=settled.enqueue(draft,"66666666-6666-4666-8666-666666666666",2.0)
            assertEquals(2,next.outgoing.size);assertEquals(kept,next.outgoing[0])
            assertEquals(state,NativeSendOutcome.settleReceipt(state,row,receipt,99,99))
            val positive=v.getJSONObject("read_submitted");val proof=NativeSendOutcome.proof(positive.string("request_id"),intent,positive)!!
            val sent=NativeSendOutcome.settle(state,row,proof,100,99).outgoing.single()
            assertTrue(NativeSendOutcome.confirmed(sent));assertEquals("submitted",sent.recover().status)
        }
    }
    @Test fun corruptMissingForeignAndLegacyNegativesStayUnknownWithTheSameUuid() {
        val v=vectors().first();val (row,_,intent)=fixture(v);val good=v.getJSONObject("not_sent")
        for(k in listOf("schema","request_id","binding","binding_sha256","status","proof","reason")) assertNull(k,NativeSendOutcome.proof(row.requestId,intent,JSONObject(good.toString()).put(k,if(k=="reason") JSONObject.NULL else "changed")))
        for(k in intent.value()!!.keys().asSequence().toList()) {
            val binding=intent.value()!!.put(k,"foreign");val bad=JSONObject(good.toString()).put("binding",binding).put("binding_sha256",SendReceiptRecovery.hash(SendReceiptRecovery.encode(binding)))
            assertNull(k,NativeSendOutcome.proof(row.requestId,intent,bad))
        }
        val state=MessageState(outgoing=listOf(row))
        for(result in listOf(JSONObject.NULL,JSONObject().put("status","refused_before_input"),JSONObject().put("status","not_offered").put("reason","not offered"))) {
            val receipt=JSONObject().put("request_id",row.requestId).put("state","failed").put("result",result)
            val kept=NativeSendOutcome.settleReceipt(state,row,receipt,100,99).outgoing.single()
            assertEquals("uncertain",kept.status);assertTrue(kept.blocksSending);assertEquals(row.requestId,kept.requestId)
        }
        val proof=NativeSendOutcome.proof(row.requestId,intent,good)!!
        val changed=MessageState(outgoing=listOf(row.copy(text=row.text+"changed")))
        assertEquals(changed,NativeSendOutcome.settle(changed,row,proof,100,99))
        assertFalse(NativeSendOutcome.supported(setOf(NativeSendOutcome.send),NativeSendOutcome.operations))
        assertThrows(IllegalArgumentException::class.java) { NativeSendOutcome.request(row.requestId,intent) }
    }
    @Test fun lostOrPlainFailedOriginalOnlyQueriesAFreshReadWithTheSavedBinding()=runBlocking {
        val v=vectors().first();val (row,_,intent)=fixture(v);val seen=mutableListOf<Pair<String,JSONObject?>>()
        val client=OkHttpClient.Builder().addInterceptor { chain ->
            val request=chain.request();val body=request.body?.let { val b=Buffer();it.writeTo(b);JSONObject(b.readUtf8()) };seen+=request.method to body
            val reply=if(body==null) JSONObject().put("request_id",row.requestId).put("state","failed").put("error","not offered")
                else JSONObject().put("request_id",body.get("request_id")).put("state","completed").put("result",JSONObject(v.getJSONObject("read_not_sent").toString()).put("request_id",body.get("request_id")))
            Response.Builder().request(request).protocol(Protocol.HTTP_1_1).code(200).message("OK").body(reply.toString().toResponseBody()).build()
        }.build()
        try {
            val api=RelayApi(client);val c=Connection("gateway","Fixture","https://gateway.example.test","synthetic")
            assertEquals(SendReceiptCheck.Unconfirmed,api.checkDelivery(c,row,null,1000));assertEquals(1,seen.size)
            val result=api.checkDelivery(c,row,null,1000,nativeOutcomeSupported=true)
            assertTrue(result is SendReceiptCheck.Outcome)
            assertEquals("not_offered",(result as SendReceiptCheck.Outcome).proof.value()!!.string("status"))
            val post=seen.last().second!!;assertEquals(NativeSendOutcome.read,post.string("operation"));assertNotEquals(row.requestId,post.string("request_id"))
            assertEquals(intent.canonical,SendReceiptRecovery.encode(post.getJSONObject("payload").getJSONObject("binding")))
            assertEquals(1,seen.count { it.first=="POST" })
        } finally { client.connectionPool.evictAll();client.dispatcher.executorService.shutdown() }
    }
    @Test fun theRealOutgoingPostKeepsItsAttemptAndFrozenAttachmentPayload()=runBlocking {
        val v=vectors().last();val (row,p,intent)=fixture(v);val posts=mutableListOf<JSONObject>()
        val client=OkHttpClient.Builder().addInterceptor { chain ->
            val request=chain.request();val buffer=Buffer();request.body!!.writeTo(buffer);posts+=JSONObject(buffer.readUtf8())
            val reply=JSONObject().put("request_id",row.requestId).put("state","failed").put("result",v.getJSONObject("not_sent"))
            Response.Builder().request(request).protocol(Protocol.HTTP_1_1).code(200).message("OK").body(reply.toString().toResponseBody()).build()
        }.build()
        try {
            val api=RelayApi(client);val c=Connection("gateway","Fixture","https://gateway.example.test","synthetic");val attempt=SendAttempt()
            val receipt=submitOutgoing(api,c,row.target,p,row.requestId,attempt,intent)
            assertEquals(TransportStage.Enqueued,attempt.stage);assertEquals(1,posts.size)
            assertEquals(NativeSendOutcome.send,posts.single().string("operation"))
            assertEquals(SendReceiptRecovery.encode(p),SendReceiptRecovery.encode(posts.single().getJSONObject("payload").getJSONObject("payload")))
            assertEquals("failed",NativeSendOutcome.settleReceipt(MessageState(outgoing=listOf(row)),row,receipt,100,99).outgoing.single().status)
            val changed=JSONObject(p.toString()).put("text","different");val refused=SendAttempt()
            assertTrue(runCatching { submitOutgoing(api,c,row.target,changed,row.requestId,refused,intent) }.isFailure)
            assertEquals(TransportStage.Local,refused.stage);assertEquals(1,posts.size)
        } finally { client.connectionPool.evictAll();client.dispatcher.executorService.shutdown() }
    }
    @Test fun aHistoryEchoCannotSettleAnUnknownNegotiatedSend() {
        val (row,_,_)=fixture(vectors().first())
        val event=Event("echo","You",row.text,"",2.0,source="hgs_delivery",requestId=row.requestId)
        val merged=ConversationMerge.merge(row.target,listOf(event),listOf(row))
        assertTrue(merged.recordedRequestIds.isEmpty())
        val state=MessageState(outgoing=listOf(row))
        assertEquals(state,OutboxRetention.recorded(state,setOf(row.requestId),emptyMap()))
        assertTrue(ConversationMerge.merge(row.target,listOf(event),listOf(row.copy(nativeSendIntent=null))).recordedRequestIds.contains(row.requestId))
        assertEquals("uncertain",row.copy(status="submitted").recover().status)
    }
}
