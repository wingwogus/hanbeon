package kr.devfive.hanbeon

import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

class TransportArbiterRuntimeTest {
    private lateinit var usb: FakeSwitch
    private lateinit var ble: FakeSwitch
    private lateinit var core: RecordingCoreInput
    private lateinit var status: TransportStatusHub
    private lateinit var runtime: SwitchTransportRuntime

    @Before
    fun reset() {
        OverlayTransportOwner.resetForTests()
        usb = FakeSwitch(TransportSource.USB)
        ble = FakeSwitch(TransportSource.BLE)
        core = RecordingCoreInput()
        status = TransportStatusHub()
        runtime = SwitchTransportRuntime(usb, ble, core, status)
        runtime.start()
    }

    @Test
    fun bleActiveThenUsbReadyPreemptsOnlyWhenReleased() {
        val bleSession = ble.ready(1)
        assertEquals(TransportSource.BLE, runtime.activeSource)
        assertEquals("ble", status.current().active)

        val usbSession = usb.ready(2)
        assertEquals(TransportSource.USB, runtime.activeSource)
        assertEquals(usbSession, runtime.activeSession)
        assertEquals("usb", status.current().active)
        assertEquals(emptyList<String>(), core.calls)

        ble.edge(bleSession, InputEdge.PRESS)
        ble.edge(bleSession, InputEdge.RELEASE)
        assertEquals(emptyList<String>(), core.calls)
        assertEquals(TransportSource.USB, runtime.activeSource)
    }

    @Test
    fun usbActiveLeavesBleOnStandbyAndIgnoresStandbyPress() {
        val usbSession = usb.ready(1)
        val bleSession = ble.ready(2)

        assertEquals(TransportSource.USB, runtime.activeSource)
        assertEquals(usbSession, runtime.activeSession)

        ble.edge(bleSession, InputEdge.PRESS)
        ble.edge(bleSession, InputEdge.RELEASE)

        assertEquals(emptyList<String>(), core.calls)
        assertEquals(TransportSource.USB, runtime.activeSource)
        assertEquals("ready", status.current().code)
    }

    @Test
    fun crossSourcePressReleaseDoesNotCreateFalsePair() {
        val usbSession = usb.ready(1)
        val bleSession = ble.ready(2)

        ble.edge(bleSession, InputEdge.PRESS)
        usb.edge(usbSession, InputEdge.PRESS)
        ble.edge(bleSession, InputEdge.RELEASE)
        usb.edge(usbSession, InputEdge.RELEASE)

        assertEquals(listOf("pressed", "released"), core.calls)
        assertEquals(TransportSource.USB, runtime.activeSource)
    }

    @Test
    fun standbyCompletePressNeverReachesCore() {
        usb.ready(1)
        val bleSession = ble.ready(2)

        ble.edge(bleSession, InputEdge.PRESS)
        ble.edge(bleSession, InputEdge.RELEASE)

        assertEquals(emptyList<String>(), core.calls)
        assertFalse(status.current().held)
    }

    @Test
    fun usbReadyWhileBleHeldDoesNotPreemptUntilRelease() {
        val bleSession = ble.ready(1)
        ble.edge(bleSession, InputEdge.PRESS)
        assertEquals(listOf("pressed"), core.calls)

        usb.ready(2)
        assertEquals(TransportSource.BLE, runtime.activeSource)
        assertTrue(status.current().held)

        ble.edge(bleSession, InputEdge.RELEASE)
        assertEquals(listOf("pressed", "released"), core.calls)
        assertEquals(TransportSource.USB, runtime.activeSource)
        assertFalse(status.current().held)
    }

    @Test
    fun releasedActiveLossAtomicallySelectsReleasedStandby() {
        usb.ready(1)
        val bleSession = ble.ready(2)
        assertEquals(TransportSource.USB, runtime.activeSource)

        usb.lost(1)
        assertEquals(TransportSource.BLE, runtime.activeSource)
        assertEquals(bleSession, runtime.activeSession)
        assertEquals(emptyList<String>(), core.calls)
        assertFalse(status.current().suspended)
        assertEquals("ble", status.current().active)
    }

