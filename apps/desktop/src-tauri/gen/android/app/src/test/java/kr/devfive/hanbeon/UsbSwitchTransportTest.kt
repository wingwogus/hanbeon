package kr.devfive.hanbeon

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class UsbSwitchHandshakeTransportTest {
    @Test
    fun permissionDenialDoesNotReachReadyOrRetry() {
        val host = UsbFakeHost(usbCandidate(), permission = false)
        val scheduler = UsbFakeScheduler()
        val callback = UsbRecordingCallback()
        val transport = UsbSwitchController(host, scheduler, callback, retryPolicy = UsbRetryPolicy(maxRetries = 2))

        transport.start()
        transport.permissionResult(granted = false)

        assertEquals(listOf(TransportState.STARTING, TransportState.LOST), callback.states)
        assertFalse(callback.ready)
        assertEquals(1, host.permissionRequests)
        assertEquals(emptyList<Long>(), scheduler.delays)
    }

    @Test
    fun permissionPendingIntentMustAllowUsbManagerToFillGrantAndDevice() {
        val immutable = 1 shl 25
        val mutable = 1 shl 26
        val updateCurrent = 1 shl 27
        val flags = UsbPermissionIntentPolicy.pendingIntentFlags(
            sdkInt = UsbPermissionIntentPolicy.MUTABLE_FLAG_API,
            immutableFlag = immutable,
            mutableFlag = mutable,
            updateCurrentFlag = updateCurrent,
        )

        assertTrue((flags and mutable) != 0)
        assertTrue((flags and updateCurrent) != 0)
        assertEquals(0, flags and immutable)
        assertTrue(UsbPermissionIntentPolicy.canUsbManagerFillGrantAndDevice(flags, immutable))

        val preMutableFlags = UsbPermissionIntentPolicy.pendingIntentFlags(
            sdkInt = UsbPermissionIntentPolicy.MUTABLE_FLAG_API - 1,
            immutableFlag = immutable,
            mutableFlag = mutable,
            updateCurrentFlag = updateCurrent,
        )
        assertEquals(0, preMutableFlags and immutable)
        assertTrue(
            UsbPermissionIntentPolicy.canUsbManagerFillGrantAndDevice(preMutableFlags, immutable),
        )
    }

    @Test
    fun wrongHelloAndIdentityNeverReachReady() {
        val helloChannel = UsbFakeChannel("HELLO\n")
        val helloCallback = UsbRecordingCallback()
        UsbSwitchController(
            UsbFakeHost(usbCandidate(), channel = helloChannel),
            UsbFakeScheduler(),
            helloCallback,
        ).start()
        assertEquals(listOf(TransportState.STARTING, TransportState.LOST), helloCallback.states)
        assertFalse(helloCallback.ready)
        assertEquals(listOf("HELLO\n"), helloChannel.writes)

        val identityChannel = UsbFakeChannel("NOT_HANBEON\n")
        val identityCallback = UsbRecordingCallback()
        UsbSwitchController(
            UsbFakeHost(usbCandidate(), channel = identityChannel),
            UsbFakeScheduler(),
            identityCallback,
        ).start()
        assertEquals(listOf(TransportState.STARTING, TransportState.LOST), identityCallback.states)
        assertFalse(identityCallback.ready)
        assertEquals(listOf("HELLO\n"), identityChannel.writes)

        val crlfChannel = UsbFakeChannel("HANBEON_UNO_V1\r\n")
        val crlfCallback = UsbRecordingCallback()
        UsbSwitchController(
            UsbFakeHost(usbCandidate(), channel = crlfChannel),
            UsbFakeScheduler(),
            crlfCallback,
        ).start()
        assertEquals(listOf(TransportState.STARTING, TransportState.LOST), crlfCallback.states)
        assertFalse(crlfCallback.ready)
        assertEquals(listOf("HELLO\n"), crlfChannel.writes)

        val v2Channel = UsbFakeChannel("HANBEON_UNO_V2\n")
        val v2Callback = UsbRecordingCallback()
        UsbSwitchController(
            UsbFakeHost(usbCandidate(), channel = v2Channel),
            UsbFakeScheduler(),
            v2Callback,
        ).start()
        assertEquals(listOf(TransportState.STARTING, TransportState.LOST), v2Callback.states)
        assertFalse(v2Callback.ready)
        assertEquals(listOf("HELLO\n"), v2Channel.writes)
    }

    @Test
    fun handshakeRetriesHelloAfterBootTimeoutThenReachesReady() {
        val channel = UsbHelloAfterTimeoutChannel()
        val callback = UsbRecordingCallback()

        UsbSwitchController(UsbFakeHost(usbCandidate(), channel = channel), UsbFakeScheduler(), callback).start()

        assertEquals(listOf(TransportState.STARTING, TransportState.READY), callback.states)
        assertTrue(callback.ready)
        assertEquals(listOf("HELLO\n", "HELLO\n"), channel.writes)
    }

    @Test
    fun nonCdcAcmCandidateIsNotAttempted() {
        val host = UsbFakeHost(
            usbCandidate().copy(cdcAcm = false),
            channel = UsbFakeChannel("HANBEON_UNO_V1\n"),
        )
        val callback = UsbRecordingCallback()

        UsbSwitchController(host, UsbFakeScheduler(), callback).start()

        assertEquals(listOf(TransportState.STARTING, TransportState.LOST), callback.states)
        assertEquals(0, host.permissionRequests)
        assertEquals(0, host.openCount)
        assertFalse(callback.ready)
        assertEquals(emptyList<InputEdge>(), callback.edges)
    }

    @Test
    fun cdcCandidatesAreSelectedDeterministicallyByVendorThenId() {
        val xiao = usbCandidate().copy(
            id = "usb-xiao",
            name = "seeed xiao nrf52840",
            vendorId = XIAO_VENDOR,
            productId = XIAO_PRODUCT,
        )
        val arduino = usbCandidate()
        val opened = mutableListOf<String>()
        val host = object : UsbHost {
            override fun candidates() = listOf(xiao, arduino)
            override fun hasPermission(candidate: UsbCandidate) = true
            override fun requestPermission(candidate: UsbCandidate) = Unit
            override fun open(candidate: UsbCandidate): UsbByteChannel? {
                opened += candidate.id
                return UsbFakeChannel("HANBEON_UNO_V1\n")
            }
        }
        val callback = UsbRecordingCallback()

        UsbSwitchController(host, UsbFakeScheduler(), callback).start()

        assertEquals(listOf("usb-1"), opened)
        assertEquals(listOf(TransportState.STARTING, TransportState.READY), callback.states)
        assertTrue(callback.ready)
    }

    @Test
    fun seeedXiaoCdcCandidateReachesReadyAfterValidHandshake() {
        val candidate = usbCandidate().copy(
            id = "usb-xiao",
            name = "seeed xiao nrf52840",
            vendorId = XIAO_VENDOR,
            productId = XIAO_PRODUCT,
        )
        val channel = UsbFakeChannel("HANBEON_UNO_V1\n")
        val callback = UsbRecordingCallback()
        val host = UsbFakeHost(candidate, channel = channel)

        UsbSwitchController(host, UsbFakeScheduler(), callback).start()

        assertEquals(listOf(TransportState.STARTING, TransportState.READY), callback.states)
        assertTrue(callback.ready)
        assertEquals(1, host.openCount)
        assertEquals(listOf("HELLO\n"), channel.writes)
        assertEquals(emptyList<InputEdge>(), callback.edges)
    }

    @Test
    fun nonArduinoCdcWrongIdentityNeverReachesReadyOrEmitsEdge() {
        val candidate = usbCandidate().copy(
            id = "usb-xiao",
            name = "seeed xiao nrf52840",
            vendorId = XIAO_VENDOR,
            productId = XIAO_PRODUCT,
        )
        val channel = UsbFakeChannel("NOT_HANBEON\n")
        val callback = UsbRecordingCallback()
        val host = UsbFakeHost(candidate, channel = channel)
        val transport = UsbSwitchController(host, UsbFakeScheduler(), callback)

        transport.start()
        transport.readerBytes(TransportSession(TransportSource.USB, 1), "P\nR\n".toByteArray())

        assertEquals(listOf(TransportState.STARTING, TransportState.LOST), callback.states)
        assertFalse(callback.ready)
        assertEquals(emptyList<InputEdge>(), callback.edges)
        assertEquals(1, host.openCount)
        assertEquals(listOf("HELLO\n"), channel.writes)
    }

    @Test
    fun fragmentedExactIdentityReachesReadyAndWritesExactHello() {
        val channel = UsbFakeChannel("HAN", "BEON_UNO_V1\n")
        val callback = UsbRecordingCallback()

        UsbSwitchController(UsbFakeHost(usbCandidate(), channel = channel), UsbFakeScheduler(), callback).start()

        assertEquals(listOf(TransportState.STARTING, TransportState.READY), callback.states)
        assertTrue(callback.ready)
        assertEquals(listOf("HELLO\n"), channel.writes)
    }

    @Test
    fun malformedRecordsAreIgnoredAndValidPairIsNormalized() {
        val callback = UsbRecordingCallback()
        val transport = UsbSwitchController(
            UsbFakeHost(usbCandidate(), channel = UsbFakeChannel("HANBEON_UNO_V1\n")),
            UsbFakeScheduler(),
            callback,
        )
        transport.start()
        val session = callback.session ?: error("USB did not become ready")

        transport.readerBytes(session, "X\nP\rR\nP\nR\n".toByteArray())

        assertEquals(listOf(InputEdge.PRESS, InputEdge.RELEASE), callback.edges)
    }
}

