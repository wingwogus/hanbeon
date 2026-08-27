package kr.devfive.hanbeon

/** Core-facing input and transport-loss boundary used by the arbiter runtime. */
interface CoreInput {
    fun pressed()

    fun released()

    fun cancelPress()

    fun suspendTransport()

    fun resumeTransport()
}

object AndroidCoreInput : CoreInput {
    override fun pressed() = Core.pressed()

    override fun released() = Core.released()

    override fun cancelPress() = Core.cancelPress()

    override fun suspendTransport() = Core.suspendTransport()

    override fun resumeTransport() = Core.resumeTransport()
}

data class TransportHints(
    val blePermissionDenied: Boolean = false,
    val userPaused: Boolean = false,
)

data class TransportFrontendEvent(
    val name: String,
    val payload: String,
) {
    companion object {
        fun from(snapshot: TransportStatusSnapshot) =
            TransportFrontendEvent(
                name = EVENT_TRANSPORT_LIFECYCLE,
                payload = snapshot.toJson(),
            )
    }
}

private const val EVENT_TRANSPORT_LIFECYCLE = "arduino://lifecycle"

data class TransportStatusSnapshot(
    val revision: Long,
    val code: String,
    val active: String? = null,
    val usb: String = TransportState.STOPPED.name.lowercase(),
    val ble: String = TransportState.STOPPED.name.lowercase(),
    val held: Boolean = false,
    val suspended: Boolean = false,
    val paused: Boolean = false,
) {
    fun toJson(): String =
        "{" +
            "\"revision\":$revision" +
            ",\"code\":" + jsonString(code) +
            ",\"active\":" + (active?.let(::jsonString) ?: "null") +
            ",\"usb\":" + jsonString(usb) +
            ",\"ble\":" + jsonString(ble) +
            ",\"held\":$held" +
            ",\"suspended\":$suspended" +
            ",\"paused\":$paused" +
            "}"
}

/**
 * Sticky, revisioned transport snapshot. A listener that mounts after the
 * current state still receives the latest value immediately.
 */
class TransportStatusHub {
    private val lock = Any()
    private val listeners = mutableListOf<(TransportStatusSnapshot) -> Unit>()
    private var snapshot = TransportStatusSnapshot(revision = 0, code = CODE_NO_INPUT)

    fun current(): TransportStatusSnapshot = synchronized(lock) { snapshot }

    fun addListener(listener: (TransportStatusSnapshot) -> Unit) {
        val current = synchronized(lock) {
            listeners += listener
            snapshot
        }
        listener(current)
    }

    fun publish(next: TransportStatusSnapshot) {
        val listeners = synchronized(lock) {
            snapshot = next
            this.listeners.toList()
        }
        for (listener in listeners) listener(next)
    }

    fun reset() {
        synchronized(lock) {
            listeners.clear()
            snapshot = TransportStatusSnapshot(revision = 0, code = CODE_NO_INPUT)
        }
    }

    companion object {
        const val CODE_NO_INPUT = "no-input"
        const val CODE_PERMISSION = "permission"
        const val CODE_SUSPENDED = "suspended"
        const val CODE_RECONNECTING = "reconnecting"
        const val CODE_PAUSED = "paused"
        const val CODE_READY = "ready"
    }
}

/**
 * USB-first serialized owner of USB serial and BLE NUS sources.
 *
 * Callers may invoke it from USB reader or BLE handler threads; every
 * transition runs under one lock so only the active source can reach Core.
 */