    @Test
    fun activeHeldLossCancelsAndSuspendsBeforeStandbyCanResume() {
        val usbSession = usb.ready(1)
        val bleSession = ble.ready(2)
        usb.edge(usbSession, InputEdge.PRESS)
        assertEquals(listOf("pressed"), core.calls)

        usb.lost(1)
        assertEquals(listOf("pressed", "cancelPress", "suspendTransport"), core.calls)
        assertFalse(core.calls.contains("released"))
        assertNull(runtime.activeSource)
        assertFalse(status.current().held)
        assertTrue(status.current().suspended)
        assertEquals("suspended", status.current().code)
        assertEquals("lost", status.current().usb)
        assertEquals("ready", status.current().ble)

        // A standby press/release establishes a new safe released boundary. It
        // cannot turn the interrupted USB press into a Core gesture.
        ble.edge(bleSession, InputEdge.PRESS)
        ble.edge(bleSession, InputEdge.RELEASE)
        assertEquals(
            listOf("pressed", "cancelPress", "suspendTransport", "resumeTransport"),
            core.calls,
        )
        assertEquals(TransportSource.BLE, runtime.activeSource)
    }

    @Test
    fun activeHeldLossWithoutStandbySuspendsAfterCancel() {
        val usbSession = usb.ready(1)
        usb.edge(usbSession, InputEdge.PRESS)

        usb.lost(1)
        assertEquals(listOf("pressed", "cancelPress", "suspendTransport"), core.calls)
        assertNull(runtime.activeSource)
        assertTrue(status.current().suspended)
        assertEquals("suspended", status.current().code)
    }

    @Test
    fun lateOldSessionCallbackIsIgnored() {
        val first = usb.ready(1)
        ble.ready(10)
        usb.lost(1)
        val second = usb.ready(2)

        usb.callback?.onEdge(first, InputEdge.PRESS)
        usb.callback?.onState(first, TransportState.LOST)
        usb.edge(second, InputEdge.PRESS)
        usb.edge(second, InputEdge.RELEASE)

        assertEquals(listOf("pressed", "released"), core.calls)
        assertEquals(second, runtime.activeSession)
        assertNotEquals(first, runtime.activeSession)
        assertEquals(TransportSource.USB, runtime.activeSource)
    }

    @Test
    fun repeatedStartCreatesOneManagerAndOneTransportStart() {
        OverlayTransportOwner.resetForTests()
        val usbOnce = FakeSwitch(TransportSource.USB)
        val bleOnce = FakeSwitch(TransportSource.BLE)
        val coreOnce = RecordingCoreInput()
        val hub = TransportStatusHub()
        var scannerStarts = 0

        val first =
            OverlayTransportOwner.start(
                usb = usbOnce,
                ble = bleOnce,
                core = coreOnce,
                status = hub,
                startScanner = { scannerStarts += 1 },
            )
        val second =
            OverlayTransportOwner.start(
                usb = FakeSwitch(TransportSource.USB),
                ble = FakeSwitch(TransportSource.BLE),
                core = RecordingCoreInput(),
                status = TransportStatusHub(),
                startScanner = { scannerStarts += 1 },
            )

        assertSame(first, second)
        assertEquals(1, scannerStarts)
        assertEquals(1, usbOnce.startCount)
        assertEquals(1, bleOnce.startCount)
        assertEquals(1, OverlayTransportOwner.scannerStarts)
    }

    @Test
    fun stickyStatusSnapshotIsDeliveredToLateListener() {
        usb.ready(1)
        val seen = mutableListOf<TransportStatusSnapshot>()
        status.addListener { seen += it }

        assertEquals(1, seen.size)
        assertEquals(status.current().revision, seen.single().revision)
        assertEquals("usb", seen.single().active)
        assertEquals("ready", seen.single().code)
        assertTrue(seen.single().revision > 0)
    }

    @Test
    fun noUsableInputAndPermissionOutrankReadyCodes() {
        assertEquals("no-input", status.current().code)
        runtime.setHints(
            TransportHints(blePermissionDenied = true),
        )
        assertEquals("permission", status.current().code)

        usb.ready(1)
        assertEquals("ready", status.current().code)
        usb.lost(1)
        runtime.setHints(TransportHints())
        usb.starting(3)
        assertEquals("reconnecting", status.current().code)
    }

