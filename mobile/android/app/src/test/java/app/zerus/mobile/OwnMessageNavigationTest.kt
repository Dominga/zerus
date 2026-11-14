package app.zerus.mobile

import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.withTimeout
import org.junit.Assert.*
import org.junit.Test

class OwnMessageNavigationTest {
    private val target = Target("computer", "codex/project/chat", "run", "conversation", "workspace")
    private fun event(id: Int, role: String = "AgentMessage") = Event("row:$id", role, "Message $id", "", id.toDouble())
    private fun page(events: List<Event>, older: Boolean = false, newer: Boolean = false, epoch: String = "epoch") =
        OwnMessageNavigation.Page(target, events, epoch, events.firstOrNull()?.id, events.lastOrNull()?.id, older, newer)
    private val previous = OwnMessageNavigation.Direction.Previous
    private val next = OwnMessageNavigation.Direction.Next

    @Test fun followsOnlyThePersonsRowsIncludingPhoneAndAnswers() = runBlocking {
        val events = listOf(event(1,"You"), event(2), event(3,"Tool"), event(4,"You").copy(origin="peer_message"),
            event(5,"You").copy(inheritedFrom="another session"), event(6,"System"),
            event(7,"You").copy(source="hgs_voice"), event(8,"You (answer)"))
        val window = page(events)
        val atEnd = OwnMessageNavigation.Selection()
        assertEquals("row:8", OwnMessageNavigation.candidate(window, atEnd, previous)?.id)
        val selected = OwnMessageNavigation.Selection(cursor="row:7")
        assertEquals("row:1", OwnMessageNavigation.candidate(window, selected, previous)?.id)
        assertEquals("row:8", OwnMessageNavigation.candidate(window, selected, next)?.id)
        assertFalse(OwnMessageNavigation.enabled(window, atEnd, next, true))
        assertTrue(OwnMessageNavigation.enabled(window, atEnd, previous, true))
        assertFalse(OwnMessageNavigation.enabled(window, atEnd, previous, true, busy=true))
        assertFalse(OwnMessageNavigation.enabled(page(emptyList()), atEnd, previous, true))
        assertFalse(OwnMessageNavigation.enabled(window,OwnMessageNavigation.Selection(cursor="row:1"),previous,true))
    }

    @Test fun readsOnePageAtATimeAndStopsAtTheFirstOwnMessage() = runBlocking {
        var window = page((40..49).map { event(it) }, older=true)
        var reads = 0
        val found = OwnMessageNavigation.find(target, OwnMessageNavigation.Selection(), previous, { window }) { direction ->
            assertEquals(previous, direction)
            reads++
            window = if (reads == 1) page((30..39).map { event(it) }, older=true)
                else page(listOf(event(29,"You")) + (30..39).map { event(it) }, older=true)
            true
        }
        assertEquals("row:29", found?.id)
        assertEquals(2, reads)
    }

    @Test fun keepsDirectionWhenBoundedPagesEvictTheCursorAndCanPageForwardAgain() = runBlocking {
        var window = page((400..499).map { event(it,if(it==410) "You" else "AgentMessage") }, older=true, newer=true)
        var reads = 0
        val found = OwnMessageNavigation.find(target, OwnMessageNavigation.Selection(cursor="row:410"), next, { window }) {
            assertEquals(next,it);reads++
            window = page((500..599).map { event(it, if(it==550) "You" else "AgentMessage") }, older=true, newer=true)
            true
        }
        assertEquals("row:550", found?.id);assertEquals(1,reads)
        assertTrue(OwnMessageNavigation.enabled(page(listOf(event(1)),newer=true), OwnMessageNavigation.Selection(),next,true))
        assertFalse(OwnMessageNavigation.enabled(page(listOf(event(1)),newer=true), OwnMessageNavigation.Selection(),next,false))
    }

    @Test fun exhaustionRefusalAndUnchangedPageCannotLoop() = runBlocking {
        for(mode in listOf("end","refused","unchanged")) {
            var window = page(listOf(event(10)),older=true)
            var reads = 0
            assertNull(OwnMessageNavigation.find(target,OwnMessageNavigation.Selection(),previous,{window}) {
                reads++
                assertTrue("$mode repeated the page read",reads <= 1)
                if(mode=="end") window=page(listOf(event(1)))
                mode!="refused"
            })
            assertEquals(mode,1,reads)
        }
    }

    @Test fun anotherTargetOrEpochCannotSupplyTheResult() = runBlocking {
        for(other in listOf(page(listOf(event(1,"You"))).copy(target=target.copy(run="other")),
            page(listOf(event(1,"You")),epoch="different"))) {
            var window=page(listOf(event(10)),older=true)
            assertNull(OwnMessageNavigation.find(target,OwnMessageNavigation.Selection(),previous,{window}) { window=other;true })
        }
    }

