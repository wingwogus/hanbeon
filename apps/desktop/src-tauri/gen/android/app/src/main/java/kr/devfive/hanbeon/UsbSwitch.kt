package kr.devfive.hanbeon

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.hardware.usb.UsbConstants
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbDeviceConnection
import android.hardware.usb.UsbEndpoint
import android.hardware.usb.UsbInterface
import android.hardware.usb.UsbManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.concurrent.thread

/** Description of a candidate which has a CDC-ACM data interface and bulk I/O. */
data class UsbCandidate(
    val id: String,
    val name: String,
    val vendorId: Int,
    val productId: Int,
    val cdcAcm: Boolean,
)

/** The small host boundary keeps USB framework objects out of JVM lifecycle tests. */
interface UsbHost {
    fun candidates(): List<UsbCandidate>

    fun hasPermission(candidate: UsbCandidate): Boolean

    fun requestPermission(candidate: UsbCandidate)

    fun open(candidate: UsbCandidate): UsbByteChannel?
}

interface UsbByteChannel {
    fun write(bytes: ByteArray): Int

    fun read(buffer: ByteArray, offset: Int, length: Int, timeoutMillis: Long): Int

    fun close()
}

/** Runs the reader only after the handshake has made the session READY. */
interface UsbReaderStarter {
    fun start(
        session: TransportSession,
        channel: UsbByteChannel,
        initialBytes: ByteArray,
        onBytes: (TransportSession, ByteArray) -> Unit,
        onLost: (TransportSession) -> Unit,
    ): TransportCancellation
}

data class UsbRetryPolicy(
    val maxRetries: Int = 3,
    val delayMillis: Long = 1_000,
) {
    init {
        require(maxRetries >= 0) { "maxRetries must not be negative" }
        require(delayMillis >= 0) { "delayMillis must not be negative" }
    }
}

/** Exact bytes exchanged by the dedicated Hanbeon Uno firmware. */
object UsbSwitchProtocol {
    const val IDENT = "HANBEON_UNO_V1"
    val HELLO = "HELLO\n".toByteArray(Charsets.US_ASCII)
    val HANDSHAKE_RESPONSE = "$IDENT\n".toByteArray(Charsets.US_ASCII)
    const val HANDSHAKE_ATTEMPTS = 4
    const val MAX_HANDSHAKE_READS = 16
    const val READ_TIMEOUT_MILLIS = 500L
}

/**
 * UsbManager.requestPermission fills EXTRA_PERMISSION_GRANTED and EXTRA_DEVICE
 * into the supplied PendingIntent. FLAG_IMMUTABLE blocks that fill-in, so a
 * real grant is observed as denial. FLAG_MUTABLE exists from API 31.
 */
object UsbPermissionIntentPolicy {
    const val MUTABLE_FLAG_API = 31

    fun pendingIntentFlags(
        sdkInt: Int,
        immutableFlag: Int,
        mutableFlag: Int,
        updateCurrentFlag: Int,
    ): Int {
        val mutability = if (sdkInt >= MUTABLE_FLAG_API) mutableFlag else 0
        return (mutability or updateCurrentFlag) and immutableFlag.inv()
    }

    fun canUsbManagerFillGrantAndDevice(flags: Int, immutableFlag: Int): Boolean =
        flags and immutableFlag == 0
}

/**
 * Lifecycle and protocol controller for one USB source.
 *
 * A session becomes READY only after an exact, incremental identity response. Every
 * callback carries that session, so reader callbacks from a closed connection are inert.
 * Calls are serialized by this controller's lock; the arbiter can therefore consume its
 * callbacks on its own service-owned execution context without transport races.
 */
