package kr.devfive.hanbeon

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

class TransportProtocolTest {
    @Test
    fun fragmentedRecordsProduceOneNormalizedPair() {
        val parser = InputRecordParser()

        assertEquals(emptyList<InputEdge>(), parser.feed("P".toByteArray()))
        assertEquals(listOf(InputEdge.PRESS), parser.feed("\nR".toByteArray()))
        assertEquals(listOf(InputEdge.RELEASE), parser.feed("\n".toByteArray()))
    }

    @Test
    fun coalescedRecordsAndCrLfProduceNormalizedEdges() {
        val parser = InputRecordParser()

        assertEquals(
            listOf(InputEdge.PRESS, InputEdge.RELEASE),
            parser.feed("P\r\nR\r\n".toByteArray()),
        )
    }

    @Test
    fun malformedAndOverlongRecordsAreDiscardedWithoutPoisoningNextRecord() {
        val parser = InputRecordParser(maxRecordBytes = 3)

        assertEquals(
            emptyList<InputEdge>(),
            parser.feed(
                byteArrayOf(
                    'X'.code.toByte(),
                    '\n'.code.toByte(),
                    0x80.toByte(),
                    '\n'.code.toByte(),
                ),
            ),
        )
        assertEquals(emptyList<InputEdge>(), parser.feed("PPPP\n".toByteArray()))
        assertEquals(
            listOf(InputEdge.PRESS, InputEdge.RELEASE),
            parser.feed("P\nR\n".toByteArray()),
        )
    }

    @Test
    fun bareCarriageReturnAndMalformedCrLfAreRejected() {
        val parser = InputRecordParser()

        assertEquals(emptyList<InputEdge>(), parser.feed("P\rR\n\r\n".toByteArray()))
        assertEquals(listOf(InputEdge.PRESS), parser.feed("P\r\n".toByteArray()))
    }
}

class TransportArbiterTest {
    @Test
    fun duplicateEdgesReachSinkOnlyOncePerSameSessionPair() {
        val sink = RecordingSink()
        val arbiter = TransportArbiter(sink)
        val usb = TransportSession(TransportSource.USB, 1)

        arbiter.ready(usb)
        arbiter.edge(usb, InputEdge.PRESS)
        arbiter.edge(usb, InputEdge.PRESS)
        arbiter.edge(usb, InputEdge.RELEASE)
        arbiter.edge(usb, InputEdge.RELEASE)

        assertEquals(listOf(InputEdge.PRESS, InputEdge.RELEASE), sink.edges)
    }

    @Test
    fun staleSessionCallbacksCannotReachSinkOrChangeCurrentState() {
        val sink = RecordingSink()
        val arbiter = TransportArbiter(sink)
        val oldUsb = TransportSession(TransportSource.USB, 1)
        val currentUsb = TransportSession(TransportSource.USB, 2)

        arbiter.ready(oldUsb)
        arbiter.ready(currentUsb)
        arbiter.edge(oldUsb, InputEdge.PRESS)
        arbiter.lost(oldUsb)
        arbiter.edge(currentUsb, InputEdge.PRESS)
        arbiter.edge(currentUsb, InputEdge.RELEASE)

        assertEquals(listOf(InputEdge.PRESS, InputEdge.RELEASE), sink.edges)
        assertEquals(currentUsb, arbiter.activeSession)
    }

    @Test
    fun selectedSourceRoutesWhileStandbySourceCannotCreateCrossSourcePair() {
        val sink = RecordingSink()
        val arbiter = TransportArbiter(sink)
        val usb = TransportSession(TransportSource.USB, 1)
        val ble = TransportSession(TransportSource.BLE, 1)

        arbiter.ready(usb)
        arbiter.ready(ble)
        arbiter.edge(ble, InputEdge.PRESS)
        arbiter.edge(usb, InputEdge.PRESS)
        arbiter.edge(ble, InputEdge.RELEASE)
        arbiter.edge(usb, InputEdge.RELEASE)

        assertEquals(usb, arbiter.activeSession)
        assertEquals(listOf(InputEdge.PRESS, InputEdge.RELEASE), sink.edges)
        assertTrue(arbiter.select(ble))
    }

