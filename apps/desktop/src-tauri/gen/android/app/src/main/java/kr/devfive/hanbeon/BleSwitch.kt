package kr.devfive.hanbeon

import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCallback
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattDescriptor
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanFilter
import android.bluetooth.le.ScanResult
import android.bluetooth.le.ScanSettings
import android.content.Context
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.ParcelUuid
import java.util.UUID

data class TrustedXiaoIdentity(
    val address: String,
)

fun interface TrustedXiaoStore {
    fun load(): TrustedXiaoIdentity?
}

data class BleCharacteristicSnapshot(
    val uuid: UUID,
    val properties: Int,
    val descriptors: List<UUID>,
)

data class BleServiceSnapshot(
    val uuid: UUID,
    val characteristics: List<BleCharacteristicSnapshot>,
)

interface BleHost {
    fun startScan(
        identity: TrustedXiaoIdentity,
        listener: ScanListener,
    ): ScanSession

    fun connect(
        address: String,
        autoConnect: Boolean,
        listener: GattListener,
    ): GattSession

    interface ScanListener {
        fun onMatch(address: String, advertisedName: String?)
    }

    interface ScanSession {
        fun stop()
    }

    interface GattListener {
        fun onConnectionState(connected: Boolean, status: Int, session: GattSession)

        fun onServicesDiscovered(status: Int, session: GattSession)

        fun onDescriptorWrite(
            descriptorUuid: UUID,
            characteristicUuid: UUID,
            status: Int,
            session: GattSession,
        )

        fun onCharacteristicWrite(
            characteristicUuid: UUID,
            status: Int,
            session: GattSession,
        )

        fun onCharacteristicChanged(
            characteristicUuid: UUID,
            value: ByteArray,
            session: GattSession,
        )
    }

    interface GattSession {
        val autoConnect: Boolean

        fun discoverServices(): Boolean

        fun services(): List<BleServiceSnapshot>

        fun enableCharacteristicNotification(characteristicUuid: UUID): Boolean

        fun writeDescriptor(
            characteristicUuid: UUID,
            descriptorUuid: UUID,
            value: ByteArray,
        ): Boolean

        fun writeCharacteristic(characteristicUuid: UUID, value: ByteArray): Boolean

        fun disconnect()

        fun close()
    }
}

class HandlerTransportScheduler(
    private val handler: Handler,
) : TransportScheduler {
    override fun schedule(delayMillis: Long, task: () -> Unit): TransportCancellation {
        val runnable = Runnable { task() }
        handler.postDelayed(runnable, delayMillis)
        return TransportCancellation { handler.removeCallbacks(runnable) }
    }
}

/** Platform BLE scanner/GATT owner. All listener calls are posted onto [handler]. */
class AndroidBleHost(
    private val context: Context,
    private val handler: Handler,
) : BleHost {
    override fun startScan(
        identity: TrustedXiaoIdentity,
        listener: BleHost.ScanListener,
    ): BleHost.ScanSession {
        val scanner = adapter()?.bluetoothLeScanner
        if (scanner == null) {
            return NoopScanSession
        }
        val callback =
            object : ScanCallback() {
                override fun onScanResult(callbackType: Int, result: ScanResult) {
                    emit(result, listener)
                }

                override fun onBatchScanResults(results: MutableList<ScanResult>) {
                    for (result in results) emit(result, listener)
                }
            }
        val filters =
            listOf(
                ScanFilter.Builder()
                    .setDeviceAddress(identity.address)
                    .setServiceUuid(ParcelUuid(BleSwitch.NUS_SERVICE))
                    .build(),
            )
        val settings =
            ScanSettings.Builder()
                .setScanMode(ScanSettings.SCAN_MODE_LOW_LATENCY)
                .build()
        return try {
            scanner.startScan(filters, settings, callback)
            object : BleHost.ScanSession {
                override fun stop() {
                    runCatching { scanner.stopScan(callback) }
                }
            }
        } catch (_: IllegalArgumentException) {
            NoopScanSession
        } catch (_: SecurityException) {
            NoopScanSession
        }
    }

    override fun connect(
        address: String,
        autoConnect: Boolean,
        listener: BleHost.GattListener,
    ): BleHost.GattSession {
        val device = adapter()?.getRemoteDevice(address)
        val session = AndroidGattSession(autoConnect, listener, handler)
        if (device == null) {
            handler.post { listener.onConnectionState(false, BluetoothGatt.GATT_FAILURE, session) }
            return session
        }
        val gatt =
            try {
                device.connectGatt(context, autoConnect, session.callback, BluetoothDevice.TRANSPORT_LE)
            } catch (_: SecurityException) {
                null
            }
        if (gatt == null) {
            handler.post { listener.onConnectionState(false, BluetoothGatt.GATT_FAILURE, session) }
            return session
        }
        session.attach(gatt)
        return session
    }

    private fun emit(result: ScanResult, listener: BleHost.ScanListener) {
        val address = result.device?.address ?: return
        val name = result.scanRecord?.deviceName ?: result.device?.name
        handler.post { listener.onMatch(address, name) }
    }

    private fun adapter() =
        context.getSystemService(BluetoothManager::class.java)?.adapter

    private object NoopScanSession : BleHost.ScanSession {
        override fun stop() = Unit
    }
}