class UsbSwitchLifecycleTransportTest {
    @Test
    fun synchronousReaderLossDoesNotLeaveReturnedCancellationAttached() {
        val callback = UsbRecordingCallback()
        val reader = UsbSynchronousLossReader()
        val transport = UsbSwitchController(
            UsbFakeHost(usbCandidate(), channel = UsbFakeChannel("HANBEON_UNO_V1\n")),
            UsbFakeScheduler(),
            callback,
            retryPolicy = UsbRetryPolicy(maxRetries = 0),
            readerStarter = reader,
        )

        transport.start()

        assertEquals(
            listOf(TransportState.STARTING, TransportState.READY, TransportState.LOST),
            callback.states,
        )
        assertTrue(reader.returnedCancellationWasCancelled)
        assertEquals(emptyList<InputEdge>(), callback.edges)
    }

    @Test
    fun detachWhileHeldReportsLossWithoutSynthesizingRelease() {
        val candidate = usbCandidate()
        val scheduler = UsbFakeScheduler()
        val callback = UsbRecordingCallback()
        val transport = UsbSwitchController(
            UsbFakeHost(candidate, channel = UsbFakeChannel("HANBEON_UNO_V1\n")),
            scheduler,
            callback,
            retryPolicy = UsbRetryPolicy(maxRetries = 1),
        )
        transport.start()
        val session = callback.session ?: error("USB did not become ready")

        transport.readerBytes(session, "P\n".toByteArray())
        transport.detached(candidate.id)

        assertEquals(listOf(InputEdge.PRESS), callback.edges)
        assertEquals(
            listOf(TransportState.STARTING, TransportState.READY, TransportState.LOST),
            callback.states,
        )
        assertEquals(listOf(1_000L), scheduler.delays)
    }