    @Test
    fun userPauseAndTransportSuspensionRemainDistinctInStickySnapshot() {
        usb.ready(1)
        runtime.setUserPausedFromCore(true)
        val userPaused = status.current()
        assertEquals("paused", userPaused.code)
        assertTrue(userPaused.paused)
        assertFalse(userPaused.suspended)

        // Core keeps a deliberate user pause paused even if an input source is
        // lost; transport loss must not overwrite that reason.
        usb.lost(1)
        val stillUserPaused = status.current()
        // No usable input outranks the user-paused code, but the sticky flags
        // retain the user-pause reason and must not claim suspension.
        assertEquals("no-input", stillUserPaused.code)
        assertTrue(stillUserPaused.paused)
        assertFalse(stillUserPaused.suspended)
        assertTrue(stillUserPaused.revision > userPaused.revision)

        runtime.setUserPausedFromCore(false)
        usb.ready(2)
        usb.lost(2)
        val transportPaused = status.current()
        assertEquals("suspended", transportPaused.code)
        assertFalse(transportPaused.paused)
        assertTrue(transportPaused.suspended)
        assertTrue(transportPaused.revision > stillUserPaused.revision)
    }

    @Test
    fun neverAdvertisingBleRemainsBoundToReconnectingWithoutCoreEdges() {
        ble.starting(1)

        val snapshot = status.current()
        assertEquals("reconnecting", snapshot.code)
        assertEquals("stopped", snapshot.usb)
        assertEquals("starting", snapshot.ble)
        assertNull(snapshot.active)
        assertEquals(emptyList<String>(), core.calls)
    }

    @Test
    fun permissionHintUpdatePreservesUserPaused() {
        usb.ready(1)
        runtime.setUserPausedFromCore(true)
        assertEquals("paused", status.current().code)
        assertTrue(status.current().paused)
        assertFalse(status.current().suspended)

        runtime.setHints(TransportHints(blePermissionDenied = true))

        val snapshot = status.current()
        assertTrue(snapshot.paused)
        assertFalse(snapshot.suspended)
        assertEquals("paused", snapshot.code)
        assertEquals("usb", snapshot.active)
    }

    @Test
    fun concurrentMutationsDispatchCoreActionsInTransportOrder() {
        OverlayTransportOwner.resetForTests()
        val usbOnce = FakeSwitch(TransportSource.USB)
        val bleOnce = FakeSwitch(TransportSource.BLE)
        val pressEntered = CountDownLatch(1)
        val releasePress = CountDownLatch(1)
        val lossFinished = CountDownLatch(1)
        val coreOnce =
            object : RecordingCoreInput() {
                override fun pressed() {
                    pressEntered.countDown()
                    check(releasePress.await(2, TimeUnit.SECONDS)) { "press was not released" }
                    super.pressed()
                }
            }
        val runtimeOnce = SwitchTransportRuntime(usbOnce, bleOnce, coreOnce, TransportStatusHub())
        runtimeOnce.start()
        val session = usbOnce.ready(1)

        val pressThread = Thread({ usbOnce.edge(session, InputEdge.PRESS) }, "transport-press")
        val lossThread =
            Thread(
                {
                    usbOnce.lost(1)
                    lossFinished.countDown()
                },
                "transport-loss",
            )
        pressThread.start()
        try {
            assertTrue(pressEntered.await(2, TimeUnit.SECONDS))
            lossThread.start()
            assertFalse(
                "later loss must wait behind the earlier Core press",
                lossFinished.await(100, TimeUnit.MILLISECONDS),
            )
        } finally {
            releasePress.countDown()
            pressThread.join(2_000)
            lossThread.join(2_000)
        }

        assertFalse(pressThread.isAlive)
        assertFalse(lossThread.isAlive)
        assertEquals(listOf("pressed", "cancelPress", "suspendTransport"), coreOnce.calls)
    }

