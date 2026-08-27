package kr.devfive.hanbeon

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.UUID

class BleSwitchTest {
    @Test
    fun missingNusServiceLeavesSourceNonReadyAndEmitsNoEdge() {
        val env = Env()
        env.start()
        env.connectTrusted()
        env.currentGatt().discover(
            listOf(
                BleServiceSnapshot(
                    uuid = UUID.fromString("00001800-0000-1000-8000-00805f9b34fb"),
                    characteristics = emptyList(),
                ),
            ),
        )

        assertFalse(env.callback.becameReady)
        assertEquals(emptyList<InputEdge>(), env.callback.edges)
        assertTrue(env.currentGatt().closed)
    }

    @Test
    fun missingRxCharacteristicLeavesSourceNonReadyAndEmitsNoEdge() {
        val env = Env()
        env.start()
        env.connectTrusted()
        env.currentGatt().discover(nusServices(rx = null))

        assertFalse(env.callback.becameReady)
        assertEquals(emptyList<InputEdge>(), env.callback.edges)
        assertTrue(env.currentGatt().closed)
    }

    @Test
    fun missingTxCharacteristicLeavesSourceNonReadyAndEmitsNoEdge() {
        val env = Env()
        env.start()
        env.connectTrusted()
        env.currentGatt().discover(nusServices(tx = null))

        assertFalse(env.callback.becameReady)
        assertEquals(emptyList<InputEdge>(), env.callback.edges)
        assertTrue(env.currentGatt().closed)
    }

    @Test
    fun missingCccdLeavesSourceNonReadyAndEmitsNoEdge() {
        val env = Env()
        env.start()
        env.connectTrusted()
        env.currentGatt().discover(nusServices(cccd = false))

        assertFalse(env.callback.becameReady)
        assertEquals(emptyList<InputEdge>(), env.callback.edges)
        assertTrue(env.currentGatt().closed)
        assertFalse(env.currentGatt().wroteHello)
    }

    @Test
    fun notificationBeforeHandshakeEmitsNoEdgeAndDoesNotBecomeReady() {
        val env = Env()
        env.start()
        env.connectTrusted()
        env.currentGatt().discover(nusServices())
        env.currentGatt().ackCccd()
        env.currentGatt().notifyTx("P\nR\n".toByteArray())

        assertFalse(env.callback.becameReady)
        assertEquals(emptyList<InputEdge>(), env.callback.edges)
        assertTrue(env.currentGatt().wroteHello)
    }

    @Test
    fun fragmentedRecordsAfterReadyProduceOneNormalizedPair() {
        val env = Env()
        env.becomeReady()

        env.currentGatt().notifyTx("P".toByteArray())
        assertEquals(emptyList<InputEdge>(), env.callback.edges)
        env.currentGatt().notifyTx("\nR".toByteArray())
        assertEquals(listOf(InputEdge.PRESS), env.callback.edges)
        env.currentGatt().notifyTx("\n".toByteArray())
        assertEquals(listOf(InputEdge.PRESS, InputEdge.RELEASE), env.callback.edges)
        assertEquals(1, env.callback.readyCount)
    }

    @Test
    fun staleGattCallbackAfterReconnectCannotEmitEdge() {
        val env = Env()
        env.becomeReady()
        val stale = env.currentGatt()
        stale.disconnect()
        assertTrue(env.scheduler.runNext())
        env.connectTrusted()
        stale.notifyTx("P\nR\n".toByteArray())
        env.completeHandshake()
        env.currentGatt().notifyTx("P\nR\n".toByteArray())

        assertEquals(listOf(InputEdge.PRESS, InputEdge.RELEASE), env.callback.edges)
        assertEquals(env.callback.lastReadySession, env.callback.lastEdgeSession)
        assertFalse(stale === env.currentGatt())
    }

    @Test
    fun disconnectWhileHeldEmitsLostWithoutRelease() {
        val env = Env()
        env.becomeReady()
        env.currentGatt().notifyTx("P\n".toByteArray())
        env.currentGatt().disconnect()

        assertEquals(listOf(InputEdge.PRESS), env.callback.edges)
        assertEquals(TransportState.LOST, env.callback.lastState)
        assertFalse(env.callback.edges.contains(InputEdge.RELEASE))
        assertTrue(env.currentGatt().closed)
    }