class SwitchTransportRuntime(
    private val usb: InputTransport,
    private val ble: InputTransport,
    private val core: CoreInput,
    private val status: TransportStatusHub,
) {
    private enum class CoreAction {
        PRESS,
        RELEASE,
        CANCEL,
        SUSPEND,
        RESUME,
    }

    private val lock = Any()
    private val arbiter = TransportArbiter { edge ->
        queue(
            when (edge) {
                InputEdge.PRESS -> CoreAction.PRESS
                InputEdge.RELEASE -> CoreAction.RELEASE
            },
        )
    }
    private val lastSession = mutableMapOf<TransportSource, TransportSession>()
    private var usbState = TransportState.STOPPED
    private var bleState = TransportState.STOPPED
    private var hints = TransportHints()
    private var coreSuspended = false
    private var started = false
    private var stopping = false
    private data class CoreBatch(
        val actions: List<CoreAction>,
        val finished: java.util.concurrent.CountDownLatch = java.util.concurrent.CountDownLatch(1),
    )

    private var revision = 0L
    private var queuedCore: MutableList<CoreAction>? = null
    private val coreBatches = java.util.ArrayDeque<CoreBatch>()
    private var coreDispatcher: Thread? = null

    val activeSession: TransportSession?
        get() = synchronized(lock) { arbiter.activeSession }

    val activeSource: TransportSource?
        get() = synchronized(lock) { arbiter.activeSession?.source }

    private val forwarder =
        object : InputTransport.Callback {
            override fun onState(session: TransportSession, state: TransportState) {
                handleState(session, state)
            }

            override fun onEdge(session: TransportSession, edge: InputEdge) {
                handleEdge(session, edge)
            }
        }

    fun start() {
        val snapshot =
            synchronized(lock) {
                if (started) return
                started = true
                stopping = false
                snapshotLocked()
            }
        status.publish(snapshot)
        usb.start(forwarder)
        ble.start(forwarder)
    }

    fun stop() {
        synchronized(lock) { stopping = true }
        usb.stop()
        ble.stop()
        mutate {
            val active = arbiter.activeSession
            if (active != null && arbiter.isPressed(active)) {
                queue(CoreAction.CANCEL)
            }
            if (active != null || coreSuspended) {
                queue(CoreAction.SUSPEND)
            }
            started = false
            lastSession.clear()
            usbState = TransportState.STOPPED
            bleState = TransportState.STOPPED
            coreSuspended = false
        }
    }

    fun setHints(hints: TransportHints) {
        mutate {
            this.hints =
                this.hints.copy(blePermissionDenied = hints.blePermissionDenied)
        }
    }

    /**
     * Core reports only its visible paused mode. The runtime knows whether it
     * initiated that pause for an input loss, so it can keep user and transport
     * pauses distinct in the native snapshot.
     */
    fun setUserPausedFromCore(paused: Boolean) {
        mutate {
            val userPaused = paused && !coreSuspended
            if (hints.userPaused == userPaused) return@mutate
            hints = hints.copy(userPaused = userPaused)
        }
    }

    private fun handleState(session: TransportSession, state: TransportState) {
        mutate {
            if (isStale(session)) return@mutate
            when (state) {
                TransportState.STARTING -> {
                    lastSession[session.source] = session
                    setSourceState(session.source, TransportState.STARTING)
                }
                TransportState.READY -> {
                    lastSession[session.source] = session
                    setSourceState(session.source, TransportState.READY)
                    val wasSuspended = coreSuspended
                    arbiter.ready(session)
                    applyUsbFirstLocked()
                    if (wasSuspended && arbiter.activeSession != null) {
                        coreSuspended = false
                        queue(CoreAction.RESUME)
                    }
                }
                TransportState.LOST -> handleLostLocked(session)
                TransportState.STOPPED -> {
                    if (lastSession[session.source] == session) {
                        setSourceState(session.source, TransportState.STOPPED)
                    }
                    if (!stopping) handleLostLocked(session)
                }
            }
        }
    }

    private fun handleEdge(session: TransportSession, edge: InputEdge) {
        mutate {
            if (stopping || isStale(session)) return@mutate
            arbiter.edge(session, edge)
            if (edge == InputEdge.RELEASE) applyUsbFirstLocked()
            activateReleasedStandbyLocked()
        }
    }

    private fun handleLostLocked(session: TransportSession) {
        if (isStale(session)) return
        val wasActive = arbiter.activeSession == session
        val wasHeld = wasActive && arbiter.isPressed(session)
        if (lastSession[session.source] == session) {
            setSourceState(session.source, TransportState.LOST)
        }
        arbiter.lost(session)
        if (!wasActive || stopping) return

        // A held source is an interrupted gesture, not a release. Queue Core
        // work so it runs without this lock; standby may resume only after a
        // fresh released boundary.
        if (wasHeld) {
            coreSuspended = !hints.userPaused
            queue(CoreAction.CANCEL)
            queue(CoreAction.SUSPEND)
            return
        }
        if (activateReleasedStandbyLocked()) return
        coreSuspended = !hints.userPaused
        queue(CoreAction.SUSPEND)
    }

    private fun applyUsbFirstLocked() {
        val usbSession = arbiter.sessionFor(TransportSource.USB) ?: return
        val active = arbiter.activeSession ?: return
        if (active.source == TransportSource.USB) return
        if (arbiter.isPressed(active) || arbiter.isPressed(usbSession)) return
        arbiter.select(usbSession)
    }

    private fun activateReleasedStandbyLocked(): Boolean {
        if (arbiter.activeSession != null) return false
        val next =
            listOfNotNull(
                arbiter.sessionFor(TransportSource.USB),
                arbiter.sessionFor(TransportSource.BLE),
            ).firstOrNull { !arbiter.isPressed(it) } ?: return false
        if (!arbiter.select(next)) return false
        if (coreSuspended) {
            coreSuspended = false
            queue(CoreAction.RESUME)
        }
        return true
    }

    private fun mutate(block: () -> Unit) {
        val actions = mutableListOf<CoreAction>()
        var batch: CoreBatch? = null
        var dispatch = false
        val snapshot =
            synchronized(lock) {
                queuedCore = actions
                try {
                    block()
                    snapshotLocked()
                } finally {
                    queuedCore = null
                }.also {
                    if (actions.isNotEmpty()) {
                        batch = CoreBatch(actions.toList()).also(coreBatches::addLast)
                        if (coreDispatcher == null) {
                            coreDispatcher = Thread.currentThread()
                            dispatch = true
                        }
                    }
                }
            }
        status.publish(snapshot)

        if (dispatch) {
            dispatchCoreBatches()
        } else if (batch != null && coreDispatcher !== Thread.currentThread()) {
            check(batch!!.finished.await(CORE_DISPATCH_TIMEOUT_SECONDS, java.util.concurrent.TimeUnit.SECONDS)) {
                "Core action dispatcher did not make progress"
            }
        }
    }

    private fun dispatchCoreBatches() {
        while (true) {
            val batch =
                synchronized(lock) {
                    coreBatches.pollFirst()
                        ?: run {
                            coreDispatcher = null
                            return
                        }
                }
            try {
                for (action in batch.actions) dispatchCore(action)
            } finally {
                batch.finished.countDown()
            }
        }
    }

    private fun dispatchCore(action: CoreAction) {
        when (action) {
            CoreAction.PRESS -> core.pressed()
            CoreAction.RELEASE -> core.released()
            CoreAction.CANCEL -> core.cancelPress()
            CoreAction.SUSPEND -> core.suspendTransport()
            CoreAction.RESUME -> core.resumeTransport()
        }
    }

    private fun queue(action: CoreAction) {
        queuedCore?.add(action)
    }

    private fun isStale(session: TransportSession): Boolean {
        val last = lastSession[session.source] ?: return false
        return session.id < last.id
    }

    private fun setSourceState(source: TransportSource, state: TransportState) {
        when (source) {
            TransportSource.USB -> usbState = state
            TransportSource.BLE -> bleState = state
        }
    }

    private fun snapshotLocked(): TransportStatusSnapshot {
        revision += 1
        val active = arbiter.activeSession
        val held = active != null && arbiter.isPressed(active)
        return TransportStatusSnapshot(
            revision = revision,
            code = codeLocked(active),
            active = active?.source?.wireName(),
            usb = usbState.name.lowercase(),
            ble = bleState.name.lowercase(),
            held = held,
            suspended = coreSuspended && active == null,
            paused = hints.userPaused && !coreSuspended,
        )
    }

    private fun codeLocked(active: TransportSession?): String {
        val usbReady = usbState == TransportState.READY
        val bleReady = bleState == TransportState.READY
        val starting = usbState == TransportState.STARTING || bleState == TransportState.STARTING
        return when {
            hints.blePermissionDenied && !usbReady -> TransportStatusHub.CODE_PERMISSION
            active == null && starting -> TransportStatusHub.CODE_RECONNECTING
            coreSuspended && active == null -> TransportStatusHub.CODE_SUSPENDED
            active == null && !usbReady && !bleReady -> TransportStatusHub.CODE_NO_INPUT
            hints.userPaused -> TransportStatusHub.CODE_PAUSED
            active != null -> TransportStatusHub.CODE_READY
            else -> TransportStatusHub.CODE_NO_INPUT
        }
    }

    private companion object {
        const val CORE_DISPATCH_TIMEOUT_SECONDS = 5L
    }
}