class UsbSwitchController(
    private val host: UsbHost,
    private val scheduler: TransportScheduler,
    private var callbacks: InputTransport.Callback,
    private val sessionIds: TransportSessionIds = TransportSessionIds(),
    private val retryPolicy: UsbRetryPolicy = UsbRetryPolicy(),
    private val readerStarter: UsbReaderStarter = NoopUsbReaderStarter,
) : InputTransport {
    override val source = TransportSource.USB

    private val lock = Any()
    private var started = false
    private var currentSession: TransportSession? = null
    private var lastSession: TransportSession? = null
    private var currentCandidate: UsbCandidate? = null
    private var retryCandidateId: String? = null
    private var channel: UsbByteChannel? = null
    private var readerCancellation: TransportCancellation? = null
    private var retryCancellation: TransportCancellation? = null
    private var awaitingPermission = false
    private var state: TransportState? = null
    private var retryCount = 0
    private var parser = InputRecordParser()
    private val namesBySession = mutableMapOf<TransportSession, String>()

    override fun start(callback: InputTransport.Callback) {
        synchronized(lock) {
            callbacks = callback
            startLocked()
        }
    }

    fun start() {
        synchronized(lock) { startLocked() }
    }

    private fun startLocked() {
        if (started) return
        started = true
        retryCount = 0
        retryCandidateId = null
        beginAttemptLocked(isRetry = false, candidateOverride = null)
    }

    override fun stop() {
        synchronized(lock) {
            if (!started && state == TransportState.STOPPED) return
            started = false
            retryCancellation?.cancel()
            retryCancellation = null
            readerCancellation?.cancel()
            readerCancellation = null
            channel?.close()
            channel = null
            awaitingPermission = false
            val stoppedSession = currentSession ?: lastSession
            currentSession = null
            currentCandidate = null
            state = TransportState.STOPPED
            // Publish only after the internal state is final. A lifecycle callback may
            // synchronously start a replacement transport; the old stop must not erase it.
            stoppedSession?.let { callbacks.onState(it, TransportState.STOPPED) }
        }
    }

    /** Completes the permission request belonging to the current STARTING session. */
    fun permissionResult(granted: Boolean, candidateId: String? = null) {
        synchronized(lock) {
            if (!started || !awaitingPermission) return
            val session = currentSession ?: return
            val candidate = currentCandidate ?: return
            if (candidateId != null && candidateId != candidate.id) return
            awaitingPermission = false
            if (!granted) {
                failCurrentLocked(session, scheduleRetry = false)
            } else {
                openCandidateLocked(session, candidate, isRetry = false)
            }
        }
    }

    /** Called by the Android USB attach broadcast. */
    fun attached(candidateId: String? = null) {
        synchronized(lock) {
            if (!started || currentSession != null || state == TransportState.READY) return
            retryCancellation?.cancel()
            retryCancellation = null
            retryCount = 0
            retryCandidateId = candidateId
            beginAttemptLocked(isRetry = false, candidateOverride = null)
        }
    }

    /** Called by the Android USB detach broadcast. */
    fun detached(candidateId: String? = null) {
        synchronized(lock) {
            if (!started) return
            val session = currentSession ?: return
            val candidate = currentCandidate ?: return
            if (candidateId != null && candidateId != candidate.id) return
            if (state == TransportState.READY) {
                retryCandidateId = candidate.id
                failCurrentLocked(session, scheduleRetry = true)
            } else {
                failCurrentLocked(session, scheduleRetry = false)
            }
        }
    }

    /** Reader seam used by tests and by the concrete reader callback. */
    fun readerBytes(session: TransportSession, bytes: ByteArray) {
        synchronized(lock) {
            if (!isCurrentReadyLocked(session)) return
            for (edge in parser.feed(bytes)) {
                if (!isCurrentReadyLocked(session)) return
                callbacks.onEdge(session, edge)
            }
        }
    }

    /** Reader seam used when bulkTransfer reports a physical loss/error. */
    fun readerLost(session: TransportSession) {
        synchronized(lock) {
            if (!started || !isCurrentReadyLocked(session)) return
            val candidate = currentCandidate ?: return
            retryCandidateId = candidate.id
            failCurrentLocked(session, scheduleRetry = true)
        }
    }

    fun candidateName(session: TransportSession): String? = synchronized(lock) {
        namesBySession[session]
    }

    private fun beginAttemptLocked(isRetry: Boolean, candidateOverride: UsbCandidate?) {
        if (!started) return
        val candidate = candidateOverride ?: selectCandidateLocked()
        val session = sessionIds.next(source)
        currentSession = session
        lastSession = session
        currentCandidate = candidate
        state = TransportState.STARTING
        awaitingPermission = false
        parser = InputRecordParser()
        candidate?.let { namesBySession[session] = it.name }
        callbacks.onState(session, TransportState.STARTING)

        if (candidate == null) {
            failCurrentLocked(session, scheduleRetry = isRetry)
        } else {
            openCandidateLocked(session, candidate, isRetry)
        }
    }

    private fun selectCandidateLocked(): UsbCandidate? {
        val candidates = host.candidates()
            .filter { it.cdcAcm }
            .sortedWith(compareBy<UsbCandidate> { it.vendorId }.thenBy { it.id })
        val requested = retryCandidateId
        return if (requested == null) candidates.firstOrNull()
        else candidates.firstOrNull { it.id == requested } ?: candidates.firstOrNull()
    }

    private fun openCandidateLocked(
        session: TransportSession,
        candidate: UsbCandidate,
        isRetry: Boolean,
    ) {
        if (!started || currentSession != session || currentCandidate != candidate) return
        if (!host.hasPermission(candidate)) {
            awaitingPermission = true
            host.requestPermission(candidate)
            return
        }

        val opened = host.open(candidate)
        if (opened == null) {
            failCurrentLocked(session, scheduleRetry = isRetry)
            return
        }

        val pending = performHandshake(opened)
        if (pending == null) {
            opened.close()
            failCurrentLocked(session, scheduleRetry = false)
            return
        }

        channel = opened
        state = TransportState.READY
        retryCount = 0
        retryCandidateId = candidate.id
        callbacks.onState(session, TransportState.READY)
        if (!started || currentSession != session || state != TransportState.READY) return
        if (pending.isNotEmpty()) readerBytes(session, pending)
        if (!started || currentSession != session || state != TransportState.READY) return
        val cancellation = readerStarter.start(
            session = session,
            channel = opened,
            initialBytes = ByteArray(0),
            onBytes = ::readerBytes,
            onLost = ::readerLost,
        )
        if (isCurrentReadyLocked(session)) {
            readerCancellation = cancellation
        } else {
            // A fake or platform reader may report loss synchronously while start()
            // is returning. Never retain a handle for the already-invalid session.
            cancellation.cancel()
        }
    }

    /**
     * Writes HELLO and accepts only the exact identity, preserving bytes after it.
     * Opening a USB Uno toggles DTR and resets the board, so a timed-out first
     * request is retried. Any non-empty mismatch is rejected immediately.
     */
    private fun performHandshake(opened: UsbByteChannel): ByteArray? {
        val expected = UsbSwitchProtocol.HANDSHAKE_RESPONSE
        val buffer = ByteArray(256)
        for (attempt in 0 until UsbSwitchProtocol.HANDSHAKE_ATTEMPTS) {
            if (!writeFully(opened, UsbSwitchProtocol.HELLO)) return null

            val received = ArrayList<Byte>(expected.size)
            var reads = 0
            while (received.size < expected.size && reads++ < UsbSwitchProtocol.MAX_HANDSHAKE_READS) {
                val count = opened.read(buffer, 0, buffer.size, UsbSwitchProtocol.READ_TIMEOUT_MILLIS)
                if (count <= 0) {
                    // A USB Uno resets on DTR, so an empty timeout retries HELLO.
                    // Partial bytes that never complete are a mismatch, not a retry.
                    break
                }
                if (count > buffer.size) return null
                repeat(count) { received += buffer[it] }
            }

            if (received.isEmpty()) {
                if (attempt + 1 < UsbSwitchProtocol.HANDSHAKE_ATTEMPTS) continue
                return null
            }
            if (received.size < expected.size) return null
            for (index in expected.indices) {
                if (received[index] != expected[index]) return null
            }
            return received.drop(expected.size).toByteArray()
        }
        return null
    }

    private fun writeFully(opened: UsbByteChannel, bytes: ByteArray): Boolean {
        var offset = 0
        while (offset < bytes.size) {
            val written = opened.write(bytes.copyOfRange(offset, bytes.size))
            if (written <= 0 || written > bytes.size - offset) return false
            offset += written
        }
        return true
    }

    private fun failCurrentLocked(
        session: TransportSession,
        scheduleRetry: Boolean,
    ) {
        if (currentSession != session) return
        readerCancellation?.cancel()
        readerCancellation = null
        channel?.close()
        channel = null
        awaitingPermission = false
        currentSession = null
        currentCandidate = null
        state = TransportState.LOST
        callbacks.onState(session, TransportState.LOST)
        if (scheduleRetry) scheduleRetryLocked()
    }

    private fun scheduleRetryLocked() {
        if (!started || currentSession != null || retryCancellation != null || retryCount >= retryPolicy.maxRetries) return
        retryCount++
        retryCancellation = scheduler.schedule(retryPolicy.delayMillis) {
            synchronized(lock) {
                retryCancellation = null
                if (started && currentSession == null) {
                    beginAttemptLocked(isRetry = true, candidateOverride = null)
                }
            }
        }
    }

    private fun isCurrentReadyLocked(session: TransportSession): Boolean =
        started && currentSession == session && state == TransportState.READY

    private object NoopUsbReaderStarter : UsbReaderStarter {
        override fun start(
            session: TransportSession,
            channel: UsbByteChannel,
            initialBytes: ByteArray,
            onBytes: (TransportSession, ByteArray) -> Unit,
            onLost: (TransportSession) -> Unit,
        ): TransportCancellation = TransportCancellation { }
    }
}