    @Test fun cancellingAnInFlightPageCannotStartAnotherReadOrDeliverAResult() = runBlocking {
        val loading=CompletableDeferred<Unit>()
        var reads=0
        val request=async {
            OwnMessageNavigation.find(target,OwnMessageNavigation.Selection(),previous,
                { page(listOf(event(10)),older=true) }) {
                reads++;loading.complete(Unit);awaitCancellation()
            }
        }
        try { withTimeout(2000) { loading.await() } }
        finally { request.cancelAndJoin() }
        assertTrue(request.isCancelled);assertEquals(1,reads)
    }

    @Test fun recentInspectionNeedsAnExactAliasBeforeNavigatingACanonicalPage() = runBlocking {
        var window = page(listOf(event(10)),older=true,epoch="")
        val reading=OwnMessageNavigation.Selection(viewport="row:10",following=false)
        assertNull(OwnMessageNavigation.find(target,reading,previous,{window}) {
            window=page(listOf(event(1,"You")));true
        })
        window=page(listOf(event(10)),older=true,epoch="")
        assertEquals("row:1",OwnMessageNavigation.find(target,reading,previous,{window}) {
            window=page(listOf(event(1,"You"),event(10).copy(id="canonical:10",originalIds=listOf("row:10"))));true
        }?.id)
    }

    @Test fun latestCanEnterCanonicalHistoryWithoutARecentInspectionAlias() = runBlocking {
        var window=page(listOf(event(10)),older=true,epoch="")
        assertEquals("row:1",OwnMessageNavigation.find(target,OwnMessageNavigation.Selection(),previous,{window}) {
            window=page(listOf(event(1,"You")));true
        }?.id)
    }

    @Test fun anEvictedCursorUsesTheCurrentViewportBeforeRequestingAnotherPage() = runBlocking {
        var window=page(listOf(event(100)),older=true)
        val reading=OwnMessageNavigation.Selection(cursor="evicted",viewport="row:100",following=false)
        assertEquals("evicted",OwnMessageNavigation.find(target,reading,previous,{window}) {
            window=page(listOf(event(50,"You"),event(75,"You").copy(id="evicted"),event(100)));true
        }?.id)
    }

    @Test fun newControlsHaveMatchingEnglishAndRussianResourceLabels() {
        val root=generateSequence(java.io.File("").absoluteFile) { it.parentFile }
            .first { java.io.File(it,"mobile/android/app/src/main/res/values/strings.xml").isFile }
        fun labels(directory:String):Map<String,String> {
            val doc=javax.xml.parsers.DocumentBuilderFactory.newInstance().newDocumentBuilder()
                .parse(java.io.File(root,"mobile/android/app/src/main/res/$directory/strings.xml"))
            val rows=doc.getElementsByTagName("string")
            return (0 until rows.length).associate { val row=rows.item(it);row.attributes.getNamedItem("name").nodeValue to row.textContent }
        }
        val english=labels("values");val russian=labels("values-ru")
        assertEquals(english.keys,russian.keys)
        assertEquals("Previous message from you",english["activity_previous_own"])
        assertEquals("Next message from you",english["activity_next_own"])
        assertTrue(russian.values.all { it.isNotBlank() && it.any { char -> char in 'А'..'я' } })
    }

    @Test fun viewportAnchorAndCanonicalAliasesKeepThePersonsPlace() {
        val window=page(listOf(event(1,"You"),event(2),event(3,"You")))
        val reading=OwnMessageNavigation.Selection(viewport="row:2",following=false)
        assertEquals("row:1",OwnMessageNavigation.candidate(window,reading,previous)?.id)
        assertEquals("row:3",OwnMessageNavigation.candidate(window,reading,next)?.id)
        val alias=page(listOf(event(1,"You"),event(2,"You").copy(id="canonical",originalIds=listOf("selected")),event(3,"You")))
        assertEquals("row:1",OwnMessageNavigation.candidate(alias,OwnMessageNavigation.Selection(cursor="selected"),previous)?.id)
    }

    @Test fun directionSnapshotStaysStableThroughFlingAndRefreshesAtIdle() {
        val availability=OwnMessageNavigation.Availability()
        val window=page(listOf(event(1,"You"),event(2),event(3,"You")))
        val before=availability.project(window,OwnMessageNavigation.Selection(),true,false,false)
        availability.commit(before)
        assertEquals(OwnMessageNavigation.Directions(true,false),before)
        // An abandoned composition must not replace the last displayed values.
        availability.project(window,OwnMessageNavigation.Selection(cursor="row:1"),true,false,false)
        for(id in listOf("row:2","row:1","row:2","row:1")) {
            val moving=OwnMessageNavigation.Selection(viewport=id,following=false)
            assertEquals(before,availability.project(window,moving,true,true,true))
        }
        val settled=OwnMessageNavigation.Selection(viewport="row:1",following=false)
        assertEquals(OwnMessageNavigation.Directions(false,true),availability.project(window,settled,true,false,false))
        assertEquals(OwnMessageNavigation.Directions(),availability.project(window,settled,true,true,false))
    }
}