private fun TransportSource.wireName(): String =
    when (this) {
        TransportSource.USB -> "usb"
        TransportSource.BLE -> "ble"
    }

/**
 * Process-wide owner so activity recreation and repeated `startForegroundService`
 * calls cannot create a second scanner or a second pair of transport workers.
 */
object OverlayTransportOwner {
    private val lock = Any()
    private var runtime: SwitchTransportRuntime? = null
    val status = TransportStatusHub()
    var scannerStarts = 0
        private set

    fun start(
        usb: InputTransport,
        ble: InputTransport,
        core: CoreInput,
        status: TransportStatusHub = this.status,
        startScanner: () -> Unit,
    ): SwitchTransportRuntime {
        val created =
            synchronized(lock) {
                runtime?.let { return it }
                startScanner()
                scannerStarts += 1
                SwitchTransportRuntime(usb, ble, core, status).also { runtime = it }
            }
        created.start()
        return created
    }

    fun stop() {
        val current =
            synchronized(lock) {
                val runtime = this.runtime
                this.runtime = null
                runtime
            }
        current?.stop()
        status.reset()
    }

    fun setHints(hints: TransportHints) {
        val current = synchronized(lock) { runtime }
        current?.setHints(hints)
    }

    fun setUserPausedFromCore(paused: Boolean) {
        val current = synchronized(lock) { runtime }
        current?.setUserPausedFromCore(paused)
    }

    fun snapshotJson(): String = status.current().toJson()

    fun usbUsable(): Boolean =
        status.current().usb == TransportState.READY.name.lowercase()

    fun resetForTests() {
        val current =
            synchronized(lock) {
                val runtime = this.runtime
                this.runtime = null
                scannerStarts = 0
                runtime
            }
        current?.stop()
        status.reset()
    }
}