    @Test
    fun releasedStandbyCanBeSelectedAndPreviouslySelectedSourceBecomesStandby() {
        val sink = RecordingSink()
        val arbiter = TransportArbiter(sink)
        val usb = TransportSession(TransportSource.USB, 1)
        val ble = TransportSession(TransportSource.BLE, 1)

        arbiter.ready(usb)
        arbiter.ready(ble)
        assertTrue(arbiter.select(ble))
        arbiter.edge(usb, InputEdge.PRESS)
        arbiter.edge(ble, InputEdge.PRESS)
        arbiter.edge(ble, InputEdge.RELEASE)

        assertEquals(ble, arbiter.activeSession)
        assertEquals(listOf(InputEdge.PRESS, InputEdge.RELEASE), sink.edges)
    }

    @Test
    fun losingActiveSessionLeavesNoActiveSource() {
        val arbiter = TransportArbiter(RecordingSink())
        val usb = TransportSession(TransportSource.USB, 1)

        arbiter.ready(usb)
        arbiter.lost(usb)

        assertNull(arbiter.activeSession)
    }

    @Test
    fun selectedSourceCannotChangeWhileItIsPressed() {
        val sink = RecordingSink()
        val arbiter = TransportArbiter(sink)
        val usb = TransportSession(TransportSource.USB, 1)
        val ble = TransportSession(TransportSource.BLE, 1)

        arbiter.ready(usb)
        arbiter.ready(ble)
        arbiter.edge(usb, InputEdge.PRESS)
        assertFalse(arbiter.select(ble))
        arbiter.edge(ble, InputEdge.PRESS)
        arbiter.edge(ble, InputEdge.RELEASE)
        arbiter.edge(usb, InputEdge.RELEASE)

        assertEquals(usb, arbiter.activeSession)
        assertEquals(listOf(InputEdge.PRESS, InputEdge.RELEASE), sink.edges)
    }

    @Test
    fun releaseFromReplacementSessionCannotCompleteOldSessionPress() {
        val sink = RecordingSink()
        val arbiter = TransportArbiter(sink)
        val oldUsb = TransportSession(TransportSource.USB, 1)
        val replacementUsb = TransportSession(TransportSource.USB, 2)

        arbiter.ready(oldUsb)
        arbiter.edge(oldUsb, InputEdge.PRESS)
        arbiter.ready(replacementUsb)
        arbiter.edge(replacementUsb, InputEdge.RELEASE)
        arbiter.edge(oldUsb, InputEdge.RELEASE)

        assertEquals(listOf(InputEdge.PRESS, InputEdge.RELEASE), sink.edges)
        assertEquals(replacementUsb, arbiter.activeSession)
    }
}

class TransportSeamTest {
    @Test
    fun sessionIdsAreMonotonicAcrossSources() {
        val ids = TransportSessionIds()

        assertEquals(TransportSession(TransportSource.USB, 1), ids.next(TransportSource.USB))
        assertEquals(TransportSession(TransportSource.BLE, 2), ids.next(TransportSource.BLE))
    }

    @Test
    fun transportAndTimeDependenciesAreReplaceableWithoutAndroidRuntime() {
        val clock = FakeClock(41)
        val scheduler = ContractTestScheduler()
        val transport = FakeTransport()
        val callback = RecordingTransportCallback()

        transport.start(callback)
        val cancellation = scheduler.schedule(9) { clock.now = 50 }
        assertEquals(41, clock.nowMillis())
        scheduler.runNext()

        assertEquals(50, clock.nowMillis())
        assertSame(callback, transport.callback)
        cancellation.cancel()
        transport.stop()
        assertNull(transport.callback)
    }
}

private class RecordingSink : InputEdgeSink {
    val edges = mutableListOf<InputEdge>()

    override fun accept(edge: InputEdge) {
        edges.add(edge)
    }
}

private class FakeClock(var now: Long) : TransportClock {
    override fun nowMillis(): Long = now
}

private class ContractTestScheduler : TransportScheduler {
    private val tasks = ArrayDeque<() -> Unit>()

    override fun schedule(delayMillis: Long, task: () -> Unit): TransportCancellation {
        require(delayMillis >= 0)
        tasks.addLast(task)
        return TransportCancellation { tasks.remove(task) }
    }

    fun runNext() = tasks.removeFirst().invoke()
}

private class FakeTransport : InputTransport {
    override val source = TransportSource.USB
    var callback: InputTransport.Callback? = null

    override fun start(callback: InputTransport.Callback) {
        this.callback = callback
    }

    override fun stop() {
        callback = null
    }
}

private class RecordingTransportCallback : InputTransport.Callback {
    override fun onState(session: TransportSession, state: TransportState) = Unit

    override fun onEdge(session: TransportSession, edge: InputEdge) = Unit
}