    @Test
    fun wrongSavedIdentityNeverConnectsOrEmits() {
        val env = Env()
        env.start()
        env.host.emitScan(OTHER_ADDRESS, "HanBeon XIAO")

        assertTrue(env.host.gattSessions.isEmpty())
        assertFalse(env.callback.becameReady)
        assertEquals(emptyList<InputEdge>(), env.callback.edges)
        assertEquals(
            listOf("startScan:$TRUSTED_ADDRESS"),
            env.host.operations.filter { it.startsWith("startScan") || it.startsWith("connect") },
        )
    }

    @Test
    fun validPressReleaseEmitsOneNormalizedPair() {
        val env = Env()
        env.becomeReady()
        env.currentGatt().notifyTx("P\nR\n".toByteArray())

        assertEquals(listOf(InputEdge.PRESS, InputEdge.RELEASE), env.callback.edges)
        assertEquals(1, env.callback.readyCount)
        assertEquals(false, env.currentGatt().autoConnect)
        assertEquals(
            listOf("HELLO\n"),
            env.currentGatt().writes.filter { it.characteristic == BleSwitch.NUS_RX }.map { it.payload.decodeToString() },
        )
    }

    @Test
    fun malformedPayloadAndHungGattProduceNoEdge() {
        val env = Env()
        env.becomeReady()
        env.currentGatt().notifyTx(byteArrayOf(0x80.toByte(), '\n'.code.toByte()))
        env.currentGatt().notifyTx("XXXX\nP\nR\n".toByteArray())
        assertEquals(listOf(InputEdge.PRESS, InputEdge.RELEASE), env.callback.edges)

        val hung = Env()
        hung.start()
        hung.connectTrusted()
        hung.currentGatt().discover(nusServices())
        assertTrue(hung.scheduler.runNext())
        assertFalse(hung.callback.becameReady)
        assertEquals(emptyList<InputEdge>(), hung.callback.edges)
        assertTrue(hung.currentGatt().closed)
    }

    @Test
    fun stopCancelsRetryAndIgnoresLaterGatt() {
        val env = Env()
        env.becomeReady()
        env.currentGatt().notifyTx("P\n".toByteArray())
        env.currentGatt().disconnect()
        env.ble.stop()
        assertFalse(env.scheduler.hasPending)
        env.host.emitScan(TRUSTED_ADDRESS, BleSwitch.ADVERTISED_NAME)
        assertEquals(1, env.host.gattSessions.size)
        env.host.gattSessions.first().notifyTx("R\n".toByteArray())

        assertEquals(listOf(InputEdge.PRESS), env.callback.edges)
        assertEquals(TransportState.STOPPED, env.callback.lastState)
    }

    @Test
    fun cccdAckIsRequiredBeforeHelloAndOldGattIsClosedOnRetry() {
        val env = Env()
        env.start()
        env.connectTrusted()
        val first = env.currentGatt()
        first.discover(nusServices())
        assertFalse(first.wroteHello)
        first.notifyTx("P\n".toByteArray())
        assertEquals(emptyList<InputEdge>(), env.callback.edges)
        first.ackCccd()
        assertTrue(first.wroteHello)
        first.failHelloWrite()
        assertTrue(first.closed)
        assertTrue(env.scheduler.runNext())
        env.connectTrusted()
        assertTrue(first.closed)
        assertEquals(false, env.currentGatt().autoConnect)
        assertEquals(2, env.host.gattSessions.size)
        assertFalse(env.callback.becameReady)
    }

    @Test
    fun exactHandshakeRejectsCrLfAndWrongIdentity() {
        val crlf = Env()
        crlf.start()
        crlf.connectTrusted()
        crlf.currentGatt().discover(nusServices())
        crlf.currentGatt().ackCccd()
        crlf.currentGatt().ackHello()
        crlf.currentGatt().notifyTx("HANBEON_UNO_V1\r\n".toByteArray())
        crlf.currentGatt().notifyTx("P\nR\n".toByteArray())

        assertFalse(crlf.callback.becameReady)
        assertEquals(emptyList<InputEdge>(), crlf.callback.edges)
        assertTrue(crlf.currentGatt().closed)

        val wrong = Env()
        wrong.start()
        wrong.connectTrusted()
        wrong.currentGatt().discover(nusServices())
        wrong.currentGatt().ackCccd()
        wrong.currentGatt().ackHello()
        wrong.currentGatt().notifyTx("HANBEON_UNO_V2\n".toByteArray())
        wrong.currentGatt().notifyTx("P\nR\n".toByteArray())

        assertFalse(wrong.callback.becameReady)
        assertEquals(emptyList<InputEdge>(), wrong.callback.edges)
        assertTrue(wrong.currentGatt().closed)
    }