    @Test
    fun staleReaderCallbackCannotMutateReplacementSession() {
        val candidate = usbCandidate()
        val scheduler = UsbFakeScheduler()
        val callback = UsbRecordingCallback()
        val transport = UsbSwitchController(
            UsbFakeHost(
                candidate,
                channels = listOf(
                    UsbFakeChannel("HANBEON_UNO_V1\n"),
                    UsbFakeChannel("HANBEON_UNO_V1\n"),
                ),
            ),
            scheduler,
            callback,
        )
        transport.start()
        val oldSession = callback.session ?: error("first USB session was not ready")
        transport.detached(candidate.id)
        scheduler.runNext()
        val replacement = callback.session ?: error("replacement USB session was not ready")

        transport.readerBytes(oldSession, "P\nR\n".toByteArray())
        transport.readerBytes(replacement, "P\nR\n".toByteArray())

        assertTrue(oldSession.id < replacement.id)
        assertEquals(listOf(InputEdge.PRESS, InputEdge.RELEASE), callback.edges)
        assertEquals(replacement, callback.session)
    }

    @Test
    fun intentionalStopCancelsRetryAndIgnoresLateReaderCallback() {
        val candidate = usbCandidate()
        val host = UsbFakeHost(candidate, channel = UsbFakeChannel("HANBEON_UNO_V1\n"))
        val scheduler = UsbFakeScheduler()
        val callback = UsbRecordingCallback()
        val transport = UsbSwitchController(host, scheduler, callback)
        transport.start()
        val session = callback.session ?: error("USB did not become ready")

        transport.detached(candidate.id)
        transport.stop()
        transport.readerBytes(session, "P\nR\n".toByteArray())
        scheduler.runAll()

        assertEquals(
            listOf(TransportState.STARTING, TransportState.READY, TransportState.LOST, TransportState.STOPPED),
            callback.states,
        )
        assertEquals(emptyList<InputEdge>(), callback.edges)
        assertEquals(1, host.openCount)
    }