/**
 * Android implementation of the host boundary. Any CDC-ACM data interface with
 * bulk I/O is enumerated; identity is the HELLO handshake, not VID/PID.
 * No CH340, FTDI, or Silicon Labs driver support is implied.
 */
private class AndroidUsbHost(private val context: Context) : UsbHost {
    private val manager: UsbManager
        get() = context.getSystemService(UsbManager::class.java)

    override fun candidates(): List<UsbCandidate> = manager.deviceList.values
        .mapNotNull(::candidate)
        .sortedWith(compareBy<UsbCandidate> { it.vendorId }.thenBy { it.id })

    override fun hasPermission(candidate: UsbCandidate): Boolean =
        manager.deviceList[candidate.id]?.let(manager::hasPermission) == true

    override fun requestPermission(candidate: UsbCandidate) {
        val intent = Intent(UsbSwitch.ACTION_PERMISSION).setPackage(context.packageName)
        val pending = PendingIntent.getBroadcast(
            context,
            PERMISSION_REQUEST_CODE,
            intent,
            UsbPermissionIntentPolicy.pendingIntentFlags(
                sdkInt = Build.VERSION.SDK_INT,
                immutableFlag = PendingIntent.FLAG_IMMUTABLE,
                mutableFlag = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                    PendingIntent.FLAG_MUTABLE
                } else {
                    0
                },
                updateCurrentFlag = PendingIntent.FLAG_UPDATE_CURRENT,
            ),
        )
        manager.deviceList[candidate.id]?.let { manager.requestPermission(it, pending) }
    }

    override fun open(candidate: UsbCandidate): UsbByteChannel? {
        val device = manager.deviceList[candidate.id] ?: return null
        val data = dataInterface(device) ?: return null
        val input = endpoint(data, UsbConstants.USB_DIR_IN) ?: return null
        val output = endpoint(data, UsbConstants.USB_DIR_OUT) ?: return null
        val opened = manager.openDevice(device) ?: return null
        val control = controlInterface(device) ?: run {
            opened.close()
            return null
        }
        try {
            if (!opened.claimInterface(control, true)) error("control claim failed")
            if (!opened.claimInterface(data, true)) error("data claim failed")
            if (!configure(opened, control)) error("CDC configuration failed")
            return AndroidUsbByteChannel(opened, control, data, input, output)
        } catch (_: Throwable) {
            opened.close()
            return null
        }
    }

    private fun candidate(device: UsbDevice): UsbCandidate? {
        if (dataInterface(device) == null) return null
        return UsbCandidate(
            id = device.deviceName,
            name = device.deviceName,
            vendorId = device.vendorId,
            productId = device.productId,
            cdcAcm = true,
        )
    }

    private fun configure(connection: UsbDeviceConnection, control: UsbInterface?): Boolean {
        if (control == null) return false
        val coding = byteArrayOf(
            0x00,
            0xC2.toByte(),
            0x01,
            0x00,
            0x00,
            0x00,
            0x08,
        )
        val codingResult = connection.controlTransfer(0x21, 0x20, 0, 0, coding, coding.size, 500)
        val lineResult = connection.controlTransfer(0x21, 0x22, 0x03, 0, null, 0, 500)
        return codingResult >= 0 && lineResult >= 0
    }

    private fun controlInterface(device: UsbDevice): UsbInterface? {
        val config = device.getConfiguration(0)
        return (0 until config.interfaceCount)
            .map(config::getInterface)
            .firstOrNull { it.interfaceClass == UsbConstants.USB_CLASS_COMM }
    }

    private fun dataInterface(device: UsbDevice): UsbInterface? {
        val config = device.getConfiguration(0)
        return (0 until config.interfaceCount)
            .map(config::getInterface)
            .firstOrNull {
                it.interfaceClass == UsbConstants.USB_CLASS_CDC_DATA &&
                    endpoint(it, UsbConstants.USB_DIR_IN) != null &&
                    endpoint(it, UsbConstants.USB_DIR_OUT) != null
            }
    }

    private fun endpoint(target: UsbInterface, direction: Int): UsbEndpoint? =
        (0 until target.endpointCount)
            .map(target::getEndpoint)
            .firstOrNull {
                it.direction == direction && it.type == UsbConstants.USB_ENDPOINT_XFER_BULK
            }

    companion object {
        const val PERMISSION_REQUEST_CODE = 47
    }
}