    @Test
    fun leftoverPressReleaseAfterExactIdentityEmitsOneNormalizedPair() {
        val env = Env()
        env.start()
        env.connectTrusted()
        env.currentGatt().discover(nusServices())
        env.currentGatt().ackCccd()
        env.currentGatt().ackHello()
        env.currentGatt().notifyTx("HANBEON_UNO_V1\nP\nR\n".toByteArray())

        assertTrue(env.callback.becameReady)
        assertEquals(listOf(InputEdge.PRESS, InputEdge.RELEASE), env.callback.edges)
        assertEquals(1, env.callback.readyCount)
    }

    @Test
    fun identityNotifyBeforeHelloAckStillBecomesReady() {
        val env = Env()
        env.start()
        env.connectTrusted()
        env.currentGatt().discover(nusServices())
        env.currentGatt().ackCccd()
        env.currentGatt().notifyTx(BleSwitch.HANDSHAKE)
        assertFalse(env.callback.becameReady)
        env.currentGatt().ackHello()

        assertTrue(env.callback.becameReady)
        env.currentGatt().notifyTx("P\nR\n".toByteArray())
        assertEquals(listOf(InputEdge.PRESS, InputEdge.RELEASE), env.callback.edges)
    }

    @Test
    fun staleFailedDiscoveryCannotTearDownReplacementSession() {
        val env = Env()
        env.becomeReady()
        val stale = env.currentGatt()
        stale.disconnect()
        assertTrue(env.scheduler.runNext())
        env.connectTrusted()
        stale.discover(nusServices(), status = 133)
        env.completeHandshake()
        env.currentGatt().notifyTx("P\nR\n".toByteArray())

        assertEquals(listOf(InputEdge.PRESS, InputEdge.RELEASE), env.callback.edges)
        assertEquals(2, env.callback.readyCount)
        assertEquals(env.callback.lastReadySession, env.callback.lastEdgeSession)
        assertFalse(stale === env.currentGatt())
        assertTrue(stale.closed)
    }

    @Test
    fun noSavedIdentityNeverScansOrConnects() {
        val env = Env(identity = null)
        env.start()

        assertEquals(emptyList<String>(), env.host.operations)
        assertTrue(env.host.gattSessions.isEmpty())
        assertFalse(env.callback.becameReady)
        assertEquals(emptyList<InputEdge>(), env.callback.edges)
        assertEquals(TransportState.LOST, env.callback.lastState)
    }

    @Test
    fun retryStopsAfterMaxAttemptsAndStopCancelsHungSetup() {
        val env = Env(maxAttempts = 2)
        env.start()
        env.connectTrusted()
        env.currentGatt().discover(nusServices(cccd = false))
        assertTrue(env.scheduler.hasPending)
        assertTrue(env.scheduler.runNext())
        env.connectTrusted()
        env.currentGatt().discover(nusServices(cccd = false))
        assertFalse(env.scheduler.hasPending)
        assertFalse(env.callback.becameReady)
        assertEquals(emptyList<InputEdge>(), env.callback.edges)
        assertEquals(2, env.host.gattSessions.size)

        val hung = Env()
        hung.start()
        hung.connectTrusted()
        hung.ble.stop()
        assertFalse(hung.scheduler.hasPending)
        hung.currentGatt().discover(nusServices())
        hung.currentGatt().ackCccd()
        hung.currentGatt().ackHello()
        hung.currentGatt().notifyTx(BleSwitch.HANDSHAKE)
        hung.currentGatt().notifyTx("P\nR\n".toByteArray())
        assertFalse(hung.callback.becameReady)
        assertEquals(emptyList<InputEdge>(), hung.callback.edges)
        assertEquals(TransportState.STOPPED, hung.callback.lastState)
    }
}