    @Test
    fun stopCallbackCanRestartWithoutOldStopErasingReplacementSession() {
        val candidate = usbCandidate()
        val scheduler = UsbFakeScheduler()
        val states = mutableListOf<TransportState>()
        val sessions = mutableListOf<TransportSession>()
        val edges = mutableListOf<InputEdge>()
        var restarted = false
        lateinit var transport: UsbSwitchController
        val callback = object : InputTransport.Callback {
            override fun onState(session: TransportSession, state: TransportState) {
                states += state
                if (state == TransportState.READY) sessions += session
                if (state == TransportState.STOPPED && !restarted) {
                    restarted = true
                    transport.start(this)
                }
            }

            override fun onEdge(session: TransportSession, edge: InputEdge) {
                edges += edge
            }
        }
        transport = UsbSwitchController(
            UsbFakeHost(
                candidate,
                channels = listOf(
                    UsbFakeChannel("HANBEON_UNO_V1\n"),
                    UsbFakeChannel("HANBEON_UNO_V1\n"),
                ),
            ),
            scheduler,
            callback,
        )

        transport.start()
        val first = sessions.single()
        transport.stop()
        val replacement = sessions.last()
        transport.readerBytes(replacement, "P\nR\n".toByteArray())

        assertTrue(first.id < replacement.id)
        assertEquals(listOf(InputEdge.PRESS, InputEdge.RELEASE), edges)
        assertEquals(TransportState.READY, states.last())

        transport.stop()
    }

    @Test
    fun physicalLossRetryIsBounded() {
        val candidate = usbCandidate()
        val host = UsbFakeHost(
            candidate,
            channels = listOf(UsbFakeChannel("HANBEON_UNO_V1\n"), null, null),
        )
        val scheduler = UsbFakeScheduler()
        val callback = UsbRecordingCallback()
        val transport = UsbSwitchController(
            host,
            scheduler,
            callback,
            retryPolicy = UsbRetryPolicy(maxRetries = 2, delayMillis = 25),
        )

        transport.start()
        transport.detached(candidate.id)
        scheduler.runAll()

        assertEquals(listOf(25L, 25L), scheduler.delays)
        assertEquals(3, host.openCount)
        assertEquals(2, scheduler.runCount)
        assertEquals(TransportState.LOST, callback.states.last())
        assertFalse(callback.readyAfterLoss)
    }
}

private const val XIAO_VENDOR = 0x2886
private const val XIAO_PRODUCT = 0x8045

private fun usbCandidate() = UsbCandidate("usb-1", "validated test candidate", 0x2341, 0x0043, true)

private class UsbRecordingCallback : InputTransport.Callback {
    val states = mutableListOf<TransportState>()
    val edges = mutableListOf<InputEdge>()
    var session: TransportSession? = null
    var ready = false
    var readyAfterLoss = false
    private var lost = false

    override fun onState(session: TransportSession, state: TransportState) {
        states += state
        if (state == TransportState.LOST) lost = true
        if (state == TransportState.READY) {
            this.session = session
            ready = true
            readyAfterLoss = readyAfterLoss || lost
        }
    }

    override fun onEdge(session: TransportSession, edge: InputEdge) {
        edges += edge
    }
}

private class UsbFakeHost(
    private val candidate: UsbCandidate,
    private val permission: Boolean = true,
    channel: UsbByteChannel? = null,
    channels: List<UsbByteChannel?>? = null,
) : UsbHost {
    private val opened = (channels ?: listOf(channel)).toMutableList()
    var permissionRequests = 0
    var openCount = 0

    override fun candidates() = listOf(candidate)
    override fun hasPermission(candidate: UsbCandidate) = permission
    override fun requestPermission(candidate: UsbCandidate) { permissionRequests++ }
    override fun open(candidate: UsbCandidate): UsbByteChannel? {
        openCount++
        return if (opened.isEmpty()) null else opened.removeAt(0)
    }
}