/**
 * UsbDeviceConnection.bulkTransfer returns a negative value both when the
 * timeout elapsed with no data and when the transfer genuinely failed; it does
 * not distinguish them. UsbByteChannel.read must, because the reader treats a
 * negative read as a physical loss and drops the device.
 *
 * An idle switch is the common case: nobody is pressing it, so every timeout
 * window ends with no bytes. Reporting that as loss dropped a healthy XIAO
 * about 200ms after it became READY. Only a closed channel is a real loss; a
 * bare timeout is "no data yet".
 */
object UsbBulkTransferResult {
    const val NO_DATA = 0
    const val LOST = -1

    fun toChannelRead(transferred: Int, channelClosed: Boolean): Int = when {
        transferred > 0 -> transferred
        channelClosed -> LOST
        else -> NO_DATA
    }
}

private class AndroidUsbByteChannel(
    private val connection: UsbDeviceConnection,
    private val control: UsbInterface,
    private val data: UsbInterface,
    private val input: UsbEndpoint,
    private val output: UsbEndpoint,
) : UsbByteChannel {
    private val closed = AtomicBoolean(false)
    private val transferBuffer = ByteArray(256)

    override fun write(bytes: ByteArray): Int {
        if (closed.get()) return -1
        return connection.bulkTransfer(output, bytes, bytes.size, UsbSwitchProtocol.READ_TIMEOUT_MILLIS.toInt())
    }

    override fun read(buffer: ByteArray, offset: Int, length: Int, timeoutMillis: Long): Int {
        if (closed.get()) return -1
        // Use the API-12 overload so the minSdk 24 build does not link the newer
        // offset overload. The reader uses a 256-byte buffer, but preserve the
        // interface contract for other callers too.
        val target = if (length <= transferBuffer.size) transferBuffer else ByteArray(length)
        val count = connection.bulkTransfer(input, target, length, timeoutMillis.toInt())
        if (count > 0) target.copyInto(buffer, offset, 0, minOf(count, length))
        return UsbBulkTransferResult.toChannelRead(count, closed.get())
    }

    override fun close() {
        if (!closed.compareAndSet(false, true)) return
        runCatching { connection.releaseInterface(data) }
        runCatching { connection.releaseInterface(control) }
        connection.close()
    }
}