private const val TRUSTED_ADDRESS = "AA:BB:CC:DD:EE:FF"
private const val OTHER_ADDRESS = "11:22:33:44:55:66"

private class Env(
    identity: TrustedXiaoIdentity? = TrustedXiaoIdentity(TRUSTED_ADDRESS),
    maxAttempts: Int = 4,
) {
    val host = FakeBleHost()
    val scheduler = FakeBleScheduler()
    val callback = RecordingBleCallback()
    val ble =
        BleSwitch(
            host = host,
            identityStore = TrustedXiaoStore { identity },
            sessionIds = TransportSessionIds(),
            scheduler = scheduler,
            setupTimeoutMillis = 25,
            retryDelayMillis = 10,
            maxAttempts = maxAttempts,
        )

    fun start() {
        ble.start(callback)
    }

    fun connectTrusted() {
        host.emitScan(TRUSTED_ADDRESS, BleSwitch.ADVERTISED_NAME)
        currentGatt().connect()
    }

    fun completeHandshake() {
        currentGatt().discover(nusServices())
        currentGatt().ackCccd()
        currentGatt().ackHello()
        currentGatt().notifyTx(BleSwitch.HANDSHAKE)
    }

    fun becomeReady() {
        start()
        connectTrusted()
        completeHandshake()
        assertTrue(callback.becameReady)
    }

    fun currentGatt(): FakeGattSession = host.gattSessions.last()
}

private fun nusServices(
    rx: BleCharacteristicSnapshot? =
        BleCharacteristicSnapshot(
            uuid = BleSwitch.NUS_RX,
            properties = BleSwitch.PROPERTY_WRITE or BleSwitch.PROPERTY_WRITE_NO_RESPONSE,
            descriptors = emptyList(),
        ),
    tx: BleCharacteristicSnapshot? =
        BleCharacteristicSnapshot(
            uuid = BleSwitch.NUS_TX,
            properties = BleSwitch.PROPERTY_NOTIFY,
            descriptors = listOf(BleSwitch.CCCD),
        ),
    cccd: Boolean = true,
): List<BleServiceSnapshot> {
    val txChar =
        tx?.copy(descriptors = if (cccd) listOf(BleSwitch.CCCD) else emptyList())
    return listOf(
        BleServiceSnapshot(
            uuid = BleSwitch.NUS_SERVICE,
            characteristics = listOfNotNull(rx, txChar),
        ),
    )
}

private class RecordingBleCallback : InputTransport.Callback {
    val states = mutableListOf<Pair<TransportSession, TransportState>>()
    val recordedEdges = mutableListOf<Pair<TransportSession, InputEdge>>()

    val edges: List<InputEdge>
        get() = recordedEdges.map { it.second }

    val becameReady: Boolean
        get() = states.any { it.second == TransportState.READY }

    val readyCount: Int
        get() = states.count { it.second == TransportState.READY }

    val lastState: TransportState?
        get() = states.lastOrNull()?.second

    val lastReadySession: TransportSession?
        get() = states.lastOrNull { it.second == TransportState.READY }?.first

    val lastEdgeSession: TransportSession?
        get() = recordedEdges.lastOrNull()?.first

    override fun onState(session: TransportSession, state: TransportState) {
        states.add(session to state)
    }

    override fun onEdge(session: TransportSession, edge: InputEdge) {
        recordedEdges.add(session to edge)
    }
}

private class FakeBleScheduler : TransportScheduler {
    private data class Scheduled(val task: () -> Unit, var cancelled: Boolean)

    private val tasks = ArrayDeque<Scheduled>()

    val hasPending: Boolean
        get() = tasks.any { !it.cancelled }

    override fun schedule(delayMillis: Long, task: () -> Unit): TransportCancellation {
        require(delayMillis >= 0)
        val scheduled = Scheduled(task, false)
        tasks.addLast(scheduled)
        return TransportCancellation { scheduled.cancelled = true }
    }

    fun runNext(): Boolean {
        while (tasks.isNotEmpty()) {
            val next = tasks.removeFirst()
            if (!next.cancelled) {
                next.task()
                return true
            }
        }
        return false
    }
}

