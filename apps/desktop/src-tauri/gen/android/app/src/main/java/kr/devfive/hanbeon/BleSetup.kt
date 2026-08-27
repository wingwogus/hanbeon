package kr.devfive.hanbeon

import java.io.File

data class BleCandidateView(
    val token: String,
    val label: String,
) {
    fun toJson(): String =
        "{" +
            "\"token\":" + jsonString(token) +
            ",\"label\":" + jsonString(label) +
            "}"
}

/** Owns setup scan completion so a stale timeout cannot end a newer scan. */
class BleScanGenerations {
    private var current = 0L
    private var active = 0L

    fun begin(): Long {
        current += 1
        active = current
        return current
    }

    fun stop() {
        active = 0
    }

    fun claimTimeout(generation: Long): Boolean = claim(generation)

    fun claimCallback(generation: Long): Boolean = claim(generation)

    private fun claim(generation: Long): Boolean {
        if (active != generation) return false
        active = 0
        return true
    }
}

data class BleSetupSnapshot(
    val code: String,
    val label: String?,
    val usbUsable: Boolean,
    val readyToConnect: Boolean,
    val canRequestPermission: Boolean,
    val scanning: Boolean,
    val candidates: List<BleCandidateView>,
) {
    fun toJson(): String {
        val items = candidates.joinToString(prefix = "[", postfix = "]") { it.toJson() }
        return "{" +
            "\"code\":" + jsonString(code) +
            ",\"label\":" + (label?.let(::jsonString) ?: "null") +
            ",\"usbUsable\":" + usbUsable +
            ",\"readyToConnect\":" + readyToConnect +
            ",\"canRequestPermission\":" + canRequestPermission +
            ",\"scanning\":" + scanning +
            ",\"candidates\":" + items +
            "}"
    }
}

data class TrustedBleRecord(
    val identity: String,
    val label: String,
)

object BleSetupPolicy {
    const val PERMISSION_SCAN = "android.permission.BLUETOOTH_SCAN"
    const val PERMISSION_CONNECT = "android.permission.BLUETOOTH_CONNECT"
    const val PERMISSION_ADVERTISE = "android.permission.BLUETOOTH_ADVERTISE"
    const val PERMISSION_FINE_LOCATION = "android.permission.ACCESS_FINE_LOCATION"
    const val PERMISSION_LEGACY = "android.permission.BLUETOOTH"
    const val NUS_SERVICE_UUID = "6e400001-b5a3-f393-e0a9-e50e24dcca9e"
    const val TRUSTED_NAME = "HanBeon XIAO"
    const val SAFE_FALLBACK_LABEL = "한번 블루투스 스위치"
    const val CODE_PERMISSION_DENIED = "permission-denied"
    const val CODE_BLUETOOTH_OFF = "bluetooth-off"
    const val CODE_NO_SELECTION = "no-selection"
    const val CODE_SELECTED = "selected"
    const val CODE_UNAVAILABLE = "unavailable"
    const val CODE_SCANNING = "scanning"
    const val PERMISSION_REQUEST = 71

    private val MAC = Regex("(?i)([0-9a-f]{2}:){5}[0-9a-f]{2}")
    private val GATT = Regex("(?i)gatt|status\\s*=?\\s*\\d+")

    fun runtimePermissions(apiLevel: Int): List<String> =
        if (apiLevel >= 31) {
            listOf(PERMISSION_SCAN, PERMISSION_CONNECT)
        } else {
            listOf(PERMISSION_FINE_LOCATION)
        }

    fun shouldRequestFromLifecycle(@Suppress("UNUSED_PARAMETER") deniedOnce: Boolean): Boolean =
        false

    fun shouldRequestFromUserAction(@Suppress("UNUSED_PARAMETER") deniedOnce: Boolean): Boolean =
        true

    fun snapshot(
        hasBleHardware: Boolean,
        permissionGranted: Boolean,
        @Suppress("UNUSED_PARAMETER") deniedOnce: Boolean,
        adapterOn: Boolean,
        selectionLabel: String?,
        scanning: Boolean,
        candidates: List<BleCandidateView>,
        usbUsable: Boolean = true,
    ): BleSetupSnapshot {
        val publicLabel = selectionLabel?.let(::publicLabel)
        val (code, ready) =
            when {
                !hasBleHardware -> CODE_UNAVAILABLE to false
                !permissionGranted -> CODE_PERMISSION_DENIED to false
                !adapterOn -> CODE_BLUETOOTH_OFF to false
                publicLabel == null && scanning -> CODE_SCANNING to false
                publicLabel == null -> CODE_NO_SELECTION to false
                else -> CODE_SELECTED to true
            }
        return BleSetupSnapshot(
            code = code,
            label = publicLabel,
            usbUsable = usbUsable,
            readyToConnect = ready,
            canRequestPermission = hasBleHardware && !permissionGranted,
            scanning = scanning,
            candidates = candidates.filter { isSafeBleLabel(it.label) },
        )
    }

    fun isTrustedCandidate(
        name: String?,
        advertisedServiceUuids: List<String>,
    ): Boolean {
        if (name.isNullOrBlank()) return false
        if (!name.equals(TRUSTED_NAME, ignoreCase = true)) return false
        if (containsForbiddenIdentity(name)) return false
        return advertisedServiceUuids.isEmpty() ||
            advertisedServiceUuids.any { it.equals(NUS_SERVICE_UUID, ignoreCase = true) }
    }