    @Test
    fun stopCancelsHeldPressAndSuspendsCore() {
        val session = usb.ready(1)
        usb.edge(session, InputEdge.PRESS)

        runtime.stop()

        assertEquals(listOf("pressed", "cancelPress", "suspendTransport"), core.calls)
    }

    @Test
    fun missingOverlayPermissionPreventsCoreAndTransportStartup() {
        assertFalse(OverlayStartupGate.canStart(canDrawOverlays = false))
        assertTrue(OverlayStartupGate.canStart(canDrawOverlays = true))
    }

    @Test
    fun transportSnapshotFrontendEventUsesLifecycleChannel() {
        val snapshot = TransportStatusSnapshot(revision = 7, code = "ready", active = "usb")
        val event = TransportFrontendEvent.from(snapshot)

        assertEquals("arduino://lifecycle", event.name)
        assertEquals(snapshot.toJson(), event.payload)
    }

    @Test
    fun corePublishDuringHeldLossDoesNotDeadlockWithRuntimeLock() {
        OverlayTransportOwner.resetForTests()
        val usbOnce = FakeSwitch(TransportSource.USB)
        val bleOnce = FakeSwitch(TransportSource.BLE)
        val enteredCore = CountDownLatch(1)
        val releaseCore = CountDownLatch(1)
        val publishFinished = CountDownLatch(1)
        val coreOnce =
            object : RecordingCoreInput() {
                override fun suspendTransport() {
                    super.suspendTransport()
                    OverlayTransportOwner.setUserPausedFromCore(true)
                    enteredCore.countDown()
                    check(releaseCore.await(2, TimeUnit.SECONDS)) {
                        "core was not released"
                    }
                }
            }

        OverlayTransportOwner.start(
            usb = usbOnce,
            ble = bleOnce,
            core = coreOnce,
            startScanner = {},
        )
        val session = usbOnce.ready(1)
        usbOnce.edge(session, InputEdge.PRESS)

        val transportThread = Thread({ usbOnce.lost(1) }, "transport-lost")
        val publishThread =
            Thread(
                {
                    OverlayTransportOwner.setUserPausedFromCore(true)
                    publishFinished.countDown()
                },
                "status-publish",
            )
        transportThread.start()
        try {
            assertTrue(enteredCore.await(2, TimeUnit.SECONDS))
            publishThread.start()
            assertTrue(
                "scan-tick publish deadlocked with Core call under runtime lock",
                publishFinished.await(1, TimeUnit.SECONDS),
            )
        } finally {
            releaseCore.countDown()
            transportThread.join(2_000)
            publishThread.join(2_000)
        }
        assertFalse(transportThread.isAlive)
        assertFalse(publishThread.isAlive)
        assertEquals(listOf("pressed", "cancelPress", "suspendTransport"), coreOnce.calls)
    }
}

private class FakeSwitch(
    override val source: TransportSource,
) : InputTransport {
    var startCount = 0
        private set
    var callback: InputTransport.Callback? = null
        private set

    override fun start(callback: InputTransport.Callback) {
        startCount += 1
        this.callback = callback
    }

    override fun stop() {
        callback = null
    }

    fun starting(id: Long): TransportSession {
        val session = TransportSession(source, id)
        callback?.onState(session, TransportState.STARTING)
        return session
    }

    fun ready(id: Long): TransportSession {
        val session = TransportSession(source, id)
        callback?.onState(session, TransportState.STARTING)
        callback?.onState(session, TransportState.READY)
        return session
    }

    fun lost(id: Long): TransportSession {
        val session = TransportSession(source, id)
        callback?.onState(session, TransportState.LOST)
        return session
    }

    fun edge(session: TransportSession, edge: InputEdge) {
        callback?.onEdge(session, edge)
    }
}

private open class RecordingCoreInput : CoreInput {
    val calls = mutableListOf<String>()

    override fun pressed() {
        calls += "pressed"
    }

    override fun released() {
        calls += "released"
    }

    override fun cancelPress() {
        calls += "cancelPress"
    }

    override fun suspendTransport() {
        calls += "suspendTransport"
    }

    override fun resumeTransport() {
        calls += "resumeTransport"
    }
}