private class AndroidGattSession(
    override val autoConnect: Boolean,
    private val listener: BleHost.GattListener,
    private val handler: Handler,
) : BleHost.GattSession {
    var callback: BluetoothGattCallback =
        object : BluetoothGattCallback() {
            override fun onConnectionStateChange(gatt: BluetoothGatt, status: Int, newState: Int) {
                handler.post {
                    attach(gatt)
                    listener.onConnectionState(
                        connected = newState == BluetoothProfile.STATE_CONNECTED,
                        status = status,
                        session = this@AndroidGattSession,
                    )
                }
            }

            override fun onServicesDiscovered(gatt: BluetoothGatt, status: Int) {
                handler.post {
                    attach(gatt)
                    listener.onServicesDiscovered(status, this@AndroidGattSession)
                }
            }

            override fun onDescriptorWrite(
                gatt: BluetoothGatt,
                descriptor: BluetoothGattDescriptor,
                status: Int,
            ) {
                handler.post {
                    attach(gatt)
                    listener.onDescriptorWrite(
                        descriptorUuid = descriptor.uuid,
                        characteristicUuid = descriptor.characteristic.uuid,
                        status = status,
                        session = this@AndroidGattSession,
                    )
                }
            }

            override fun onCharacteristicWrite(
                gatt: BluetoothGatt,
                characteristic: BluetoothGattCharacteristic,
                status: Int,
            ) {
                handler.post {
                    attach(gatt)
                    listener.onCharacteristicWrite(
                        characteristicUuid = characteristic.uuid,
                        status = status,
                        session = this@AndroidGattSession,
                    )
                }
            }

            @Deprecated("Use the value-bearing onCharacteristicChanged overload")
            @Suppress("DEPRECATION")
            override fun onCharacteristicChanged(
                gatt: BluetoothGatt,
                characteristic: BluetoothGattCharacteristic,
            ) {
                deliverChanged(gatt, characteristic, characteristic.value)
            }

            override fun onCharacteristicChanged(
                gatt: BluetoothGatt,
                characteristic: BluetoothGattCharacteristic,
                value: ByteArray,
            ) {
                deliverChanged(gatt, characteristic, value)
            }

            private fun deliverChanged(
                gatt: BluetoothGatt,
                characteristic: BluetoothGattCharacteristic,
                value: ByteArray?,
            ) {
                val payload = value?.copyOf() ?: ByteArray(0)
                handler.post {
                    attach(gatt)
                    listener.onCharacteristicChanged(
                        characteristicUuid = characteristic.uuid,
                        value = payload,
                        session = this@AndroidGattSession,
                    )
                }
            }
        }
        private set

    private var gatt: BluetoothGatt? = null

    fun attach(gatt: BluetoothGatt) {
        this.gatt = gatt
    }

    override fun discoverServices(): Boolean = gatt?.discoverServices() == true

    override fun services(): List<BleServiceSnapshot> {
        val gatt = gatt ?: return emptyList()
        val discovered = gatt.services ?: return emptyList()
        return discovered.map { service ->
            BleServiceSnapshot(
                uuid = service.uuid,
                characteristics =
                    service.characteristics.orEmpty().map { characteristic ->
                        BleCharacteristicSnapshot(
                            uuid = characteristic.uuid,
                            properties = characteristic.properties,
                            descriptors = characteristic.descriptors.orEmpty().map { it.uuid },
                        )
                    },
            )
        }
    }

    override fun enableCharacteristicNotification(characteristicUuid: UUID): Boolean {
        val characteristic = characteristic(characteristicUuid) ?: return false
        return gatt?.setCharacteristicNotification(characteristic, true) == true
    }

    override fun writeDescriptor(
        characteristicUuid: UUID,
        descriptorUuid: UUID,
        value: ByteArray,
    ): Boolean {
        val gatt = gatt ?: return false
        val descriptor = characteristic(characteristicUuid)?.getDescriptor(descriptorUuid) ?: return false
        val payload = value.copyOf()
        return if (Build.VERSION.SDK_INT >= 33) {
            gatt.writeDescriptor(descriptor, payload) == BluetoothGatt.GATT_SUCCESS
        } else {
            @Suppress("DEPRECATION")
            run {
                descriptor.value = payload
                gatt.writeDescriptor(descriptor)
            }
        }
    }

    override fun writeCharacteristic(characteristicUuid: UUID, value: ByteArray): Boolean {
        val gatt = gatt ?: return false
        val characteristic = characteristic(characteristicUuid) ?: return false
        val payload = value.copyOf()
        return if (Build.VERSION.SDK_INT >= 33) {
            gatt.writeCharacteristic(
                characteristic,
                payload,
                BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT,
            ) == BluetoothGatt.GATT_SUCCESS
        } else {
            @Suppress("DEPRECATION")
            run {
                characteristic.writeType = BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT
                characteristic.value = payload
                gatt.writeCharacteristic(characteristic)
            }
        }
    }

    override fun disconnect() {
        gatt?.disconnect()
    }

    override fun close() {
        val current = gatt
        gatt = null
        current?.disconnect()
        current?.close()
    }

    private fun characteristic(uuid: UUID): BluetoothGattCharacteristic? {
        val gatt = gatt ?: return null
        for (service in gatt.services.orEmpty()) {
            val characteristic = service.getCharacteristic(uuid)
            if (characteristic != null) return characteristic
        }
        return null
    }
}