    fun publicLabel(raw: String?): String? {
        if (raw.isNullOrBlank()) return null
        return if (isSafeBleLabel(raw)) raw.trim() else SAFE_FALLBACK_LABEL
    }

    fun candidateView(
        token: String,
        name: String?,
        address: String?,
    ): BleCandidateView {
        val label = publicLabel(name) ?: SAFE_FALLBACK_LABEL
        require(token.isNotBlank())
        require(!containsForbiddenIdentity(token))
        require(!containsForbiddenIdentity(label))
        require(address == null || !label.contains(address, ignoreCase = true))
        return BleCandidateView(token = token, label = label)
    }

    fun isSafeBleLabel(value: String?): Boolean {
        if (value.isNullOrBlank()) return false
        return !containsForbiddenIdentity(value)
    }

    fun containsForbiddenIdentity(value: String): Boolean =
        MAC.containsMatchIn(value) || GATT.containsMatchIn(value)
}

class TrustedBleStore(private val file: File) {
    @Synchronized
    fun load(): TrustedBleRecord? {
        if (!file.isFile) return null
        val raw = runCatching { file.readText() }.getOrNull() ?: return null
        val identity = jsonField(raw, "identity")?.trim().orEmpty()
        val label = jsonField(raw, "label")?.trim().orEmpty()
        if (identity.isEmpty()) return null
        return TrustedBleRecord(identity = identity, label = label)
    }

    @Synchronized
    fun save(record: TrustedBleRecord) {
        val parent = file.parentFile
        if (parent != null && !parent.exists()) parent.mkdirs()
        val label = BleSetupPolicy.publicLabel(record.label) ?: BleSetupPolicy.SAFE_FALLBACK_LABEL
        file.writeText(
            "{" +
                "\"identity\":" + jsonString(record.identity.trim()) +
                ",\"label\":" + jsonString(label) +
                "}",
        )
    }

    @Synchronized
    fun revoke() {
        if (file.exists()) file.delete()
    }

    fun publicSelectionLabel(): String? = BleSetupPolicy.publicLabel(load()?.label)
}

class BleSetupController(
    private val store: TrustedBleStore,
    private val apiLevel: Int,
    private val hasBleHardware: Boolean,
    private val adapterOn: () -> Boolean,
    private val permissionGranted: () -> Boolean,
    private val scan: (onResult: (List<BleCandidateView>) -> Unit) -> Unit,
    private val usbUsable: () -> Boolean = { true },
) {
    @Volatile private var deniedOnce = false
    @Volatile private var scanning = false
    private var candidates: List<BleCandidateView> = emptyList()

    fun snapshot(): BleSetupSnapshot =
        BleSetupPolicy.snapshot(
            hasBleHardware = hasBleHardware,
            permissionGranted = permissionGranted(),
            deniedOnce = deniedOnce,
            adapterOn = adapterOn(),
            selectionLabel = store.publicSelectionLabel(),
            scanning = scanning,
            candidates = candidates,
            usbUsable = usbUsable(),
        )

    fun notePermissionResult(granted: Boolean) {
        if (!granted) deniedOnce = true
        if (granted) deniedOnce = false
    }

    fun requestPermissionIfAllowed(): Boolean {
        if (permissionGranted()) return false
        return BleSetupPolicy.shouldRequestFromUserAction(deniedOnce)
    }

    fun select(token: String, identity: String, name: String?): BleSetupSnapshot {
        val candidate = candidates.firstOrNull { it.token == token }
        val label = candidate?.label ?: BleSetupPolicy.publicLabel(name)
        require(token.isNotBlank())
        require(identity.isNotBlank())
        require(!BleSetupPolicy.containsForbiddenIdentity(token))
        require(label != null)
        store.save(TrustedBleRecord(identity = identity, label = label))
        candidates = emptyList()
        scanning = false
        return snapshot()
    }

    fun revoke(): BleSetupSnapshot {
        store.revoke()
        candidates = emptyList()
        scanning = false
        return snapshot()
    }

    fun beginScan(): BleSetupSnapshot {
        if (!permissionGranted() || !adapterOn()) return snapshot()
        if (!scanning) {
            scanning = true
            scan { found ->
                candidates = found.filter { BleSetupPolicy.isSafeBleLabel(it.label) }
                scanning = false
            }
        }
        return snapshot()
    }

    fun runtimePermissions(): Array<String> =
        BleSetupPolicy.runtimePermissions(apiLevel).toTypedArray()
}

internal fun jsonString(value: String): String =
    buildString {
        append('"')
        for (ch in value) {
            when (ch) {
                '\\' -> append("\\\\")
                '"' -> append("\\\"")
                '\n' -> append("\\n")
                '\r' -> append("\\r")
                '\t' -> append("\\t")
                else -> append(ch)
            }
        }
        append('"')
    }

internal fun jsonField(raw: String, key: String): String? {
    val needle = "\"$key\""
    val start = raw.indexOf(needle)
    if (start < 0) return null
    val colon = raw.indexOf(':', start + needle.length)
    if (colon < 0) return null
    val first = raw.indexOf('"', colon + 1)
    if (first < 0) return null
    val end = raw.indexOf('"', first + 1)
    if (end < 0) return null
    return raw.substring(first + 1, end)
}