private class UsbSynchronousLossReader : UsbReaderStarter {
    var returnedCancellationWasCancelled = false

    override fun start(
        session: TransportSession,
        channel: UsbByteChannel,
        initialBytes: ByteArray,
        onBytes: (TransportSession, ByteArray) -> Unit,
        onLost: (TransportSession) -> Unit,
    ): TransportCancellation {
        onLost(session)
        return TransportCancellation { returnedCancellationWasCancelled = true }
    }
}

private class UsbFakeChannel(vararg responseParts: String) : UsbByteChannel {
    val writes = mutableListOf<String>()
    private val responses = responseParts.map { it.toByteArray(Charsets.US_ASCII) }.toMutableList()

    override fun write(bytes: ByteArray): Int {
        writes += bytes.toString(Charsets.US_ASCII)
        return bytes.size
    }

    override fun read(buffer: ByteArray, offset: Int, length: Int, timeoutMillis: Long): Int {
        if (responses.isEmpty()) return 0
        val response = responses.removeAt(0)
        val count = minOf(length, response.size)
        response.copyInto(buffer, offset, 0, count)
        if (count < response.size) responses.add(0, response.copyOfRange(count, response.size))
        return count
    }

    override fun close() = Unit
}

/** First HELLO is lost during DTR reset; the second write is answered exactly. */
private class UsbHelloAfterTimeoutChannel : UsbByteChannel {
    val writes = mutableListOf<String>()
    private var hellos = 0
    private val pending = ArrayDeque<ByteArray>()

    override fun write(bytes: ByteArray): Int {
        writes += bytes.toString(Charsets.US_ASCII)
        if (++hellos >= 2) pending.add("HANBEON_UNO_V1\n".toByteArray(Charsets.US_ASCII))
        return bytes.size
    }

    override fun read(buffer: ByteArray, offset: Int, length: Int, timeoutMillis: Long): Int {
        val response = pending.removeFirstOrNull() ?: return 0
        val count = minOf(length, response.size)
        response.copyInto(buffer, offset, 0, count)
        if (count < response.size) pending.addFirst(response.copyOfRange(count, response.size))
        return count
    }

    override fun close() = Unit
}

private class UsbFakeScheduler : TransportScheduler {
    private data class Task(val action: () -> Unit, var cancelled: Boolean = false)
    val delays = mutableListOf<Long>()
    var runCount = 0
        private set
    private val tasks = mutableListOf<Task>()

    override fun schedule(delayMillis: Long, task: () -> Unit): TransportCancellation {
        val scheduled = Task(task)
        delays += delayMillis
        tasks += scheduled
        return TransportCancellation { scheduled.cancelled = true }
    }

    fun runNext() {
        val index = tasks.indexOfFirst { !it.cancelled }
        if (index < 0) error("no scheduled task")
        val task = tasks.removeAt(index)
        runCount++
        task.action()
    }

    fun runAll() {
        while (tasks.any { !it.cancelled }) runNext()
    }
}


/**
 * The reader drops the device on a negative read. bulkTransfer returns a
 * negative value for an ordinary idle timeout too, so an untouched switch was
 * read as "device gone" roughly 200ms after it became READY.
 */
class UsbBulkTransferResultTest {
    @Test
    fun idleTimeoutIsNotLoss() {
        assertEquals(
            UsbBulkTransferResult.NO_DATA,
            UsbBulkTransferResult.toChannelRead(transferred = -1, channelClosed = false),
        )
    }

    @Test
    fun closedChannelIsLoss() {
        assertEquals(
            UsbBulkTransferResult.LOST,
            UsbBulkTransferResult.toChannelRead(transferred = -1, channelClosed = true),
        )
    }

    @Test
    fun deliveredBytesArePreserved() {
        assertEquals(7, UsbBulkTransferResult.toChannelRead(transferred = 7, channelClosed = false))
    }

    @Test
    fun bytesWinOverACloseRaceSoNoPressIsDropped() {
        assertEquals(3, UsbBulkTransferResult.toChannelRead(transferred = 3, channelClosed = true))
    }
}