/**
 * Trusted XIAO Nordic UART transport.
 *
 * Scan matches only the caregiver-saved address plus the NUS service. GATT
 * setup is serialized: connect (`autoConnect=false`) → discover NUS → CCCD ACK
 * → HELLO → exact `HANBEON_UNO_V1` identity. Press/release records are accepted
 * only after Ready. Disconnect while held reports Lost and never synthesizes a
 * release.
 */
class BleSwitch(
    private val host: BleHost,
    private val identityStore: TrustedXiaoStore,
    private val sessionIds: TransportSessionIds = TransportSessionIds(),
    private val scheduler: TransportScheduler,
    private val setupTimeoutMillis: Long = SETUP_TIMEOUT_MS,
    private val retryDelayMillis: Long = RETRY_DELAY_MS,
    private val maxAttempts: Int = MAX_ATTEMPTS,
) : InputTransport {
    constructor(
        context: Context,
        identityStore: TrustedXiaoStore,
        sessionIds: TransportSessionIds = TransportSessionIds(),
        looper: Looper = Looper.getMainLooper(),
    ) : this(
        host = AndroidBleHost(context, Handler(looper)),
        identityStore = identityStore,
        sessionIds = sessionIds,
        scheduler = HandlerTransportScheduler(Handler(looper)),
    )

    override val source = TransportSource.BLE

    private val lock = Any()
    private var parser = InputRecordParser()
    private var callback: InputTransport.Callback? = null
    private var running = false
    private var session: TransportSession? = null
    private var scan: BleHost.ScanSession? = null
    private var gatt: BleHost.GattSession? = null
    private var setupCancellation: TransportCancellation? = null
    private var retryCancellation: TransportCancellation? = null
    private var attempts = 0
    private var handshakeComplete = false
    private var identitySeen = false
    private val pendingBytes = ArrayList<Byte>()
    private var phase = Phase.IDLE

    override fun start(callback: InputTransport.Callback) {
        synchronized(lock) {
            stopLocked()
            this.callback = callback
            running = true
            attempts = 0
            beginSessionLocked()
            connectOrScanLocked()
        }
    }

    override fun stop() {
        synchronized(lock) { stopLocked() }
    }

    private fun stopLocked() {
        val current = session
        val currentCallback = callback
        running = false
        cancelTimersLocked()
        closeScanLocked()
        closeGattLocked()
        resetProtocolLocked()
        phase = Phase.IDLE
        session = null
        callback = null
        if (current != null && currentCallback != null) {
            currentCallback.onState(current, TransportState.STOPPED)
        }
    }

    private fun beginSessionLocked() {
        resetProtocolLocked()
        phase = Phase.IDLE
        val next = sessionIds.next(TransportSource.BLE)
        session = next
        callback?.onState(next, TransportState.STARTING)
    }

    private fun connectOrScanLocked() {
        if (!running) return
        val identity = identityStore.load()
        if (identity == null) {
            emitLostLocked()
            return
        }
        if (attempts >= maxAttempts) {
            emitLostLocked()
            return
        }
        attempts += 1
        closeScanLocked()
        scan = host.startScan(identity, ScanForwarder(identity.address))
    }

    private fun onScanMatchLocked(address: String) {
        if (!running || phase != Phase.IDLE) return
        closeScanLocked()
        closeGattLocked()
        phase = Phase.CONNECTING
        gatt = host.connect(address, false, GattForwarder())
        armSetupTimeoutLocked()
    }

    private fun onConnectedLocked(sessionHandle: BleHost.GattSession) {
        if (!acceptLocked(sessionHandle) || phase != Phase.CONNECTING) return
        phase = Phase.DISCOVERING
        if (!sessionHandle.discoverServices()) {
            failAndRetryLocked()
        }
    }

    private fun onServicesLocked(sessionHandle: BleHost.GattSession) {
        if (!acceptLocked(sessionHandle) || phase != Phase.DISCOVERING) return
        val nus = sessionHandle.services().firstOrNull { it.uuid == NUS_SERVICE }
        val rx = nus?.characteristics?.firstOrNull { it.uuid == NUS_RX }
        val tx = nus?.characteristics?.firstOrNull { it.uuid == NUS_TX }
        val writable = rx != null && rx.properties and (PROPERTY_WRITE or PROPERTY_WRITE_NO_RESPONSE) != 0
        val notifiable = tx != null && tx.properties and PROPERTY_NOTIFY != 0
        val hasCccd = tx?.descriptors?.contains(CCCD) == true
        if (nus == null || !writable || !notifiable || !hasCccd) {
            failAndRetryLocked()
            return
        }
        if (!sessionHandle.enableCharacteristicNotification(NUS_TX)) {
            failAndRetryLocked()
            return
        }
        phase = Phase.ENABLING_CCCD
        if (!sessionHandle.writeDescriptor(NUS_TX, CCCD, CCCD_NOTIFY_ENABLE)) {
            failAndRetryLocked()
        }
    }

    private fun onCccdAckLocked(sessionHandle: BleHost.GattSession) {
        if (!acceptLocked(sessionHandle) || phase != Phase.ENABLING_CCCD) return
        phase = Phase.WRITING_HELLO
        if (!sessionHandle.writeCharacteristic(NUS_RX, HELLO)) {
            failAndRetryLocked()
        }
    }

    private fun onHelloAckLocked(sessionHandle: BleHost.GattSession) {
        if (!acceptLocked(sessionHandle) || phase != Phase.WRITING_HELLO) return
        phase = Phase.AWAITING_IDENTITY
        if (identitySeen) becomeReadyLocked()
    }

    private fun onNotifyLocked(
        sessionHandle: BleHost.GattSession,
        characteristic: UUID,
        value: ByteArray,
    ) {
        if (!acceptLocked(sessionHandle) || characteristic != NUS_TX) return
        if (phase == Phase.WRITING_HELLO || phase == Phase.AWAITING_IDENTITY) {
            consumeHandshakeLocked(value)
            return
        }
        if (phase != Phase.READY || !handshakeComplete) return
        emitEdgesLocked(value)
    }

    private fun consumeHandshakeLocked(value: ByteArray) {
        for (byte in value) {
            if (identitySeen) {
                pendingBytes += byte
                continue
            }
            val index = pendingBytes.size
            if (index >= HANDSHAKE.size || byte != HANDSHAKE[index]) {
                failAndRetryLocked()
                return
            }
            pendingBytes += byte
            if (pendingBytes.size == HANDSHAKE.size) {
                identitySeen = true
                pendingBytes.clear()
            }
        }
        if (identitySeen && phase == Phase.AWAITING_IDENTITY) {
            becomeReadyLocked()
        }
    }

    private fun becomeReadyLocked() {
        val leftover = pendingBytes.toByteArray()
        pendingBytes.clear()
        handshakeComplete = true
        identitySeen = true
        parser = InputRecordParser()
        phase = Phase.READY
        cancelSetupTimeoutLocked()
        val current = session ?: return
        callback?.onState(current, TransportState.READY)
        if (leftover.isNotEmpty()) emitEdgesLocked(leftover)
    }

    private fun emitEdgesLocked(value: ByteArray) {
        val current = session ?: return
        for (edge in parser.feed(value)) {
            callback?.onEdge(current, edge)
        }
    }

    private fun onDisconnectedLocked(sessionHandle: BleHost.GattSession) {
        if (!acceptLocked(sessionHandle)) return
        val wasReady = phase == Phase.READY
        closeGattLocked()
        resetProtocolLocked()
        phase = Phase.IDLE
        cancelSetupTimeoutLocked()
        if (wasReady) attempts = 0
        emitLostLocked()
        if (running) scheduleRetryLocked()
    }

    private fun failAndRetryLocked() {
        closeScanLocked()
        closeGattLocked()
        resetProtocolLocked()
        phase = Phase.IDLE
        cancelSetupTimeoutLocked()
        emitLostLocked()
        if (running) scheduleRetryLocked()
    }

    private fun emitLostLocked() {
        val current = session ?: return
        callback?.onState(current, TransportState.LOST)
    }

    private fun scheduleRetryLocked() {
        if (!running || attempts >= maxAttempts) return
        cancelRetryLocked()
        retryCancellation =
            scheduler.schedule(retryDelayMillis) {
                synchronized(lock) {
                    retryCancellation = null
                    if (running && phase == Phase.IDLE) {
                        beginSessionLocked()
                        connectOrScanLocked()
                    }
                }
            }
    }

    private fun armSetupTimeoutLocked() {
        cancelSetupTimeoutLocked()
        setupCancellation =
            scheduler.schedule(setupTimeoutMillis) {
                synchronized(lock) {
                    setupCancellation = null
                    if (running && phase != Phase.READY && phase != Phase.IDLE) {
                        failAndRetryLocked()
                    }
                }
            }
    }

    private fun cancelTimersLocked() {
        cancelSetupTimeoutLocked()
        cancelRetryLocked()
    }

    private fun cancelSetupTimeoutLocked() {
        setupCancellation?.cancel()
        setupCancellation = null
    }

    private fun cancelRetryLocked() {
        retryCancellation?.cancel()
        retryCancellation = null
    }

    private fun closeScanLocked() {
        scan?.stop()
        scan = null
    }

    private fun closeGattLocked() {
        val current = gatt
        gatt = null
        current?.close()
    }

    private fun resetProtocolLocked() {
        parser = InputRecordParser()
        handshakeComplete = false
        identitySeen = false
        pendingBytes.clear()
    }

    private fun acceptLocked(sessionHandle: BleHost.GattSession): Boolean =
        running && gatt === sessionHandle

    private inner class ScanForwarder(
        private val trustedAddress: String,
    ) : BleHost.ScanListener {
        override fun onMatch(address: String, advertisedName: String?) {
            synchronized(lock) {
                if (!address.equals(trustedAddress, ignoreCase = true)) return
                onScanMatchLocked(address)
            }
        }
    }

    private inner class GattForwarder : BleHost.GattListener {
        override fun onConnectionState(connected: Boolean, status: Int, session: BleHost.GattSession) {
            synchronized(lock) {
                if (!acceptLocked(session)) return
                if (connected && status == 0) {
                    onConnectedLocked(session)
                } else {
                    onDisconnectedLocked(session)
                }
            }
        }

        override fun onServicesDiscovered(status: Int, session: BleHost.GattSession) {
            synchronized(lock) {
                if (!acceptLocked(session)) return
                if (status != 0) {
                    failAndRetryLocked()
                    return
                }
                onServicesLocked(session)
            }
        }

        override fun onDescriptorWrite(
            descriptorUuid: UUID,
            characteristicUuid: UUID,
            status: Int,
            session: BleHost.GattSession,
        ) {
            synchronized(lock) {
                if (!acceptLocked(session)) return
                if (descriptorUuid != CCCD || characteristicUuid != NUS_TX) return
                if (status != 0) {
                    failAndRetryLocked()
                    return
                }
                onCccdAckLocked(session)
            }
        }

        override fun onCharacteristicWrite(
            characteristicUuid: UUID,
            status: Int,
            session: BleHost.GattSession,
        ) {
            synchronized(lock) {
                if (!acceptLocked(session)) return
                if (characteristicUuid != NUS_RX) return
                if (status != 0) {
                    failAndRetryLocked()
                    return
                }
                onHelloAckLocked(session)
            }
        }

        override fun onCharacteristicChanged(
            characteristicUuid: UUID,
            value: ByteArray,
            session: BleHost.GattSession,
        ) {
            synchronized(lock) {
                onNotifyLocked(session, characteristicUuid, value)
            }
        }
    }

    private enum class Phase {
        IDLE,
        CONNECTING,
        DISCOVERING,
        ENABLING_CCCD,
        WRITING_HELLO,
        AWAITING_IDENTITY,
        READY,
    }

    companion object {
        val NUS_SERVICE: UUID = UUID.fromString("6e400001-b5a3-f393-e0a9-e50e24dcca9e")
        val NUS_RX: UUID = UUID.fromString("6e400002-b5a3-f393-e0a9-e50e24dcca9e")
        val NUS_TX: UUID = UUID.fromString("6e400003-b5a3-f393-e0a9-e50e24dcca9e")
        val CCCD: UUID = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb")
        const val PROPERTY_WRITE = 0x08
        const val PROPERTY_WRITE_NO_RESPONSE = 0x04
        const val PROPERTY_NOTIFY = 0x10
        const val ADVERTISED_NAME = "HanBeon XIAO"
        val HELLO = "HELLO\n".toByteArray(Charsets.US_ASCII)
        val HANDSHAKE = "HANBEON_UNO_V1\n".toByteArray(Charsets.US_ASCII)
        const val HANDSHAKE_IDENTITY = "HANBEON_UNO_V1\n"
        val CCCD_NOTIFY_ENABLE = byteArrayOf(0x01, 0x00)
        const val SETUP_TIMEOUT_MS = 2_000L
        const val RETRY_DELAY_MS = 2_000L
        const val MAX_ATTEMPTS = 4
    }
}
