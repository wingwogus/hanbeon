package kr.devfive.hanbeon

/** Source-independent switch edges accepted by the native core boundary. */
enum class InputEdge {
    PRESS,
    RELEASE,
}

enum class TransportSource {
    USB,
    BLE,
}

/** A callback identity. IDs are process-local and never reused. */
data class TransportSession(
    val source: TransportSource,
    val id: Long,
) {
    init {
        require(id > 0) { "session id must be positive" }
    }
}

enum class TransportState {
    STARTING,
    READY,
    LOST,
    STOPPED,
}

/** Platform transports implement this boundary; tests can use a synchronous fake. */
interface InputTransport {
    val source: TransportSource

    fun start(callback: Callback)

    fun stop()

    interface Callback {
        fun onState(session: TransportSession, state: TransportState)

        fun onEdge(session: TransportSession, edge: InputEdge)
    }
}

fun interface TransportClock {
    fun nowMillis(): Long
}

fun interface TransportCancellation {
    fun cancel()
}

fun interface TransportScheduler {
    fun schedule(delayMillis: Long, task: () -> Unit): TransportCancellation
}

class TransportSessionIds {
    private var lastId = 0L

    @Synchronized
    fun next(source: TransportSource): TransportSession {
        check(lastId < Long.MAX_VALUE) { "transport session id exhausted" }
        return TransportSession(source, ++lastId)
    }
}

/** Strict incremental parser for newline-framed ASCII `P` and `R` records. */
class InputRecordParser(private val maxRecordBytes: Int = 32) {
    private val record = ArrayList<Byte>()
    private var discarding = false

    init {
        require(maxRecordBytes > 0) { "maxRecordBytes must be positive" }
    }

    fun feed(bytes: ByteArray): List<InputEdge> {
        val edges = mutableListOf<InputEdge>()
        for (byte in bytes) {
            if (byte == NEWLINE) {
                if (!discarding) decodeRecord()?.let(edges::add)
                record.clear()
                discarding = false
            } else if (!discarding) {
                if (byte.toInt() !in ASCII_MIN..ASCII_MAX || record.size == maxRecordBytes) {
                    record.clear()
                    discarding = true
                } else {
                    record.add(byte)
                }
            }
        }
        return edges
    }

    private fun decodeRecord(): InputEdge? {
        val size = if (record.lastOrNull() == CARRIAGE_RETURN) record.size - 1 else record.size
        if (size != 1) return null
        return when (record[0]) {
            PRESS -> InputEdge.PRESS
            RELEASE -> InputEdge.RELEASE
            else -> null
        }
    }

    private companion object {
        const val ASCII_MIN = 0
        const val ASCII_MAX = 0x7f
        const val NEWLINE: Byte = 0x0a
        const val CARRIAGE_RETURN: Byte = 0x0d
        const val PRESS: Byte = 0x50
        const val RELEASE: Byte = 0x52
    }
}

fun interface InputEdgeSink {
    fun accept(edge: InputEdge)
}

/**
 * Serialized source selector. Callers invoke it from one service-owned execution context.
 * Standby edges update only local state and cannot complete the selected session's pair.
 */
class TransportArbiter(private val sink: InputEdgeSink) {
    private data class SessionState(var pressed: Boolean = false)

    private val currentBySource = mutableMapOf<TransportSource, TransportSession>()
    private val states = mutableMapOf<TransportSession, SessionState>()

    var activeSession: TransportSession? = null
        private set

    fun sessionFor(source: TransportSource): TransportSession? = currentBySource[source]

    fun isPressed(session: TransportSession): Boolean = states[session]?.pressed == true

    fun ready(session: TransportSession) {
        val current = currentBySource[session.source]
        if (current != null && session.id <= current.id) return

        currentBySource[session.source] = session
        states[session] = SessionState()
        if (activeSession == null) {
            activeSession = session
        } else if (activeSession == current && states[current]?.pressed == false) {
            states.remove(current)
            activeSession = session
        }
    }

    fun lost(session: TransportSession) {
        if (currentBySource[session.source] != session) return
        currentBySource.remove(session.source)
        states.remove(session)
        if (activeSession == session) activeSession = null
    }

    fun edge(session: TransportSession, edge: InputEdge) {
        if (currentBySource[session.source] != session && activeSession != session) return
        val state = states[session] ?: return
        val nextPressed = edge == InputEdge.PRESS
        if (state.pressed == nextPressed) return
        state.pressed = nextPressed
        if (activeSession == session) sink.accept(edge)

        if (!nextPressed) promoteReplacementFor(session)
    }

    fun select(session: TransportSession): Boolean {
        if (currentBySource[session.source] != session) return false
        val active = activeSession
        if (states[session]?.pressed != false || (active != null && states[active]?.pressed != false)) {
            return false
        }
        activeSession = session
        return true
    }

    private fun promoteReplacementFor(session: TransportSession) {
        if (activeSession != session) return
        val replacement = currentBySource[session.source] ?: return
        if (replacement != session && states[replacement]?.pressed == false) {
            states.remove(session)
            activeSession = replacement
        }
    }
}