internal class AndroidUsbReaderStarter : UsbReaderStarter {
    override fun start(
        session: TransportSession,
        channel: UsbByteChannel,
        initialBytes: ByteArray,
        onBytes: (TransportSession, ByteArray) -> Unit,
        onLost: (TransportSession) -> Unit,
    ): TransportCancellation {
        val stopped = AtomicBoolean(false)
        val worker = thread(name = "hanbeon-usb") {
            val buffer = ByteArray(256)
            while (!stopped.get()) {
                val count = channel.read(buffer, 0, buffer.size, 200)
                if (count > 0) {
                    onBytes(session, buffer.copyOf(count))
                } else if (count < 0) {
                    if (stopped.compareAndSet(false, true)) onLost(session)
                    return@thread
                }
            }
        }
        return TransportCancellation {
            if (stopped.compareAndSet(false, true)) worker.interrupt()
        }
    }
}

/**
 * Android USB serial switch facade. The legacy Event constructor remains as a narrow
 * compatibility adapter for the current service; normalized consumers use InputTransport.
 */
class UsbSwitch(
    private val context: Context,
    private val onEvent: (Event) -> Unit,
) : InputTransport {
    sealed interface Event {
        data class Connected(val name: String) : Event

        data object Disconnected : Event

        data object Press : Event

        data object Release : Event
    }

    override val source = TransportSource.USB
    private var normalizedCallback: InputTransport.Callback? = null
    private var registered = false

    private val controller = UsbSwitchController(
        host = AndroidUsbHost(context),
        scheduler = AndroidTransportScheduler(),
        callbacks = object : InputTransport.Callback {
            override fun onState(session: TransportSession, state: TransportState) {
                normalizedCallback?.onState(session, state)
                when (state) {
                    TransportState.READY -> onEvent(Event.Connected("USB"))
                    TransportState.LOST -> onEvent(Event.Disconnected)
                    TransportState.STARTING, TransportState.STOPPED -> Unit
                }
            }

            override fun onEdge(session: TransportSession, edge: InputEdge) {
                normalizedCallback?.onEdge(session, edge)
                when (edge) {
                    InputEdge.PRESS -> onEvent(Event.Press)
                    InputEdge.RELEASE -> onEvent(Event.Release)
                }
            }
        },
        readerStarter = AndroidUsbReaderStarter(),
    )

    override fun start(callback: InputTransport.Callback) {
        normalizedCallback = callback
        start()
    }

    fun start() {
        if (registered) return
        val filter = IntentFilter().apply {
            addAction(ACTION_PERMISSION)
            addAction(UsbManager.ACTION_USB_DEVICE_ATTACHED)
            addAction(UsbManager.ACTION_USB_DEVICE_DETACHED)
        }
        @Suppress("DEPRECATION")
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            context.registerReceiver(receiver, filter, Context.RECEIVER_NOT_EXPORTED)
        } else {
            context.registerReceiver(receiver, filter)
        }
        registered = true
        controller.start()
    }

    override fun stop() {
        if (!registered) return
        controller.stop()
        normalizedCallback = null
        registered = false
        runCatching { context.unregisterReceiver(receiver) }
    }

    private val receiver = object : BroadcastReceiver() {
        override fun onReceive(receiverContext: Context, intent: Intent) {
            val device = usbDevice(intent)
            when (intent.action) {
                ACTION_PERMISSION -> controller.permissionResult(
                    intent.getBooleanExtra(UsbManager.EXTRA_PERMISSION_GRANTED, false),
                    device?.deviceName,
                )
                UsbManager.ACTION_USB_DEVICE_ATTACHED -> controller.attached(device?.deviceName)
                UsbManager.ACTION_USB_DEVICE_DETACHED -> controller.detached(device?.deviceName)
            }
        }
    }

    @Suppress("DEPRECATION")
    private fun usbDevice(intent: Intent): UsbDevice? =
        intent.getParcelableExtra(UsbManager.EXTRA_DEVICE)

    private class AndroidTransportScheduler : TransportScheduler {
        private val handler = Handler(Looper.getMainLooper())

        override fun schedule(delayMillis: Long, task: () -> Unit): TransportCancellation {
            val runnable = Runnable(task)
            handler.postDelayed(runnable, delayMillis)
            return TransportCancellation { handler.removeCallbacks(runnable) }
        }
    }

    companion object {
        const val ACTION_PERMISSION = "kr.devfive.hanbeon.USB_PERMISSION"
    }
}