private class FakeBleHost : BleHost {
    val operations = mutableListOf<String>()
    val gattSessions = mutableListOf<FakeGattSession>()
    private var scan: FakeScanSession? = null

    override fun startScan(
        identity: TrustedXiaoIdentity,
        listener: BleHost.ScanListener,
    ): BleHost.ScanSession {
        operations += "startScan:${identity.address}"
        val session = FakeScanSession(listener)
        scan = session
        return session
    }

    override fun connect(
        address: String,
        autoConnect: Boolean,
        listener: BleHost.GattListener,
    ): BleHost.GattSession {
        operations += "connect:$address:$autoConnect"
        val session = FakeGattSession(autoConnect, listener)
        gattSessions += session
        return session
    }

    fun emitScan(address: String, advertisedName: String?) {
        scan?.takeIf { it.active }?.listener?.onMatch(address, advertisedName)
    }
}

private class FakeScanSession(
    val listener: BleHost.ScanListener,
) : BleHost.ScanSession {
    var active = true
        private set

    override fun stop() {
        active = false
    }
}

private class FakeGattSession(
    override val autoConnect: Boolean,
    private val listener: BleHost.GattListener,
) : BleHost.GattSession {
    var closed = false
        private set
    var wroteHello = false
        private set
    val writes = mutableListOf<GattWrite>()
    private var services: List<BleServiceSnapshot> = emptyList()
    private var notifyEnabled = false

    override fun discoverServices(): Boolean {
        if (closed) return false
        return true
    }

    override fun services(): List<BleServiceSnapshot> = services

    override fun enableCharacteristicNotification(characteristicUuid: UUID): Boolean {
        if (closed) return false
        val characteristic = services.characteristic(characteristicUuid) ?: return false
        if (characteristic.properties and BleSwitch.PROPERTY_NOTIFY == 0) return false
        notifyEnabled = true
        return true
    }

    override fun writeDescriptor(
        characteristicUuid: UUID,
        descriptorUuid: UUID,
        value: ByteArray,
    ): Boolean {
        if (closed) return false
        val characteristic = services.characteristic(characteristicUuid) ?: return false
        if (descriptorUuid !in characteristic.descriptors) return false
        writes += GattWrite(characteristicUuid, descriptorUuid, value.copyOf())
        return true
    }

    override fun writeCharacteristic(characteristicUuid: UUID, value: ByteArray): Boolean {
        if (closed) return false
        writes += GattWrite(characteristicUuid, null, value.copyOf())
        if (characteristicUuid == BleSwitch.NUS_RX && value.contentEquals(BleSwitch.HELLO)) {
            wroteHello = true
        }
        return true
    }

    override fun disconnect() {
        if (closed) return
        listener.onConnectionState(connected = false, status = 0, session = this)
        close()
    }

    override fun close() {
        closed = true
    }

    fun connect() {
        listener.onConnectionState(connected = true, status = 0, session = this)
    }

    fun discover(discovered: List<BleServiceSnapshot>, status: Int = 0) {
        services = discovered
        listener.onServicesDiscovered(status = status, session = this)
    }

    fun ackCccd(status: Int = 0) {
        listener.onDescriptorWrite(
            descriptorUuid = BleSwitch.CCCD,
            characteristicUuid = BleSwitch.NUS_TX,
            status = status,
            session = this,
        )
    }

    fun ackHello(status: Int = 0) {
        listener.onCharacteristicWrite(
            characteristicUuid = BleSwitch.NUS_RX,
            status = status,
            session = this,
        )
    }

    fun failHelloWrite() {
        listener.onCharacteristicWrite(
            characteristicUuid = BleSwitch.NUS_RX,
            status = 133,
            session = this,
        )
    }

    fun notifyTx(value: ByteArray) {
        listener.onCharacteristicChanged(
            characteristicUuid = BleSwitch.NUS_TX,
            value = value,
            session = this,
        )
    }
}

private data class GattWrite(
    val characteristic: UUID,
    val descriptor: UUID?,
    val payload: ByteArray,
)

private fun List<BleServiceSnapshot>.characteristic(uuid: UUID): BleCharacteristicSnapshot? =
    asSequence().flatMap { it.characteristics.asSequence() }.firstOrNull { it.uuid == uuid }
